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
    /// 规则集版本号，写进每条 Activity 的 `classifier_version`（spec §7.2）
    pub version: String,
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

        // 版本号由规则内容派生：改了规则，重算出来的 Activity 就能被区分开
        let version = format!("rules:{}", rules.len());
        Ok(RuleSet { rules, version })
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
