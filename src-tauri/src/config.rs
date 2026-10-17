//! config.toml 的解析与默认模板（spec §5）。
//!
//! 设计原则（spec §5.2）：**任何问题都回退到默认值，绝不报错**。
//! 只有一个文件不存在时才写默认模板；文件坏了就只读不写，留给用户自己修。
//!
//! 逐字段读取（而不是整份 deserialize 成一个 struct）是有意的：
//! 整份反序列化时，一个字段类型错会让整份文件失效；spec 要求
//! 「单个字段值非法 → 该字段用默认值，其余照常生效」。

use activity_engine::EngineConfig;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const MIN_IDLE_THRESHOLD_S: u32 = 10;
pub const MIN_HEARTBEAT_S: u32 = 1;

/// 上限：一天。超过它的配置值没有意义，夹住可以防止 u32 的极值把
/// 时间线切成几十万段。
const MAX_SECONDS: u32 = 86_400;
const MAX_HEARTBEAT_S: u32 = 600;

/// 原始事件的默认保留天数（B3）。365 天够覆盖热力图默认窗口
/// （`wallWindow` 是 53 周 ≈371 天）。
pub const DEFAULT_RETENTION_DAYS: u32 = 365;
/// 上限 100 年。超出的一律夹到这里 =「近乎永不删」，
/// 这是用户手滑写了个 10^9 时最安全的解释。
pub const MAX_RETENTION_DAYS: u32 = 36_500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseBehavior {
    /// 首次关窗时询问；用户的选择会把本项覆写成 minimize/quit
    Ask,
    /// 直接隐藏到托盘
    Minimize,
    /// 直接退出
    Quit,
}

impl CloseBehavior {
    pub fn as_str(&self) -> &'static str {
        match self {
            CloseBehavior::Ask => "ask",
            CloseBehavior::Minimize => "minimize",
            CloseBehavior::Quit => "quit",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ask" => Some(CloseBehavior::Ask),
            "minimize" => Some(CloseBehavior::Minimize),
            "quit" => Some(CloseBehavior::Quit),
            _ => None,
        }
    }
}

// ===========================================================================
// 「一个配置项」的唯一所有者
//
// 2026-10-17。在这之前，一个配置项的知识散在**八处**：结构体字段、`Default`、
// `parse` 的逐字段夹取、`to_toml`、`set_config` 的外层 match、同一个函数里
// 第二层 match（把夹取后的值再取回来一次）、`CONFIG_KEYS`、
// `config_field_needs_restart`。代码自己都在报怨 —— 那个第二层 match 里有个
// arm 的报错文案写着「外层 match 加了键这里要一起改」。
//
// 现在加一个字段只需要：① 结构体加字段（typed 边界，必须显式）；
// ② 这张表加一行；③ 前端 `FIELDS` 加一项文案。判定不再各写一遍。
// ===========================================================================

/// 一项的类型、取值范围与默认值。
pub enum FieldKind {
    /// 整数（秒 / 天）。缺字段、类型不对、文件坏了都回退到 `default`。
    Int { default: u32, min: u32, max: u32 },
    Bool { default: bool },
    /// 枚举。`values` 同时是**合法取值**与**报错文案**的一部分。
    Enum {
        values: &'static [&'static str],
        default: &'static str,
    },
}

pub struct FieldSpec {
    /// `config.toml` 里的键名（snake_case）。也是前端 `needsRestart` 的键。
    pub key: &'static str,
    pub kind: FieldKind,
    /// 改完是否**必须重启**才生效。依据是参数绑在哪里，不是界面偏好：
    /// - 四个采集/引擎参数：绑进 `spawn_input_poller` 的线程参数与
    ///   `EngineConfig` 的构造注入，进程内改不了（STATUS §4.1）
    /// - `autostart`：`Residency` 启动时快照 + 系统注册表，`set_config` 碰不到
    /// - `close_behavior`：关窗拦截**现读 config.toml**，真热生效
    /// - `events_retention_days`：`set_config` 回写内存，下一次轮询就按新值清理
    pub needs_restart: bool,
}

/// `close_behavior` 的合法取值。与 `CloseBehavior::from_str` 必须一致
/// （有测试钉着）。
pub const CLOSE_BEHAVIORS: &[&str] = &["ask", "minimize", "quit"];

pub const FIELDS: &[FieldSpec] = &[
    FieldSpec {
        key: "idle_threshold_s",
        // 下限 10：idle_threshold = 0 会让每条事件都切段，一天几万段
        kind: FieldKind::Int { default: 300, min: MIN_IDLE_THRESHOLD_S, max: MAX_SECONDS },
        needs_restart: true,
    },
    FieldSpec {
        key: "min_segment_duration_s",
        kind: FieldKind::Int { default: 30, min: 0, max: MAX_SECONDS },
        needs_restart: true,
    },
    FieldSpec {
        key: "heartbeat_every_s",
        kind: FieldKind::Int {
            default: 10,
            min: MIN_HEARTBEAT_S,
            max: MAX_HEARTBEAT_S,
        },
        needs_restart: true,
    },
    FieldSpec {
        key: "events_retention_days",
        // 0 是有效取值（永不删），所以下限就是 0
        kind: FieldKind::Int { default: DEFAULT_RETENTION_DAYS, min: 0, max: MAX_RETENTION_DAYS },
        needs_restart: false,
    },
    FieldSpec {
        key: "autostart",
        kind: FieldKind::Bool { default: false },
        needs_restart: true,
    },
    FieldSpec {
        key: "close_behavior",
        kind: FieldKind::Enum { values: CLOSE_BEHAVIORS, default: "ask" },
        needs_restart: false,
    },
];

pub fn field(key: &str) -> Option<&'static FieldSpec> {
    FIELDS.iter().find(|f| f.key == key)
}

/// 这个键改完要重启吗？**未知键一律当作要重启** —— 没登记过的键宁可多提醒
/// 一次重启，也不要悄悄标成「已生效」。
pub fn needs_restart(key: &str) -> bool {
    field(key).map(|f| f.needs_restart).unwrap_or(true)
}

/// 每个键 → 改完是否要重启。界面那个「重启后生效」角标读的就是它。
pub fn needs_restart_map() -> BTreeMap<&'static str, bool> {
    FIELDS.iter().map(|f| (f.key, f.needs_restart)).collect()
}

/// 全部可改键。`set_config` 收不收这个键、界面画不画角标，都以它为准。
fn int_default(key: &str) -> u32 {
    match field(key).map(|f| &f.kind) {
        Some(FieldKind::Int { default, .. }) => *default,
        _ => panic!("FIELDS 里 `{key}` 必须登记成 Int —— 表和结构体对不上"),
    }
}

fn bool_default(key: &str) -> bool {
    match field(key).map(|f| &f.kind) {
        Some(FieldKind::Bool { default }) => *default,
        _ => panic!("FIELDS 里 `{key}` 必须登记成 Bool —— 表和结构体对不上"),
    }
}

fn enum_default(key: &str) -> &'static str {
    match field(key).map(|f| &f.kind) {
        Some(FieldKind::Enum { default, .. }) => default,
        _ => panic!("FIELDS 里 `{key}` 必须登记成 Enum —— 表和结构体对不上"),
    }
}

/// 把用户填的字符串变成**可以写回文件的 TOML 文本**。校验与夹取都在这里，
/// 于是 `set_config` 不再需要「拼一份假 TOML 再解析回来再 match 一次」那两步。
///
/// 返回值第二项是数字键的新取值（保留期靠它热生效），非数字键为 `None`。
pub fn render_value(key: &str, raw: &str) -> Result<(String, Option<u32>), String> {
    let spec = field(key).ok_or_else(|| format!("未知配置项 `{key}`"))?;
    match spec.kind {
        FieldKind::Int { min, max, .. } => {
            let n: i64 = raw.parse().map_err(|_| format!("`{raw}` 不是数字"))?;
            let clamped = n.clamp(min as i64, max as i64) as u32;
            Ok((clamped.to_string(), Some(clamped)))
        }
        FieldKind::Bool { .. } => match raw {
            "true" => Ok(("true".to_string(), None)),
            "false" => Ok(("false".to_string(), None)),
            other => Err(format!("`{other}` 不是 true / false")),
        },
        FieldKind::Enum { values, .. } => {
            if values.contains(&raw) {
                Ok((format!("\"{raw}\""), None))
            } else {
                Err(format!("`{key}` 只能是 {}", values.join(" / ")))
            }
        }
    }
}

/// 校验并写回一个键。**只改磁盘上的那一行**（`patch_line`），
/// 不覆盖整份文件、不抹掉注释；文件坏了就拒绝写入并把原因带回去。
pub fn set_value(path: &Path, key: &str, raw: &str) -> Result<SetOutcome, String> {
    let (rendered, int) = render_value(key, raw)?;
    patch_line(path, key, &rendered).map_err(|e| e.to_string())?;
    Ok(SetOutcome {
        needs_restart: needs_restart(key),
        int,
    })
}

pub struct SetOutcome {
    pub needs_restart: bool,
    /// 数字键的新取值 —— 保留期靠它热生效。
    pub int: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppConfig {
    /// 无输入多少秒算空闲
    pub idle_threshold_s: u32,
    /// 短于此的段并入相邻段
    pub min_segment_duration_s: u32,
    /// 心跳聚合窗口
    pub heartbeat_every_s: u32,
    /// 开机自启
    pub autostart: bool,
    /// 点 ✕ 时的行为
    pub close_behavior: CloseBehavior,
    /// 保留多少天的**原始事件**。0 = 永不删（spec §8.1，B3）。
    ///
    /// 只管 `events`（原始流，实测 ≈5 MB/天 ≈1.8 GB/年）。
    /// **`activities` 一行不删** —— 它才是时长账本，而且很小
    /// （≈50k 行/年）。所以时间线、汇总、热力图不受这个设置影响，
    /// 受影响的只有「窗口标题 Top」与段详情里能回溯到的标题范围。
    pub events_retention_days: u32,
}

impl Default for AppConfig {
    /// 各项默认值从 `FIELDS` 取，**不再各写一遍** —— 原来这里与 `parse` 里的
    /// `unwrap_or(d.xxx)` 是同一批数字的两份副本。
    fn default() -> Self {
        Self {
            idle_threshold_s: int_default("idle_threshold_s"),
            min_segment_duration_s: int_default("min_segment_duration_s"),
            heartbeat_every_s: int_default("heartbeat_every_s"),
            autostart: bool_default("autostart"),
            close_behavior: CloseBehavior::from_str(enum_default("close_behavior"))
                .expect("FIELDS 里的枚举默认值必须是 CloseBehavior 认识的取值"),
            events_retention_days: int_default("events_retention_days"),
        }
    }
}

impl AppConfig {
    pub fn to_engine_config(self) -> EngineConfig {
        EngineConfig::default()
            .with_idle_threshold_s(self.idle_threshold_s as u64)
            .with_min_segment_duration_s(self.min_segment_duration_s as u64)
    }
}

/// **纯函数**：任何输入都返回一个可用的配置。
///
/// 逐字段取值：类型对不上、越界、缺失都只影响那一个字段。
/// **夹取范围与默认值从 `FIELDS` 取**，不再在这里各写一遍。
pub fn parse(src: &str) -> AppConfig {
    let d = AppConfig::default();
    let Ok(table) = src.parse::<toml::Table>() else {
        eprintln!("[time-scope] config.toml 解析失败，本次使用默认配置（文件未被修改）");
        return d;
    };

    // 整数字段：非整数（字符串/浮点/数组）就当没写；越界按表里的范围夹住。
    // 「负数天」最合理的解释是「不要按天数删」，下限 0 表达的就是这个。
    let int = |key: &str| -> Option<u32> {
        let v = table.get(key)?.as_integer()?;
        match field(key).map(|f| &f.kind) {
            Some(FieldKind::Int { min, max, .. }) => Some(v.clamp(*min as i64, *max as i64) as u32),
            _ => None,
        }
    };
    let boolean = |key: &str| -> Option<bool> { table.get(key).and_then(|v| v.as_bool()) };
    let text = |key: &str| -> Option<&str> { table.get(key).and_then(|v| v.as_str()) };

    AppConfig {
        idle_threshold_s: int("idle_threshold_s").unwrap_or(d.idle_threshold_s),
        min_segment_duration_s: int("min_segment_duration_s")
            .unwrap_or(d.min_segment_duration_s),
        heartbeat_every_s: int("heartbeat_every_s").unwrap_or(d.heartbeat_every_s),
        autostart: boolean("autostart").unwrap_or(d.autostart),
        close_behavior: text("close_behavior")
            .and_then(CloseBehavior::from_str)
            .unwrap_or(d.close_behavior),
        events_retention_days: int("events_retention_days").unwrap_or(d.events_retention_days),
    }
}

/// 数据目录的名字。**有前缀**，放在 exe 旁边一眼能认出是谁生成的，
/// 也不会和别人的 `data\` 撞（2026-10-17 用户要求）。
pub const DATA_DIR_NAME: &str = "time-scope-data";

/// 库 / `config.toml` / `rules.toml` 住哪 —— **「程序」所在目录下的 `time-scope-data\`**。
///
/// 一条规则：**程序在哪，账本就在哪**。不区分「安装版 / 便携版」——
/// 它们是同一个软件，凭什么走两条路。拷贝一份 exe 到 U 盘，账本跟着走；
/// 删掉目录 = 连数据一起删，这才是「便携」该有的样子。
///
/// **与操作系统无关**：这里只用 `current_exe()` 与路径拼接 —— 没有
/// `%APPDATA%`、没有注册表、没有 `dirs` 之类的平台库。同一份规则在 Linux /
/// macOS 上一样成立，只要程序本身待在一个**可写**的目录里。
///
/// 但「程序」在三个平台上长的不一样，于是有两条容器规则（见 `app_container`）：
/// - **macOS 的 `.app`**：exe 在 `Foo.app/Contents/MacOS/foo` 里。往那里写会
///   破坏代码签名、用户也看不见，而且更新会整个替换掉 bundle —— 所以认
///   那个 `.app` **文件夹**为「程序」，数据放在它**旁边**。
/// - **Linux 的 `.AppImage`**：exe 在一个只读且临时的 squashfs 挂载点里
///   （`/tmp/.mount_*`）。运行时 `$APPIMAGE` 指向那个 `.AppImage` 文件本身 ——
///   数据就放在它旁边。
///
/// 唯一例外是 `TIME_SCOPE_DATA` 显式覆盖：开发时把库指到别处（别让 dev
/// 写真实账本）、或者你想把账本放在另一块盘上。它是一个开关，不是一条分支。
///
/// **前提**：放置程序的目录必须可写。这不是本项目的选择，是各系统的规则：
/// - Windows：默认的 NSIS 是当前用户安装（`installMode: currentUser`），可写；
///   装进 `C:\Program Files`（per-machine / MSI）普通用户写不进去
/// - Linux：`/usr/bin`、Flatpak / Snap 的沙箱都是只读 —— 那里只能走
///   `TIME_SCOPE_DATA` 或发行版自带的数据目录
/// - macOS：`/Applications` 对普通用户只读
///
/// 写不进去时**启动会在建目录那一步失败并弹出带路径的说明**，
/// 而不是悄悄把数据放到别处（那才是真的找不到账本）。
pub fn data_dir() -> PathBuf {
    resolve_data_dir(
        std::env::var_os("TIME_SCOPE_DATA"),
        std::env::var_os("APPIMAGE"),
        std::env::current_exe().ok().as_deref(),
    )
}

/// 纯逻辑部分（好测）：给定「覆盖值 / AppImage / 自身路径」，算出数据目录。
///
/// 三个入参都是显式传进来的，于是这三个分支不需要真的跑在三个系统上才能测。
fn resolve_data_dir(
    override_dir: Option<std::ffi::OsString>,
    appimage: Option<std::ffi::OsString>,
    exe: Option<&Path>,
) -> PathBuf {
    if let Some(dir) = override_dir {
        let p = PathBuf::from(dir);
        if !p.as_os_str().is_empty() {
            return p;
        }
    }
    // .AppImage：exe 在只读挂载点里，用 `$APPIMAGE` 指向的那个文件作锚
    if let Some(img) = appimage {
        let p = PathBuf::from(img);
        if let Some(parent) = p.parent() {
            return parent.join(DATA_DIR_NAME);
        }
    }
    let exe = exe.expect("取不到自身路径（current_exe）—— 无法确定数据该放哪");
    app_container(exe)
        .parent()
        .expect("程序所在目录没有父目录 —— 无法确定数据该放哪")
        .join(DATA_DIR_NAME)
}

/// 「程序」在文件系统上的那个东西：macOS 的 `.app` 文件夹，否则就是 exe 本身。
///
/// 判据只看路径里有没有 `.app/` 这一段 —— 不需要 `cfg!(target_os)`，
/// 于是这条测试在 Windows 上也能跑（喂一个 macOS 形状的路径即可）。
fn app_container(exe: &Path) -> PathBuf {
    let comps: Vec<_> = exe.components().collect();
    if let Some(i) = comps.iter().rposition(|c| {
        c.as_os_str()
            .to_string_lossy()
            .to_ascii_lowercase()
            .ends_with(".app")
    }) {
        return comps[..=i].iter().collect();
    }
    exe.to_path_buf()
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.toml")
}

/// 读配置；文件不存在则写默认模板。
///
/// **文件坏了不覆盖**（spec §5.2）：那是用户自己的内容，留着给他修。
pub fn load_or_create(path: &Path) -> AppConfig {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(_) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if std::fs::write(path, DEFAULT_CONFIG_TOML).is_ok() {
                eprintln!("[time-scope] 已生成默认配置 {}", path.display());
            }
            AppConfig::default()
        }
    }
}

/// 改一个键并写回，**只动那一行**。
///
/// 为什么不整份重写：模板里的注释是给用户看的说明，整份重写会把它们全部抹掉；
/// 而且文件一旦被用户改坏，整读再整写会用默认值**覆盖**他写的东西（spec §5.2）。
///
/// `rendered_value` 是已经渲染好的 TOML 值，例如 `true`、`"minimize"`。
/// - 文件不存在：写默认模板 + 追加这一行
/// - 文件存在但 TOML 语法错：**不写**，返回 Err。错误信息只说「未写回」——
///   「本次选择还有没有生效」各调用方不一样，由调用方自己补，不要在这里替它们断言
/// - 键不存在：追加到末尾
pub fn patch_line(path: &Path, key: &str, rendered_value: &str) -> std::io::Result<()> {
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };

    let mut text = match existing {
        Some(t) => {
            if t.parse::<toml::Table>().is_err() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{key} 未写回：config.toml 已是无效 TOML，请手工修好"),
                ));
            }
            t
        }
        None => DEFAULT_CONFIG_TOML.to_string(),
    };

    // 匹配整行的 `key = ...`（允许缩进），避免误伤 close_behavior_hint 这类前缀相同的键
    let is_target = |line: &str| -> bool {
        let l = line.trim_start();
        match l.strip_prefix(key) {
            Some(rest) => rest.starts_with(' ') || rest.starts_with('='),
            None => false,
        }
    };
    let replacement = format!("{key} = {rendered_value}");

    let mut replaced = false;
    text = text
        .lines()
        .map(|line| {
            if is_target(line) {
                replaced = true;
                replacement.clone()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !replaced {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&replacement);
        text.push('\n');
    } else if !text.ends_with('\n') {
        text.push('\n');
    }

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, text)
}

pub const DEFAULT_CONFIG_TOML: &str = r#"# Time Scope 行为参数
#
# 多数项改完要重启（暂不支持热重载）；events_retention_days 与
# close_behavior 是热生效的，不用重启。
# 任何一项写错或缺失都会回退到默认值，不会导致应用起不来。

# 无输入多少秒判定为空闲。最小 10。
idle_threshold_s = 300

# 短于这个时长的段会并入相邻段。0 = 关掉合并。
min_segment_duration_s = 30

# 输入心跳的聚合窗口（秒）。
heartbeat_every_s = 10

# 开机自启。默认关——替用户决定要不要自动启动并记录窗口标题不合适。
autostart = false

# 点窗口的 ✕ 时：ask = 问一次 / minimize = 最小化到托盘 / quit = 退出
# 改完不用重启 —— 下一次点 ✕ 就照新的走（关窗拦截现读这个文件）。
close_behavior = "ask"

# 保留多少天的**原始窗口事件**。0 = 永不删。
#
# 原始事件是数据流本身，实测约 5 MB/天、一年约 1.8 GB；不设上限就会一直涨。
# 但删掉它不影响你的时长统计：activities（时间线、汇总、热力图用的那一批）
# 永远保留。受影响的只有「窗口标题 Top」与段详情里能回溯到的标题范围。
#
# 365 天够覆盖热力图默认的那面墙（53 周 ≈ 371 天）。想留更久就调大，
# 想彻底不删就填 0（代价是磁盘一直涨）。
#
# 改完不用重启。清理本身有 24 小时节流，所以调小不会立刻删 ——
# 要等上次清理满 24 小时后的下一次读取才会按新值动手。
events_retention_days = 365
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_spec() {
        let c = AppConfig::default();
        assert_eq!(c.idle_threshold_s, 300);
        assert_eq!(c.min_segment_duration_s, 30);
        assert_eq!(c.heartbeat_every_s, 10);
        assert!(!c.autostart);
        assert_eq!(c.close_behavior, CloseBehavior::Ask);
    }

    #[test]
    fn parses_a_complete_file() {
        let c = parse(
            r#"
idle_threshold_s = 120
min_segment_duration_s = 5
heartbeat_every_s = 30
autostart = true
close_behavior = "minimize"
"#,
        );
        assert_eq!(c.idle_threshold_s, 120);
        assert_eq!(c.min_segment_duration_s, 5);
        assert_eq!(c.heartbeat_every_s, 30);
        assert!(c.autostart);
        assert_eq!(c.close_behavior, CloseBehavior::Minimize);
    }

    #[test]
    fn an_empty_file_yields_defaults() {
        assert_eq!(parse(""), AppConfig::default());
    }

    #[test]
    fn malformed_toml_yields_defaults() {
        // 用户手改坏文件不该让应用起不来
        assert_eq!(parse("[[[ not toml"), AppConfig::default());
    }

    #[test]
    fn missing_fields_keep_their_defaults() {
        let c = parse("idle_threshold_s = 42\n");
        assert_eq!(c.idle_threshold_s, 42, "写了的字段生效");
        assert_eq!(c.heartbeat_every_s, 10, "没写的字段用默认");
    }

    #[test]
    fn a_wrongly_typed_field_falls_back_without_taking_the_file_down() {
        // 一个字段类型错，不该让其余字段也失效
        let c = parse("idle_threshold_s = \"soon\"\nheartbeat_every_s = 15\n");
        assert_eq!(c.idle_threshold_s, 300, "类型错的字段回退默认");
        assert_eq!(c.heartbeat_every_s, 15, "其余字段照常生效");
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        // Review Focus #3：idle_threshold=0 会让每条事件都切段，一天几万段
        let c = parse("idle_threshold_s = 0\nheartbeat_every_s = -5\n");
        assert_eq!(c.idle_threshold_s, MIN_IDLE_THRESHOLD_S);
        assert_eq!(c.heartbeat_every_s, MIN_HEARTBEAT_S);
    }

    #[test]
    fn zero_is_allowed_where_zero_is_meaningful() {
        // min_segment=0 / retention=0 有明确含义（关掉该行为），不该被夹走
        let c = parse("min_segment_duration_s = 0\nevents_retention_days = 0\n");
        assert_eq!(c.min_segment_duration_s, 0);
        assert_eq!(c.events_retention_days, 0);
    }

    #[test]
    fn an_unknown_close_behavior_falls_back_to_ask() {
        assert_eq!(
            parse("close_behavior = \"explode\"").close_behavior,
            CloseBehavior::Ask
        );
        // 大小写与空格都容忍
        assert_eq!(
            parse("close_behavior = \" Minimize \"").close_behavior,
            CloseBehavior::Minimize
        );
    }

    #[test]
    fn close_behavior_roundtrips_through_str() {
        for b in [CloseBehavior::Ask, CloseBehavior::Minimize, CloseBehavior::Quit] {
            assert_eq!(CloseBehavior::from_str(b.as_str()), Some(b));
        }
        assert_eq!(CloseBehavior::from_str("nope"), None);
    }

    #[test]
    fn every_field_survives_a_write_then_parse_round_trip() {
        // 走**生产那条路**（`set_value` → `patch_line` → `parse`），
        // 而不是另写一个序列化器再验它 —— 那会多出一份「键名清单」，
        // 两份清单迟早有一份忘了改。
        let dir = std::env::temp_dir().join(format!("ts-cfg-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");

        for (key, raw) in [
            ("idle_threshold_s", "120"),
            ("min_segment_duration_s", "45"),
            ("heartbeat_every_s", "5"),
            ("events_retention_days", "180"),
            ("autostart", "true"),
            ("close_behavior", "quit"),
        ] {
            set_value(&path, key, raw).expect("每个登记过的键都必须写得进去");
        }

        let back = parse(&std::fs::read_to_string(&path).unwrap());
        assert_eq!(back.idle_threshold_s, 120);
        assert_eq!(back.min_segment_duration_s, 45);
        assert_eq!(back.heartbeat_every_s, 5);
        assert_eq!(back.events_retention_days, 180);
        assert!(back.autostart);
        assert_eq!(back.close_behavior, CloseBehavior::Quit);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn engine_config_carries_the_three_segment_params() {
        let c = AppConfig {
            idle_threshold_s: 111,
            min_segment_duration_s: 33,
            ..AppConfig::default()
        };
        let e = c.to_engine_config();
        assert_eq!(e.idle_threshold_s, 111);
        assert_eq!(e.min_segment_duration_s, 33);
    }

    #[test]
    fn default_toml_parses_to_defaults() {
        // 自检：内置模板必须真的能被自己解析
        assert_eq!(parse(DEFAULT_CONFIG_TOML), AppConfig::default());
    }

    #[test]
    fn a_broken_file_is_never_overwritten() {        // spec §5.2 / §7：文件坏了只读不写，那是用户自己的内容
        let dir = std::env::temp_dir().join(format!("ts-cfg-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.toml");
        std::fs::write(&path, "idle_threshold_s = \"soon\"\nautostart = true\n").unwrap();

        let c = load_or_create(&path);
        assert_eq!(c.idle_threshold_s, 300);
        assert!(c.autostart, "其余字段照常生效");

        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(
            on_disk.contains("\"soon\""),
            "用户的文件必须原样保留，磁盘内容变成了：{on_disk}"
        );

        // 文件不存在时才写默认模板
        let fresh = dir.join("fresh.toml");
        let c2 = load_or_create(&fresh);
        assert_eq!(c2, AppConfig::default());
        assert_eq!(std::fs::read_to_string(&fresh).unwrap(), DEFAULT_CONFIG_TOML);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- 写回单个键（行级）---

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ts-cfg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn patching_a_key_keeps_the_comments_and_the_other_keys() {
        // 写回整份文件会把模板里的注释全部抹掉，用户就看不到每个参数的说明了
        let dir = tmp_dir("patch-keep");
        let path = dir.join("config.toml");
        std::fs::write(&path, DEFAULT_CONFIG_TOML).unwrap();

        patch_line(&path, "close_behavior", "\"minimize\"").unwrap();

        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains("close_behavior = \"minimize\""));
        assert!(
            after.contains("# 点窗口的 ✕ 时"),
            "注释必须原样保留：\n{after}"
        );
        assert!(after.contains("idle_threshold_s = 300"), "别的键不能丢");
        assert_eq!(parse(&after).close_behavior, CloseBehavior::Minimize);
        // 往返：写回后的文件仍能被自己解析
        assert_eq!(parse(&after).idle_threshold_s, 300);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn patching_appends_a_key_that_is_not_there() {
        let dir = tmp_dir("patch-append");
        let path = dir.join("config.toml");
        std::fs::write(&path, "idle_threshold_s = 42\n").unwrap();

        patch_line(&path, "autostart", "true").unwrap();

        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains("idle_threshold_s = 42"));
        assert!(after.contains("autostart = true"));
        assert!(parse(&after).autostart);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn patching_a_file_with_duplicate_keys_reports_failure_and_keeps_the_file() {
        // 重复键让整个文件解析失败 -> 属于"用户改坏了"，不碰
        let dir = tmp_dir("patch-dup");
        let path = dir.join("config.toml");
        let original = "autostart = false\nidle_threshold_s = 42\nautostart = false\n";
        std::fs::write(&path, original).unwrap();

        let err = patch_line(&path, "autostart", "true").unwrap_err();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert!(err.to_string().contains("未写回"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn patching_a_file_with_a_wrong_typed_value_still_works() {
        // 值类型错（比如写成字符串）**仍是合法 TOML**，所以只改我们那一行，
        // 不顺手"修"用户那一项——他在文件里写什么就留着什么
        let dir = tmp_dir("patch-wrongtype");
        let path = dir.join("config.toml");
        std::fs::write(&path, "idle_threshold_s = \"soon\"\nautostart = false\n").unwrap();

        patch_line(&path, "autostart", "true").unwrap();

        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains("idle_threshold_s = \"soon\""), "内容：{after}");
        assert!(after.contains("autostart = true"));
        // 类型错的那项仍回退默认（spec §5.2）
        assert_eq!(parse(&after).idle_threshold_s, 300);
        assert!(parse(&after).autostart);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn patching_a_broken_file_reports_failure_and_keeps_the_file() {
        // spec §5.2：文件坏了是用户自己的内容，不能因为我们想写一个键就抹掉
        let dir = tmp_dir("patch-broken");
        let path = dir.join("config.toml");
        let original = "idle_threshold_s = \"soon\n# 我改坏了\n";
        std::fs::write(&path, original).unwrap();

        let err = patch_line(&path, "autostart", "true").unwrap_err();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert!(
            err.to_string().contains("未写回"),
            "错误信息要告诉用户本次选择只在本进程内生效：{err}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn patching_a_missing_file_writes_the_template_plus_the_key() {
        let dir = tmp_dir("patch-missing");
        let path = dir.join("config.toml");

        patch_line(&path, "autostart", "true").unwrap();

        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains("autostart = true"));
        let c = parse(&after);
        assert!(c.autostart);
        assert_eq!(c.idle_threshold_s, 300, "其余项应是默认值");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_key_prefix_is_not_mistaken_for_the_key_itself() {
        // 改 close_behavior 不应该动到 close_behavior_hint
        let dir = tmp_dir("patch-prefix");
        let path = dir.join("config.toml");
        std::fs::write(&path, "close_behavior_hint = 1\nclose_behavior = \"ask\"\n").unwrap();

        patch_line(&path, "close_behavior", "\"quit\"").unwrap();

        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains("close_behavior_hint = 1"), "内容：{after}");
        assert!(after.contains("close_behavior = \"quit\""));

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod retention_tests {
    use super::*;

    #[test]
    fn default_keeps_a_year_of_events() {
        // B3。events 表实测 ≈5 MB/天 ≈1.8 GB/年，无限增长。
        // 默认保留 365 天 —— 够覆盖热力图默认窗口（`wallWindow` 是 53 周
        // ≈371 天），一年后约 1.4 GB 而不是 1.8 GB。
        assert_eq!(AppConfig::default().events_retention_days, 365);
    }

    #[test]
    fn the_key_is_read_from_config_toml() {
        let c = parse("events_retention_days = 90");
        assert_eq!(c.events_retention_days, 90);
    }

    #[test]
    fn zero_means_never_prune() {
        // 0 必须是有效取值而不是被夹到下限 —— 「我就是要全留」是个合理诉求。
        let c = parse("events_retention_days = 0");
        assert_eq!(c.events_retention_days, 0);
    }

    #[test]
    fn a_missing_or_broken_value_falls_back_to_the_default() {
        assert_eq!(parse("").events_retention_days, 365);
        assert_eq!(parse("events_retention_days = \"一年\"").events_retention_days, 365);
        assert_eq!(parse("这不是 TOML").events_retention_days, 365);
    }

    #[test]
    fn absurd_values_are_clamped_rather_than_trusted() {
        // 手滑写了个 10^9 也不该真删成 27 亿天后 —— 夹到上限，
        // 等于「近乎永不删」，这是出错时最安全的解释。
        assert_eq!(
            parse("events_retention_days = 1000000000").events_retention_days,
            MAX_RETENTION_DAYS
        );
        assert_eq!(
            parse("events_retention_days = -5").events_retention_days,
            0,
            "负数当 0（永不删），不是当成 1970 年前"
        );
    }

    #[test]
    fn the_default_template_documents_the_key() {
        // 用户得能在模板里看到这一项及其含义，否则没人会改。
        assert!(DEFAULT_CONFIG_TOML.contains("events_retention_days"));
        let c = parse(DEFAULT_CONFIG_TOML);
        assert_eq!(c.events_retention_days, 365, "模板里的默认值必须与代码一致");
    }

    #[test]
    fn the_key_survives_a_write_then_parse_round_trip() {
        let dir = std::env::temp_dir().join(format!("ts-cfg-rt2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        set_value(&path, "events_retention_days", "180").unwrap();
        let back = parse(&std::fs::read_to_string(&path).unwrap());
        assert_eq!(back.events_retention_days, 180);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- 配置项的唯一所有者：FIELDS ---

    /// 表与结构体必须一一对应。**加字段忘了登记表、或表里写错类型**，
    /// 这里立刻红 —— 而不是等界面上少一个控件、或那个键改不进去。
    ///
    /// 默认值写死在这里是故意的：改默认值应当是个显眼的动作。
    #[test]
    fn the_field_table_matches_the_struct() {
        assert_eq!(FIELDS.len(), 6, "config.toml 的可改键变了就同步这里");
        assert_eq!(
            AppConfig::default(),
            AppConfig {
                idle_threshold_s: 300,
                min_segment_duration_s: 30,
                heartbeat_every_s: 10,
                autostart: false,
                close_behavior: CloseBehavior::Ask,
                events_retention_days: 365,
            }
        );
        for f in FIELDS {
            assert_eq!(field(f.key).map(|x| x.key), Some(f.key), "`{}` 查不到自己", f.key);
        }
        assert!(matches!(
            field("autostart").unwrap().kind,
            FieldKind::Bool { .. }
        ));
        assert!(matches!(
            field("close_behavior").unwrap().kind,
            FieldKind::Enum { .. }
        ));
    }

    /// 表里那个 `values` 既是合法取值也是报错文案，必须与枚举一致 ——
    /// 否则会出现「表说合法、`from_str` 不认」的键。
    #[test]
    fn close_behavior_values_match_the_enum() {
        for v in CLOSE_BEHAVIORS {
            assert!(
                CloseBehavior::from_str(v).is_some(),
                "`{v}` 登记在表里，但 from_str 不认它"
            );
        }
        for cb in [CloseBehavior::Ask, CloseBehavior::Minimize, CloseBehavior::Quit] {
            assert!(
                CLOSE_BEHAVIORS.contains(&cb.as_str()),
                "枚举取值 `{}` 不在表里 —— 界面不给它选项、也写不回去",
                cb.as_str()
            );
        }
    }

    #[test]
    fn render_value_clamps_and_quotes_like_patch_line_expects() {
        assert_eq!(render_value("idle_threshold_s", "0").unwrap().0, "10", "下限 10");
        assert_eq!(render_value("events_retention_days", "-5").unwrap().0, "0");
        assert_eq!(render_value("autostart", "true").unwrap().0, "true");
        assert_eq!(
            render_value("close_behavior", "quit").unwrap().0,
            "\"quit\"",
            "枚举要带引号写回 TOML"
        );
        // 数字键把夹取后的值一并带出来（保留期靠它热生效）
        assert_eq!(
            render_value("events_retention_days", "40000").unwrap().1,
            Some(MAX_RETENTION_DAYS)
        );
        assert_eq!(render_value("autostart", "true").unwrap().1, None);
    }

    #[test]
    fn render_value_rejects_garbage_before_anything_is_written() {
        assert!(render_value("idle_threshold_s", "soon").is_err());
        assert!(render_value("autostart", "yes").is_err());
        assert!(render_value("close_behavior", "explode").is_err());
        assert!(render_value("no_such_key", "1").is_err(), "未知键必须拒绝");
    }

    // --- 数据目录：exe 旁边，一条规则（2026-10-17）---

    #[test]
    fn data_dir_sits_next_to_the_executable() {
        // 一条规则：程序在哪，账本就在哪。不分安装版/便携版。
        let dir = resolve_data_dir(
            None,
            None,
            Some(std::path::Path::new(r"D:\tools\时迹\time-scope.exe")),
        );
        assert_eq!(dir, std::path::PathBuf::from(r"D:\tools\时迹\time-scope-data"));
    }

    #[test]
    fn the_data_dir_name_has_a_prefix_so_it_is_never_ambiguous() {
        // 用户要求：别叫 `data` —— 目录里可能已经有一个 `data`；
        // 而且看见 `time-scope-data` 就知道是谁生成的。
        assert_eq!(DATA_DIR_NAME, "time-scope-data");
        assert!(!DATA_DIR_NAME.is_empty());
    }

    #[test]
    fn time_scope_data_overrides_the_exe_directory() {
        let dir = resolve_data_dir(
            Some(std::ffi::OsString::from(r"D:\ledgers\ts")),
            None,
            Some(std::path::Path::new(r"C:\Program Files\时迹\time-scope.exe")),
        );
        assert_eq!(
            dir,
            std::path::PathBuf::from(r"D:\ledgers\ts"),
            "显式覆盖必须优先 —— 这是给「放别的盘」「开发时别碰真实账本」用的"
        );
    }

    #[test]
    fn an_empty_override_is_ignored() {
        // 环境变量存在但为空（`$env:TIME_SCOPE_DATA = ""`）不该把目录解析成空路径
        let dir = resolve_data_dir(
            Some(std::ffi::OsString::from("")),
            None,
            Some(std::path::Path::new(r"D:\tools\时迹\time-scope.exe")),
        );
        assert_eq!(dir, std::path::PathBuf::from(r"D:\tools\时迹\time-scope-data"));
    }

    #[test]
    fn missing_exe_dir_is_a_loud_failure() {
        // 解析不出 exe 目录时**不静默落到当前工作目录** —— 那会让账本出现在
        // 随机位置（比如从快捷方式启动时的 system32）。宁可在这里炸。
        let r = std::panic::catch_unwind(|| resolve_data_dir(None, None, None));
        assert!(r.is_err(), "没有 exe 目录时必须报错，不能悄悄换地方");
    }

    #[test]
    fn a_macos_app_bundle_puts_the_ledger_next_to_the_bundle() {
        // exe 在 Foo.app/Contents/MacOS/ 里。往 bundle 内部写会破坏代码签名、
        // 用户看不见，更新还会整个替换掉 —— 所以锚是那个 .app 文件夹。
        let dir = resolve_data_dir(
            None,
            None,
            Some(std::path::Path::new(
                "/Users/me/Applications/时迹.app/Contents/MacOS/time-scope",
            )),
        );
        assert_eq!(
            dir,
            std::path::PathBuf::from("/Users/me/Applications/time-scope-data"),
            "数据要在 .app 旁边，不是在它里面"
        );
    }

    #[test]
    fn a_linux_appimage_puts_the_ledger_next_to_the_image_file() {
        // AppImage 的 exe 在 /tmp/.mount_xxx/ 里（只读且临时），
        // $APPIMAGE 才指向用户手里那个可写的文件。
        let dir = resolve_data_dir(
            None,
            Some(std::ffi::OsString::from("/home/me/apps/时迹.AppImage")),
            Some(std::path::Path::new("/tmp/.mount_abc123/usr/bin/time-scope")),
        );
        assert_eq!(
            dir,
            std::path::PathBuf::from("/home/me/apps/time-scope-data"),
            "数据要跟在那个 .AppImage 文件旁边，不能写进临时挂载点"
        );
    }

    #[test]
    fn an_empty_appimage_is_ignored() {
        // 环境变量存在但为空时不该把数据放到根目录
        let dir = resolve_data_dir(
            None,
            Some(std::ffi::OsString::from("")),
            Some(std::path::Path::new("/opt/time-scope/time-scope")),
        );
        assert_eq!(
            dir,
            std::path::PathBuf::from("/opt/time-scope/time-scope-data")
        );
    }

    #[test]
    fn the_override_beats_both_container_rules() {
        let dir = resolve_data_dir(
            Some(std::ffi::OsString::from("/data/ledger")),
            Some(std::ffi::OsString::from("/home/me/apps/时迹.AppImage")),
            Some(std::path::Path::new(
                "/Users/me/Applications/时迹.app/Contents/MacOS/time-scope",
            )),
        );
        assert_eq!(dir, std::path::PathBuf::from("/data/ledger"));
    }

    // --- 「改完要不要重启」的诚实性（2026-10-15）---
    //
    // 界面标「已保存，立刻生效」的字段**必须真的会热生效**。2026-10-15 之前
    // 这里把 `autostart` / `close_behavior` 都算成热生效，但 `Residency` 在
    // `install()` 时快照了这两个值，`set_config` 只写文件、不回写快照 ——
    // 界面说「已保存」而行为要等下次启动。界面说谎比多要求一次重启更糟。

    #[test]
    fn close_behavior_claims_to_be_live_because_the_close_handler_re_reads_the_file() {
        assert!(
            !needs_restart("close_behavior"),
            "关窗拦截现读 config.toml（residency::effective_close_behavior），真热生效"
        );
    }

    #[test]
    fn autostart_does_not_claim_to_be_live() {
        assert!(
            needs_restart("autostart"),
            "Residency 是启动快照，托盘勾选态与系统注册表 set_config 都碰不到 —— 必须说要重启"
        );
    }

    #[test]
    fn the_genuinely_live_fields_stay_live() {
        assert!(!needs_restart("events_retention_days"));
    }

    #[test]
    fn engine_and_collector_params_still_need_restart() {
        for key in [
            "idle_threshold_s",
            "grace_period_s",
            "min_segment_duration_s",
            "heartbeat_every_s",
        ] {
            assert!(needs_restart(key), "{key} 绑在构造注入上");
        }
    }

    #[test]
    fn an_unknown_key_is_treated_as_needing_restart() {
        assert!(
            needs_restart("some_key_we_never_saw"),
            "没登记过的键宁可多提醒一次重启，也别让它悄悄标成已生效"
        );
    }

    /// 界面读的那张表就是这张，不许再抄一份。
    #[test]
    fn the_verdict_map_covers_every_key() {
        let map = needs_restart_map();
        assert_eq!(map.len(), FIELDS.len());
        assert_eq!(map.get("close_behavior"), Some(&false));
        assert_eq!(map.get("events_retention_days"), Some(&false));
        assert_eq!(map.get("autostart"), Some(&true));
    }
}
