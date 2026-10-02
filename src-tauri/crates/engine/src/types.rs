use serde::{Deserialize, Serialize};

/// 规则分类器的名字。spec §7.2：每次分类都记录 classifier 与规则集版本。
pub const CLASSIFIER_RULE: &str = "rule";

/// spec §6 的分类枚举。
///
/// Phase 1 靠进程名规则匹配，**没有** domain/URL 也没有语义模型，
/// 所以 `Browsing` 只能粗分，`Study`/`Life` 基本靠用户手写规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Work,
    Study,
    Entertainment,
    Communication,
    Browsing,
    Life,
    Idle,
    Unknown,
}

impl Category {
    pub fn all() -> Vec<Category> {
        vec![
            Category::Work,
            Category::Study,
            Category::Entertainment,
            Category::Communication,
            Category::Browsing,
            Category::Life,
            Category::Idle,
            Category::Unknown,
        ]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Category::Work => "work",
            Category::Study => "study",
            Category::Entertainment => "entertainment",
            Category::Communication => "communication",
            Category::Browsing => "browsing",
            Category::Life => "life",
            Category::Idle => "idle",
            Category::Unknown => "unknown",
        }
    }

    /// 解析用户手写 rules.toml 里的 category。**大小写不敏感**：
    /// 用户写 `Work` / `WORK` / `work` 都应该认。
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Category> {
        let lower = s.trim().to_ascii_lowercase();
        Category::all().into_iter().find(|c| c.as_str() == lower)
    }
}

/// 一个已完成的（或正在生长的）活动时间段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivitySegment {
    pub id: String,
    pub start_at: i64,
    pub end_at: i64,
    pub category: Category,
    pub application: Option<String>,
    /// 0.0 ~ 1.0。规则命中即用规则给的置信度；未命中为 0.0。
    pub confidence: f32,
    pub classifier: String,
    pub classifier_version: String,
    /// 支撑这个结论的 Event id 列表（spec §6 evidence）。
    #[serde(default)]
    pub evidence_event_ids: Vec<String>,
}

impl ActivitySegment {
    pub fn duration_ms(&self) -> i64 {
        (self.end_at - self.start_at).max(0)
    }

    pub fn is_empty(&self) -> bool {
        self.end_at <= self.start_at
    }
}

/// 临时上下文。**不落库**（spec §7.1），只活在 EngineState 里。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ActivityContext {
    pub application: Option<String>,
    pub window_title: Option<String>,
    pub input_active: bool,
    pub is_idle: bool,
}

impl ActivityContext {
    /// 判定"是否同一 context"，即该不该延长当前段而不是切段。
    ///
    /// **标题变化不算 context 变化**——浏览器每秒都在改标题，按标题切段会碎成渣。
    /// spec §7.3 的状态机只说"app + category 相同就延长"，与此一致。
    pub fn same_app_and_category(&self, other: &ActivityContext) -> bool {
        self.application == other.application && self.is_idle == other.is_idle
    }
}

/// 正在生长、尚未关闭的段。
#[derive(Debug, Clone, PartialEq)]
pub struct OpenSegment {
    pub start_at: i64,
    pub end_at: i64,
    pub category: Category,
    pub application: Option<String>,
    pub confidence: f32,
    pub evidence_event_ids: Vec<String>,
    /// 关闭时用哪个时间点收尾。可能被后续 Event 撑大。
    pub last_evidence_at: i64,
}
