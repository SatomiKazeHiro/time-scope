use crate::Category;
use serde::{Deserialize, Serialize};

/// 用户把 rules.toml 改坏了时的错误。**app 层据此退回内置默认规则**（Review Focus #5）。
#[derive(Debug)]
pub enum RuleError {
    Parse(toml::de::Error),
    BadCategory { rule_id: String, value: String },
    BadConfidence { rule_id: String, value: f32 },
}

impl std::fmt::Display for RuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuleError::Parse(e) => write!(f, "rules.toml 语法错误: {e}"),
            RuleError::BadCategory { rule_id, value } => {
                write!(f, "规则 {rule_id} 的 category={value:?} 不是合法类别")
            }
            RuleError::BadConfidence { rule_id, value } => {
                write!(f, "规则 {rule_id} 的 confidence={value} 不在 0.0~1.0")
            }
        }
    }
}

impl std::error::Error for RuleError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawRule {
    pub id: String,
    #[serde(default)]
    pub process: Vec<String>,
    pub category: String,
    pub confidence: f32,
}

/// TOML 文档的根永远是一张 table，`[[rule]]` 在其中对应一个 `rule` 数组。
/// 直接 `toml::from_str::<Vec<RawRule>>` 会报 "invalid type: map, expected a sequence"。
#[derive(Debug, Deserialize)]
struct RulesDoc {
    #[serde(default, rename = "rule")]
    rule: Vec<RawRule>,
    /// 脱敏模式（spec §11）。**保持未编译的字符串**：engine 不该为隐私关注点
    /// 多一个 regex 依赖，编译与否由 app 层决定。
    #[serde(default, rename = "redact")]
    redact: Vec<RawRedact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawRedact {
    pub pattern: String,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub id: String,
    /// 已全部转成小写、且只保留文件名部分，用于匹配
    process_lower: Vec<String>,
    pub category: Category,
    pub confidence: f32,
}

impl Rule {
    fn matches(&self, process_lower: &str) -> bool {
        self.process_lower.iter().any(|p| p == process_lower)
    }
}

#[derive(Debug, Clone)]
pub struct RuleSet {
    pub rules: Vec<Rule>,
    /// 脱敏正则的**源码**，未编译（spec §11）
    pub redact: Vec<String>,
    /// 规则集版本号，写进每条 Activity 的 `classifier_version`（spec §7.2）
    pub version: String,
}

/// 规则集的版本号（spec §7.2），由**内容**派生（B2）。
///
/// 保留 `rules:N` / `redact:M` 两个计数前缀：人看版本串时一眼知道规模，
/// 而真正区分"内容变没变"的是后面的内容哈希。
///
/// **哈希必须是确定性的** —— 这个字符串会**落库**
/// （`activities.classifier_version`），同一份 rules.toml 在任何时候、
/// 任何机器上都必须算出同一个值。所以不能用 `HashMap` 的迭代顺序、
/// 也不能用 `DefaultHasher::new()`（SipHash 的实现细节不承诺跨版本稳定）。
/// 这里用 FNV-1a：几十行以内、跨版本跨平台稳定、engine 零依赖。
fn version_of(rules: &[Rule], redact: &[String]) -> String {
    let mut h = Fnv1a::new();
    h.write(rules.len().to_string().as_bytes());
    for r in rules {
        h.write(r.id.as_bytes());
        h.write(b"\x1f");
        h.write(r.category.as_str().as_bytes());
        h.write(b"\x1f");
        // confidence 用 bits 而不是 Display：`-1.0` 与 `1.0` 的字符串不同
        // 但它们本来就不是合法配置，位模式能避开浮点格式化的各种花样。
        h.write(&r.confidence.to_bits().to_le_bytes());
        // 进程列表也参与哈希：改了它就改了匹配范围。
        // 列表内部顺序不影响匹配结果（`matches` 是 any），但排序要稳定，
        // 否则同一份文件因书写顺序不同会得到两个版本号。
        let mut procs = r.process_lower.clone();
        procs.sort();
        for p in &procs {
            h.write(p.as_bytes());
            h.write(b"\x1e");
        }
        h.write(b"\x1d");
    }
    h.write(redact.len().to_string().as_bytes());
    for p in redact {
        h.write(p.as_bytes());
        h.write(b"\x1e");
    }
    format!("rules:{}+redact:{}+fnv1a:{:016x}", rules.len(), redact.len(), h.finish())
}

/// FNV-1a 64 位。为「给规则内容算一个稳定指纹」而写，不做通用哈希用。
struct Fnv1a(u64);

impl Fnv1a {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= *b as u64;
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    fn finish(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Classification {
    pub category: Category,
    pub confidence: f32,
    pub rule_id: Option<String>,
}

impl Classification {
    pub fn unknown() -> Self {
        Self {
            category: Category::Unknown,
            confidence: 0.0,
            rule_id: None,
        }
    }
}

impl RuleSet {
    /// **纯函数**：只解析传入的字符串，不碰文件系统。IO 在 app 层。
    pub fn from_toml(src: &str) -> Result<RuleSet, RuleError> {
        let doc: RulesDoc = toml::from_str(src).map_err(RuleError::Parse)?;
        let raw = doc.rule;
        let redact: Vec<String> = doc.redact.into_iter().map(|r| r.pattern).collect();

        let mut rules = Vec::with_capacity(raw.len());
        for r in raw {
            let category = Category::from_str(&r.category).ok_or_else(|| {
                RuleError::BadCategory {
                    rule_id: r.id.clone(),
                    value: r.category.clone(),
                }
            })?;
            if !(0.0..=1.0).contains(&r.confidence) {
                return Err(RuleError::BadConfidence {
                    rule_id: r.id,
                    value: r.confidence,
                });
            }
            rules.push(Rule {
                process_lower: r
                    .process
                    .iter()
                    .map(|p| file_name_lower(p))
                    .filter(|p| !p.is_empty())
                    .collect(),
                id: r.id,
                category,
                confidence: r.confidence,
            });
        }

        // 版本号由规则**内容**派生（B2）：改了规则（含脱敏规则），重算出来的
        // Activity 才能被区分开。原来只由条数派生，把 category 从 work 改成
        // study、或者改一条正则的文本，条数不变、版本号一模一样 ——
        // 注释里那句话当时是不成立的。
        let version = version_of(&rules, &redact);
        Ok(RuleSet { rules, redact, version })
    }

    /// 分类。进程名匹配**大小写不敏感**且只看文件名部分。
    pub fn classify(&self, process_name: Option<&str>) -> Classification {
        let Some(name) = process_name else {
            return Classification::unknown();
        };
        let key = file_name_lower(name);
        if key.is_empty() {
            return Classification::unknown();
        }
        for rule in &self.rules {
            if rule.matches(&key) {
                return Classification {
                    category: rule.category,
                    confidence: rule.confidence,
                    rule_id: Some(rule.id.clone()),
                };
            }
        }
        Classification::unknown()
    }
}

fn file_name_lower(p: &str) -> String {
    p.trim()
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

/// 内置默认规则。首次启动时由 app 层拷到 `%APPDATA%/time-scope/rules.toml`。
/// **只作为兜底**——用户改了之后以用户的文件为准（spec §7.2）。
pub const DEFAULT_RULES_TOML: &str = r#"
# Time Scope 分类规则
#
# process    只写文件名，大小写不敏感，路径会被自动忽略
# category   work / study / entertainment / communication / browsing / life / idle / unknown
# confidence 0.0 ~ 1.0
#
# 没命中任何规则的程序会落到 unknown，置信度 0.0 —— 这是预期行为。
# 想让它有颜色，往这里加一条就行。
#
# ---------------------------------------------------------------------------
# 窗口标题脱敏（spec §11）
#
# 命中的部分会在**入库前**被替换成 [redacted]。例如：
#
#   [[redact]]
#   pattern = '客户\d+'
#   [[redact]]
#   pattern = '(?i)salary'
#
# 注意：
#   * 写正则**必须用单引号**。TOML 的双引号会处理转义，"\d" 会被当成非法转义而报错。
#   * 默认**不预置任何脱敏规则**。过度脱敏会把有用的数据也毁掉，所以宁可你自己按需添加。
#   * 正则写错只跳过这一条并在终端提示，不影响你的分类规则。
#   * 脱敏只作用于新采集的数据；已落库的历史数据不会被回溯改写。
#     但界面的两条读路径（段详情、汇总页标题 Top）会在返回前各自再脱敏一次，
#     所以旧标题也不会以明文露出来。
# ---------------------------------------------------------------------------

[[rule]]
id = "vscode"
process = ["Code.exe"]
category = "work"
confidence = 0.9

[[rule]]
id = "jetbrains"
process = ["idea64.exe", "pycharm64.exe", "goland64.exe", "rider64.exe", "webstorm64.exe"]
category = "work"
confidence = 0.9

[[rule]]
id = "terminal"
process = ["WindowsTerminal.exe", "cmd.exe", "powershell.exe", "pwsh.exe", "alacritty.exe"]
category = "work"
confidence = 0.7

[[rule]]
id = "git-gui"
process = ["git-gui.exe", "tortoisegit.exe", "GitHubDesktop.exe"]
category = "work"
confidence = 0.8

[[rule]]
id = "office"
process = ["WINWORD.EXE", "EXCEL.EXE", "POWERPNT.EXE", "OUTLOOK.EXE", "onenote.exe"]
category = "work"
confidence = 0.8

[[rule]]
id = "browser"
process = ["msedge.exe", "chrome.exe", "firefox.exe", "brave.exe", "vivaldi.exe"]
category = "browsing"
confidence = 0.5

[[rule]]
id = "wechat"
process = ["WeChat.exe", "Weixin.exe"]
category = "communication"
confidence = 0.85

[[rule]]
id = "qq"
process = ["QQ.exe"]
category = "communication"
confidence = 0.85

[[rule]]
id = "chat"
process = ["slack.exe", "Teams.exe", "Discord.exe", "Telegram.exe"]
category = "communication"
confidence = 0.8

[[rule]]
id = "meet"
process = ["Zoom.exe", "Teams.exe", "TencentMeeting.exe"]
category = "communication"
confidence = 0.8

[[rule]]
id = "steam"
process = ["steam.exe", "steamwebhelper.exe"]
category = "entertainment"
confidence = 0.8

[[rule]]
id = "music"
process = ["Spotify.exe", "QQMusic.exe", "NetEaseMusic.exe"]
category = "entertainment"
confidence = 0.8

[[rule]]
id = "video"
process = ["vlc.exe", "PotPlayerMini64.exe", "PotPlayerMini.exe", "mpv.exe"]
category = "entertainment"
confidence = 0.75

[[rule]]
id = "netdisk"
process = ["BaiduNetdisk.exe", "Nutstore.exe"]
category = "life"
confidence = 0.6

[[rule]]
id = "todo"
process = ["Todoist.exe", "Things3.exe"]
category = "life"
confidence = 0.6
"#;
