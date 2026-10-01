//! config.toml 的解析与默认模板（spec §5）。
//!
//! 设计原则（spec §5.2）：**任何问题都回退到默认值，绝不报错**。
//! 只有一个文件不存在时才写默认模板；文件坏了就只读不写，留给用户自己修。
//!
//! 逐字段读取（而不是整份 deserialize 成一个 struct）是有意的：
//! 整份反序列化时，一个字段类型错会让整份文件失效；spec 要求
//! 「单个字段值非法 → 该字段用默认值，其余照常生效」。

use activity_engine::EngineConfig;
use std::path::{Path, PathBuf};

pub const MIN_IDLE_THRESHOLD_S: u32 = 10;
pub const MIN_HEARTBEAT_S: u32 = 1;

/// 上限：一天。超过它的配置值没有意义，夹住可以防止 u32 的极值把
/// 时间线切成几十万段。
const MAX_SECONDS: u32 = 86_400;
const MAX_HEARTBEAT_S: u32 = 600;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppConfig {
    /// 无输入多少秒算空闲
    pub idle_threshold_s: u32,
    /// 短暂上下文切换的宽限窗口
    pub grace_period_s: u32,
    /// 短于此的段并入相邻段
    pub min_segment_duration_s: u32,
    /// 心跳聚合窗口
    pub heartbeat_every_s: u32,
    /// 开机自启
    pub autostart: bool,
    /// 点 ✕ 时的行为
    pub close_behavior: CloseBehavior,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            idle_threshold_s: 300,
            grace_period_s: 60,
            min_segment_duration_s: 30,
            heartbeat_every_s: 10,
            autostart: false,
            close_behavior: CloseBehavior::Ask,
        }
    }
}

impl AppConfig {
    pub fn to_engine_config(&self) -> EngineConfig {
        EngineConfig::default()
            .with_idle_threshold_s(self.idle_threshold_s as u64)
            .with_grace_period_s(self.grace_period_s as u64)
            .with_min_segment_duration_s(self.min_segment_duration_s as u64)
    }

    /// 序列化成 TOML。写回 `close_behavior` / `autostart` 时用这个。
    pub fn to_toml(&self) -> String {
        format!(
            "idle_threshold_s = {}\n\
             grace_period_s = {}\n\
             min_segment_duration_s = {}\n\
             heartbeat_every_s = {}\n\
             autostart = {}\n\
             close_behavior = \"{}\"\n",
            self.idle_threshold_s,
            self.grace_period_s,
            self.min_segment_duration_s,
            self.heartbeat_every_s,
            self.autostart,
            self.close_behavior.as_str(),
        )
    }
}

fn clamp_u32(v: i64, min: u32, max: u32) -> u32 {
    v.clamp(min as i64, max as i64) as u32
}

/// **纯函数**：任何输入都返回一个可用的配置。
///
/// 逐字段取值：类型对不上、越界、缺失都只影响那一个字段。
pub fn parse(src: &str) -> AppConfig {
    let d = AppConfig::default();
    let Ok(table) = src.parse::<toml::Table>() else {
        eprintln!("[time-scope] config.toml 解析失败，本次使用默认配置（文件未被修改）");
        return d;
    };

    // 整数字段：非整数（字符串/浮点/数组）就当没写
    let int = |key: &str| -> Option<i64> { table.get(key).and_then(|v| v.as_integer()) };
    let boolean = |key: &str| -> Option<bool> { table.get(key).and_then(|v| v.as_bool()) };
    let text = |key: &str| -> Option<&str> { table.get(key).and_then(|v| v.as_str()) };

    AppConfig {
        // 下限 10：idle_threshold = 0 会让每条事件都切段，一天几万段
        idle_threshold_s: int("idle_threshold_s")
            .map(|v| clamp_u32(v, MIN_IDLE_THRESHOLD_S, MAX_SECONDS))
            .unwrap_or(d.idle_threshold_s),
        // 0 对这两个参数是有意义的（关掉该行为），所以下限就是 0
        grace_period_s: int("grace_period_s")
            .map(|v| clamp_u32(v, 0, MAX_SECONDS))
            .unwrap_or(d.grace_period_s),
        min_segment_duration_s: int("min_segment_duration_s")
            .map(|v| clamp_u32(v, 0, MAX_SECONDS))
            .unwrap_or(d.min_segment_duration_s),
        heartbeat_every_s: int("heartbeat_every_s")
            .map(|v| clamp_u32(v, MIN_HEARTBEAT_S, MAX_HEARTBEAT_S))
            .unwrap_or(d.heartbeat_every_s),
        autostart: boolean("autostart").unwrap_or(d.autostart),
        close_behavior: text("close_behavior")
            .and_then(CloseBehavior::from_str)
            .unwrap_or(d.close_behavior),
    }
}

pub fn app_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA").expect("APPDATA env var");
    PathBuf::from(appdata).join("time-scope")
}

pub fn config_path() -> PathBuf {
    app_dir().join("config.toml")
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

/// 把配置写回磁盘。**调用方要容忍失败**（退出路径上尤其重要）。
pub fn save(path: &Path, cfg: &AppConfig) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, cfg.to_toml())
}

pub const DEFAULT_CONFIG_TOML: &str = r#"# Time Scope 行为参数
#
# 改完重启生效（暂不支持热重载）。
# 任何一项写错或缺失都会回退到默认值，不会导致应用起不来。

# 无输入多少秒判定为空闲。最小 10。
idle_threshold_s = 300

# 短暂上下文切换的宽限窗口。0 = 关掉抖动吸收。
grace_period_s = 60

# 短于这个时长的段会并入相邻段。0 = 关掉合并。
min_segment_duration_s = 30

# 输入心跳的聚合窗口（秒）。
heartbeat_every_s = 10

# 开机自启。默认关——替用户决定要不要自动启动并记录窗口标题不合适。
autostart = false

# 点窗口的 ✕ 时：ask = 问一次 / minimize = 最小化到托盘 / quit = 退出
close_behavior = "ask"
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_spec() {
        let c = AppConfig::default();
        assert_eq!(c.idle_threshold_s, 300);
        assert_eq!(c.grace_period_s, 60);
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
grace_period_s = 0
min_segment_duration_s = 5
heartbeat_every_s = 30
autostart = true
close_behavior = "minimize"
"#,
        );
        assert_eq!(c.idle_threshold_s, 120);
        assert_eq!(c.grace_period_s, 0);
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
        assert_eq!(c.grace_period_s, 60, "没写的字段用默认");
    }

    #[test]
    fn a_wrongly_typed_field_falls_back_without_taking_the_file_down() {
        // 一个字段类型错，不该让其余字段也失效
        let c = parse("idle_threshold_s = \"soon\"\ngrace_period_s = 15\n");
        assert_eq!(c.idle_threshold_s, 300, "类型错的字段回退默认");
        assert_eq!(c.grace_period_s, 15, "其余字段照常生效");
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
        // grace=0 / min_segment=0 有明确含义（关掉该行为），不该被夹走
        let c = parse("grace_period_s = 0\nmin_segment_duration_s = 0\n");
        assert_eq!(c.grace_period_s, 0);
        assert_eq!(c.min_segment_duration_s, 0);
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
    fn serialising_round_trips() {
        let c = AppConfig {
            autostart: true,
            ..AppConfig::default()
        };
        let text = c.to_toml();
        let back = parse(&text);
        assert_eq!(back.autostart, true);
        assert_eq!(back.idle_threshold_s, c.idle_threshold_s);
    }

    #[test]
    fn engine_config_carries_the_three_segment_params() {
        let c = AppConfig {
            idle_threshold_s: 111,
            grace_period_s: 22,
            min_segment_duration_s: 33,
            ..AppConfig::default()
        };
        let e = c.to_engine_config();
        assert_eq!(e.idle_threshold_s, 111);
        assert_eq!(e.grace_period_s, 22);
        assert_eq!(e.min_segment_duration_s, 33);
    }

    #[test]
    fn default_toml_parses_to_defaults() {
        // 自检：内置模板必须真的能被自己解析
        assert_eq!(parse(DEFAULT_CONFIG_TOML), AppConfig::default());
    }

    #[test]
    fn a_broken_file_is_never_overwritten() {
        // spec §5.2 / §7：文件坏了只读不写，那是用户自己的内容
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
}
