# Time Scope Phase 1 — 引擎与分类时间线 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 建 `activity-engine` 纯库（Context Builder + TOML 规则分类器 + Segmenter 状态机），把已采集的 Event 归并成带语义的 Activity Segment，落库并让前端时间线按 category 着色、支持粒度切换与当日汇总。

**Architecture:** 新增 `activity-engine` crate，**零 IO、零 Tauri 依赖、可独立 `cargo test`**。它提供纯函数 `reduce(state, event) -> EngineOutput`，逐个吞 Event、吐出已关闭的 Segment。App 层负责加载 `rules.toml`、驱动引擎、把关闭的段写进已有的 `activities` / `activity_evidence` 表。IPC 从 `get_events` 扩到 `get_segments` + `segment-updated` 推送。前端从"按事件类型着色"升级为"按 category 着色 + 10/30/60/120 分桶 + 当日汇总"。

**Tech Stack:** Rust 1.96（stable, x86_64-pc-windows-msvc）、`toml` crate（规则/配置解析）、rusqlite 0.32、serde、Tauri 2.12、React 19 + Vite 6 + TypeScript、vitest。

**Spec:** `docs/superpowers/specs/2026-10-01-time-scope-phase1-design.md` —— **实施者必须先读 §3（架构）、§6（数据模型）、§7（引擎，本计划的核心）、§8（Schema）、§9（IPC）、§10（前端）、§13（测试策略）**。本 plan 与 spec 冲突时以 spec 为准。

**前置状态:** Phase 1 第一步（骨架）已完成并合入本分支。Event 已能采集、落库、按天查回。详见 `docs/superpowers/STATUS.md`。

---

## Global Constraints

（spec 逐字提取的全局约束，每个 task 的要求都隐含包含本节）

- **平台**：仅 Windows。
- **包管理器**：前端 **pnpm**。禁止 npm/yarn 生成 lockfile。
- **Rust 版**：stable `x86_64-pc-windows-msvc`（本机 1.96.1 已验证）。
- **engine crate 的硬边界**：**零 IO、零 Tauri、零 SQLite**。它只接收 `Event`、吐出 `ActivitySegment`。规则文件与配置由 app 层读好再传进来。这是 spec §4 的要求，也是本 crate 能独立测试的前提。
- **Event 不可变**：`reduce` 不得修改传入的 `Event`，也不得修改历史 Event。
- **`reduce` 无副作用**：相同 `(state, event)` 必须得到相同 `output`。这是 spec §7.4 可重放的基础。
- **规则外置**：`%APPDATA%/time-scope/rules.toml`，首次启动从内置默认拷贝，用户可改。匹配失败 → `category = unknown, confidence = 0.0`。**宁可 unknown，不要伪准确**（spec §3 原则 4）。
- **时间粒度不落库**：分桶是前端纯函数 `bucket.ts`（spec §8.2）。切换粒度只改前端参数，不重查后端。
- **当日汇总由前端从 `get_segments` 聚合**，后端不提供单独的 summary 接口（spec §9）。
- **schema 不新增表**：`activities` / `activity_evidence` 在骨架阶段已建好（SCHEMA_VERSION=1）。本计划**不碰 schema**，因此不写迁移。
- **TDD**：engine 的每个决策先写表驱动单测再实现；`cargo test --workspace` 必须带 `--workspace`。
- **注释/commit 用英文**，面向用户的 UI 文案用中文。
- **命名**：crate kebab-case（`activity-engine`），类型 PascalCase，函数/变量 snake_case。

## Review Focus

（spec 隐含、但没有哪个 task 的测试显式覆盖、最可能坑到真实用户的五类输入/情况，按可能性排序）

1. **跨零点的 Event 流**（24h 常驻必然发生）：`reduce` 逐个吃 Event，`end_at` 必须能被下一个 Event 的 `timestamp` 撑开；否则凌晨那段会显示成零长度或负长度。落到 Task 5（segment 时间不变量测试）。
2. **只有 1 条 Event 的一天 / 只有 1 次切换的一天**：当天只有一个 segment 时，grace period 逻辑不能因为"没有上一个"而 panic 或产生空 segment。落到 Task 4。
3. **进程名大小写与路径形态**（`Code.exe` vs `code.exe` vs `C:\...\Code.EXE`）：规则匹配若做大小写归一，用户改规则时的心智模型会不一致；若不做，`code.exe` 写进规则却不匹配会让用户以为规则坏了。spec 没明说——本计划裁定**大小写不敏感匹配**（见 Task 3 ruling），并用测试钉死。落到 Task 3。
4. **`min_segment_duration` 与"已落库的段无法回溯合并"的冲突**：spec §7.3 要求短段并入相邻段，但段一旦落库就无法再改。落到 Task 5 的 pending buffer 设计（延迟 30s 落库）。
5. **规则文件被用户改坏**（语法错误 / 删掉全部规则 / 磁盘写入中断导致半截 TOML）：应用不能崩，应该退回内置默认规则并在 stderr 说明。落到 Task 3。

---

## File Structure

```
time-scope/
└── src-tauri/
    ├── crates/
    │   └── engine/                    # 新增：纯库，零 IO
        ├── Cargo.toml
        ├── src/
        │   ├── lib.rs                 # reduce / EngineState / EngineOutput 入口
        │   ├── types.rs               # Category / ActivitySegment / OpenSegment / ActivityContext
        │   ├── context.rs             # Context Builder：从 Event 流维护上下文
        │   ├── classifier.rs          # RuleSet + classify()
        │   ├── segmenter.rs           # 状态机：切段 / grace / idle / pending 缓冲
        │   ├── config.rs              # EngineConfig（idle_threshold / grace / min_segment）
        │   └── tests/                 # 表驱动单测（纯函数，无 Win32）
    ├── src/
    │   ├── main.rs
    │   ├── lib.rs                     # 改：加 get_segments、规则加载、引擎驱动
    │   ├── rules.rs                   # 新增：内置默认规则 + %APPDATA% 首次拷贝
    │   └── engine_runtime.rs          # 新增：持有 EngineState，消费 events，驱动 reduce
└── src/
    ├── types.ts                       # 改：ActivitySegment / Category
    ├── lib/bucket.ts                  # 新增：分桶纯函数（spec §8.2）
    └── components/
        ├── Timeline.tsx               # 改：按 category 着色
        ├── GranularityPicker.tsx      # 新增
        ├── DaySummary.tsx             # 新增
        └── EventDetail.tsx            # 改：显示 category/置信度/evidence
```

**刻意不新建**：`config.rs` 独立成一个文件而不是塞进 `types.rs`——参数是"可调行为"，类型是"数据形状"，分开后 `config.rs` 可以独立演进而不碰数据模型。

---

## Task 1: engine crate 骨架与类型

**Files:**
- Create: `src-tauri/crates/engine/Cargo.toml`
- Create: `src-tauri/crates/engine/src/types.rs`
- Create: `src-tauri/crates/engine/src/lib.rs`
- Create: `src-tauri/crates/engine/src/tests/types.rs`
- Modify: `src-tauri/Cargo.toml`（workspace members 加 `crates/engine`）

**Interfaces:**
- Consumes: 无（起点）
- Produces:
  - `activity_engine::Category`（8 变体，serde snake_case，附 `as_str()` / `from_str()`）
  - `activity_engine::ActivitySegment { id, start_at, end_at, category, application: Option<String>, confidence: f32, classifier: String, classifier_version: String, evidence_event_ids: Vec<String> }`
  - `activity_engine::ActivityContext { application: Option<String>, window_title: Option<String>, input_active: bool, is_idle: bool }`，含 `same_app_and_category(&self, other) -> bool`（判定"同一 context"）
  - `activity_engine::OpenSegment`（正在生长、未关闭的段）

- [ ] **Step 1: 把 engine 加进 workspace members**

编辑 `src-tauri/Cargo.toml`：

```toml
[workspace]
members = ["crates/core", "crates/storage", "crates/collector", "crates/engine", "."]
resolver = "2"

[workspace.dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

- [ ] **Step 2: 创建 `src-tauri/crates/engine/Cargo.toml`**

```toml
[package]
name = "activity-engine"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
activity-core = { path = "../core" }
serde.workspace = true
serde_json.workspace = true

[dev-dependencies]
activity-core = { path = "../core" }
```

> **这个 Cargo.toml 里故意没有 `tauri`、`rusqlite`、`std::fs`。** 边界是 spec §4 的硬要求，
> Task 10 会有一个检查专门验它。

- [ ] **Step 3: 写失败测试 `src-tauri/crates/engine/src/tests/types.rs`**

```rust
use crate::{ActivityContext, ActivitySegment, Category, CLASSIFIER_RULE};

fn seg(category: Category, start: i64, end: i64) -> ActivitySegment {
    ActivitySegment {
        id: format!("s{start}"),
        start_at: start,
        end_at: end,
        category,
        application: Some("Code.exe".into()),
        confidence: 0.9,
        classifier: CLASSIFIER_RULE.into(),
        classifier_version: "test-v1".into(),
        evidence_event_ids: vec![],
    }
}

#[test]
fn category_roundtrips_through_str() {
    for c in Category::all() {
        assert_eq!(Category::from_str(c.as_str()), Some(c.clone()), "{c:?}");
    }
}

#[test]
fn category_as_str_is_snake_case() {
    assert_eq!(Category::Work.as_str(), "work");
    assert_eq!(Category::Entertainment.as_str(), "entertainment");
    assert_eq!(Category::Unknown.as_str(), "unknown");
    assert_eq!(Category::Idle.as_str(), "idle");
}

#[test]
fn category_from_str_rejects_unknown() {
    assert_eq!(Category::from_str("nope"), None);
    assert_eq!(Category::from_str(""), None);
    // 大小写不敏感：用户手写 rules.toml 时不该因为大小写就失配
    assert_eq!(Category::from_str("WORK"), Some(Category::Work));
    assert_eq!(Category::from_str("Work"), Some(Category::Work));
}

#[test]
fn category_has_all_eight_variants() {
    assert_eq!(Category::all().len(), 8);
}

#[test]
fn context_equality_uses_app_and_idle_only() {
    let a = ActivityContext {
        application: Some("Code.exe".into()),
        window_title: Some("main.rs".into()),
        input_active: true,
        is_idle: false,
    };
    let mut b = a.clone();
    b.window_title = Some("lib.rs".into()); // 标题变了
    assert!(a.same_app_and_category(&b), "标题变化不切段");

    b.application = Some("chrome.exe".into());
    assert!(!a.same_app_and_category(&b), "应用变化应切段");
}

#[test]
fn context_idle_flag_always_diffs() {
    let mut a = ActivityContext {
        application: Some("Code.exe".into()),
        window_title: None,
        input_active: true,
        is_idle: false,
    };
    let b = a.clone();
    a.is_idle = true;
    assert!(!a.same_app_and_category(&b), "idle 状态不同必然切段");
}

#[test]
fn segment_helpers() {
    let s = seg(Category::Work, 1000, 61_000);
    assert_eq!(s.duration_ms(), 60_000);
    assert!(!s.is_empty());
    let zero = seg(Category::Work, 1000, 1000);
    assert!(zero.is_empty());
    assert_eq!(zero.duration_ms(), 0);
}
```

- [ ] **Step 4: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p activity-engine`
Expected: ERROR（`Category` 等未定义）。

- [ ] **Step 5: 实现 `src-tauri/crates/engine/src/types.rs`**

```rust
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
    /// 关闭时用哪个时间点收尾。可能被后续 Event 撑大（idle 段靠 resume 撑）。
    pub last_evidence_at: i64,
}
```

- [ ] **Step 6: 创建 `src-tauri/crates/engine/src/lib.rs` 骨架**

```rust
pub mod types;

pub use types::{
    ActivityContext, ActivitySegment, Category, OpenSegment, CLASSIFIER_RULE,
};
```

- [ ] **Step 7: 创建 `src-tauri/crates/engine/src/tests/mod.rs`**

```rust
mod types;
```

并在 `lib.rs` 顶部加：

```rust
#[cfg(test)]
mod tests;
```

- [ ] **Step 8: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p activity-engine`
Expected: PASS（6 tests）。

- [ ] **Step 9: 提交**

```bash
git add src-tauri/Cargo.toml src-tauri/crates/engine/
git commit -m "feat(engine): add Category, ActivitySegment and ActivityContext types"
```

---

## Task 2: EngineConfig（可调参数）

**Files:**
- Create: `src-tauri/crates/engine/src/config.rs`
- Create: `src-tauri/crates/engine/src/tests/config.rs`
- Modify: `src-tauri/crates/engine/src/lib.rs`

**Interfaces:**
- Consumes: 无
- Produces: `activity_engine::EngineConfig { idle_threshold_s: u64, grace_period_s: u64, min_segment_duration_s: u64 }`，`Default` 为 spec §7.3 的 300 / 60 / 30；提供 `with_*` 构造器（builder），便于测试。

**为什么单独一个 task:** 参数是后面 segmenter 所有分支的输入，先固定住默认值和边界行为，后面 task 才能只测状态机不测参数。

- [ ] **Step 1: 写失败测试 `src-tauri/crates/engine/src/tests/config.rs`**

```rust
use crate::EngineConfig;

#[test]
fn defaults_match_spec() {
    let c = EngineConfig::default();
    assert_eq!(c.idle_threshold_s, 300);
    assert_eq!(c.grace_period_s, 60);
    assert_eq!(c.min_segment_duration_s, 30);
}

#[test]
fn builder_overrides_each_field() {
    let c = EngineConfig::default()
        .with_idle_threshold_s(10)
        .with_grace_period_s(0)
        .with_min_segment_duration_s(0);
    assert_eq!(c.idle_threshold_s, 10);
    assert_eq!(c.grace_period_s, 0);
    assert_eq!(c.min_segment_duration_s, 0);
}

#[test]
fn config_is_copy_and_comparable() {
    let a = EngineConfig::default();
    let b = a;
    assert_eq!(a, b);
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p activity-engine`
Expected: ERROR（`EngineConfig` 未定义）。

- [ ] **Step 3: 实现 `src-tauri/crates/engine/src/config.rs`**

```rust
/// 可调参数。默认值取自 spec §7.3 的表格。
///
/// 这些值由 app 层从配置文件读进来传给引擎；engine 自己不碰磁盘（spec §4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineConfig {
    /// 无输入多少秒判定为空闲
    pub idle_threshold_s: u64,
    /// 短暂切换的宽限窗口
    pub grace_period_s: u64,
    /// 短于此的段并入相邻段
    pub min_segment_duration_s: u64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            idle_threshold_s: 300,
            grace_period_s: 60,
            min_segment_duration_s: 30,
        }
    }
}

impl EngineConfig {
    pub fn with_idle_threshold_s(mut self, v: u64) -> Self {
        self.idle_threshold_s = v;
        self
    }

    pub fn with_grace_period_s(mut self, v: u64) -> Self {
        self.grace_period_s = v;
        self
    }

    pub fn with_min_segment_duration_s(mut self, v: u64) -> Self {
        self.min_segment_duration_s = v;
        self
    }
}
```

- [ ] **Step 4: 在 `lib.rs` 导出**

```rust
pub mod config;
pub mod types;

pub use config::EngineConfig;
pub use types::{ActivityContext, ActivitySegment, Category, OpenSegment, CLASSIFIER_RULE};
```

- [ ] **Step 5: 在 `tests/mod.rs` 加 `mod config;`**

- [ ] **Step 6: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p activity-engine`
Expected: PASS（9 tests）。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/crates/engine/
git commit -m "feat(engine): add EngineConfig with spec defaults"
```

---

## Task 3: Rule Classifier（TOML 规则集 + 匹配）

**Files:**
- Create: `src-tauri/crates/engine/src/classifier.rs`
- Create: `src-tauri/crates/engine/src/tests/classifier.rs`
- Modify: `src-tauri/crates/engine/Cargo.toml`（加 `toml`）
- Modify: `src-tauri/crates/engine/src/lib.rs`
- Modify: `src-tauri/crates/engine/src/tests/mod.rs`

**Interfaces:**
- Consumes: `Category`、`CLASSIFIER_RULE`
- Produces:
  - `activity_engine::Rule { id: String, process: Vec<String>, category: Category, confidence: f32 }`
  - `activity_engine::RuleSet { rules: Vec<Rule>, version: String }`
  - `RuleSet::from_toml(src: &str) -> Result<RuleSet, RuleError>`（**纯函数**：只吃字符串，不读文件）
  - `RuleSet::classify(&self, process_name: Option<&str>) -> Classification`
  - `activity_engine::Classification { category: Category, confidence: f32, rule_id: Option<String> }`
  - `activity_engine::DEFAULT_RULES_TOML: &str`（内置默认规则，spec §7.2 那三段 + 扩充）

**匹配规则（本计划的裁定，实现必须照此）:**
1. 进程名**大小写不敏感**匹配（`Code.exe` 规则能匹配 `code.exe`）—— Review Focus #3
2. 进程名只取**文件名部分**比较（`C:\x\y\Code.exe` 匹配 `Code.exe`）
3. 规则按声明顺序，**第一条命中即返回**
4. 都没命中 → `Classification { category: Unknown, confidence: 0.0, rule_id: None }`
5. `confidence` 直接用规则里写的值

- [ ] **Step 1: 加 `toml` 依赖**

在 `src-tauri/crates/engine/Cargo.toml` 的 `[dependencies]` 加：

```toml
toml = "0.8"
```

- [ ] **Step 2: 写失败测试 `src-tauri/crates/engine/src/tests/classifier.rs`**

```rust
use crate::{Classification, RuleSet};

const SAMPLE: &str = r#"
[[rule]]
id = "vscode"
process = ["Code.exe"]
category = "work"
confidence = 0.9

[[rule]]
id = "browser"
process = ["msedge.exe", "chrome.exe"]
category = "browsing"
confidence = 0.5

[[rule]]
id = "wechat"
process = ["WeChat.exe"]
category = "communication"
confidence = 0.85
"#;

#[test]
fn matches_first_rule_in_declaration_order() {
    let rs = RuleSet::from_toml(SAMPLE).unwrap();
    assert_eq!(rs.classify(Some("Code.exe")).rule_id.as_deref(), Some("vscode"));
    assert_eq!(rs.classify(Some("chrome.exe")).rule_id.as_deref(), Some("browser"));
    assert_eq!(rs.classify(Some("WeChat.exe")).rule_id.as_deref(), Some("wechat"));
}

#[test]
fn returns_rule_category_and_confidence() {
    let rs = RuleSet::from_toml(SAMPLE).unwrap();
    let c = rs.classify(Some("Code.exe"));
    assert_eq!(c.category.as_str(), "work");
    assert!((c.confidence - 0.9).abs() < 1e-6);
}

#[test]
fn process_match_is_case_insensitive() {
    // Review Focus #3：用户手写规则不该因为大小写失配
    let rs = RuleSet::from_toml(SAMPLE).unwrap();
    for name in ["code.exe", "CODE.EXE", "CoDe.ExE"] {
        assert_eq!(
            rs.classify(Some(name)).rule_id.as_deref(),
            Some("vscode"),
            "{name} 应匹配 vscode"
        );
    }
}

#[test]
fn matches_on_file_name_not_full_path() {
    let rs = RuleSet::from_toml(SAMPLE).unwrap();
    let c = rs.classify(Some("C:\\Program Files\\Microsoft VS Code\\Code.exe"));
    assert_eq!(c.rule_id.as_deref(), Some("vscode"));
}

#[test]
fn unknown_process_falls_back_to_unknown_with_zero_confidence() {
    let rs = RuleSet::from_toml(SAMPLE).unwrap();
    let c = rs.classify(Some("SomeRandomGame.exe"));
    assert_eq!(c.category.as_str(), "unknown");
    assert_eq!(c.confidence, 0.0);
    assert!(c.rule_id.is_none());
}

#[test]
fn missing_process_name_is_unknown() {
    let rs = RuleSet::from_toml(SAMPLE).unwrap();
    assert_eq!(rs.classify(None).category.as_str(), "unknown");
    assert_eq!(rs.classify(Some("")).category.as_str(), "unknown");
}

#[test]
fn empty_rule_set_classifies_everything_as_unknown() {
    let rs = RuleSet::from_toml("").unwrap();
    assert_eq!(rs.classify(Some("Code.exe")).category.as_str(), "unknown");
}

#[test]
fn category_is_case_insensitive_and_trimmed() {
    let rs = RuleSet::from_toml(
        r#"
[[rule]]
id = "x"
process = ["a.exe"]
category = "  Work  "
confidence = 0.5
"#,
    )
    .unwrap();
    assert_eq!(rs.classify(Some("a.exe")).category.as_str(), "work");
}

#[test]
fn invalid_category_string_is_an_error_not_a_silent_unknown() {
    // 规则写错类别必须报错，否则用户以为规则生效了其实全落到 unknown
    let err = RuleSet::from_toml(
        r#"
[[rule]]
id = "x"
process = ["a.exe"]
category = "not_a_category"
confidence = 0.5
"#,
    );
    assert!(err.is_err(), "非法 category 应报错");
}

#[test]
fn malformed_toml_is_an_error() {
    assert!(RuleSet::from_toml("[[[not toml").is_err());
}

#[test]
fn rules_version_is_recorded_for_reproducibility() {
    let rs = RuleSet::from_toml(SAMPLE).unwrap();
    assert!(!rs.version.is_empty(), "规则集必须有版本号，spec §7.2");
}

#[test]
fn rule_with_no_process_matches_nothing() {
    // 空 process 列表不该变成"匹配所有程序"
    let rs = RuleSet::from_toml(
        r#"
[[rule]]
id = "empty"
process = []
category = "work"
confidence = 0.9
"#,
    )
    .unwrap();
    assert_eq!(rs.classify(Some("anything.exe")).category.as_str(), "unknown");
}

#[test]
fn default_rules_toml_is_valid() {
    let rs = RuleSet::from_toml(crate::DEFAULT_RULES_TOML).expect("内置默认规则必须能解析");
    assert!(!rs.rules.is_empty());
    assert_eq!(rs.classify(Some("Code.exe")).category.as_str(), "work");
    assert_eq!(rs.classify(Some("msedge.exe")).category.as_str(), "browsing");
}

#[test]
fn classification_defaults_to_unknown() {
    let c = Classification::unknown();
    assert_eq!(c.category.as_str(), "unknown");
    assert_eq!(c.confidence, 0.0);
    assert!(c.rule_id.is_none());
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p activity-engine`
Expected: ERROR（`RuleSet` 未定义）。

- [ ] **Step 4: 实现 `src-tauri/crates/engine/src/classifier.rs`**

```rust
use crate::{Category, CLASSIFIER_RULE};
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

#[derive(Debug, Clone)]
pub struct Rule {
    pub id: String,
    /// 已全部转成小写，用于匹配
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
        let raw: Vec<RawRule> = toml::from_str(src).map_err(RuleError::Parse)?;

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
                    rule_id: r.id.clone(),
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
# process  只写文件名，大小写不敏感，路径会被自动忽略
# category work / study / entertainment / communication / browsing / life / idle / unknown
# confidence 0.0 ~ 1.0，决定时间线上这个类别的"可信程度"展示
#
# 没命中任何规则的程序会落到 unknown，置信度 0.0 —— 这是预期行为，
# 想让它有颜色，往这里加一条就行。

[[rule]]
id = "vscode"
process = ["Code.exe", "code.exe"]
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
id = "slack-teams-discord"
process = ["slack.exe", "Teams.exe", "Discord.exe"]
category = "communication"
confidence = 0.8

[[rule]]
id = "zoom-meet"
process = ["Zoom.exe", "Teams.exe"]
category = "communication"
confidence = 0.8

[[rule]]
id = "steam"
process = ["steam.exe", "steamwebhelper.exe"]
category = "entertainment"
confidence = 0.8

[[rule]]
id = "spotify"
process = ["Spotify.exe"]
category = "entertainment"
confidence = 0.8

[[rule]]
id = "video"
process = ["vlc.exe", "PotPlayerMini64.exe", "mpv.exe", "PotPlayerMini.exe"]
category = "entertainment"
confidence = 0.75

[[rule]]
id = "netdisk"
process = ["BaiduNetdisk.exe", "Pan.baidu.com.exe", "Nutstore.exe"]
category = "life"
confidence = 0.6

[[rule]]
id = "todo"
process = ["Todoist.exe", "Things3.exe"]
category = "life"
confidence = 0.6
"#;
```

- [ ] **Step 5: 在 `lib.rs` 导出，在 `tests/mod.rs` 加 `mod classifier;`**

`lib.rs`：

```rust
pub mod classifier;
pub mod config;
pub mod types;

pub use classifier::{Classification, Rule, RuleError, RuleSet, DEFAULT_RULES_TOML};
pub use config::EngineConfig;
pub use types::{ActivityContext, ActivitySegment, Category, OpenSegment, CLASSIFIER_RULE};
```

`tests/mod.rs`：

```rust
mod classifier;
mod config;
mod types;
```

- [ ] **Step 6: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p activity-engine`
Expected: PASS（22 tests）。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/crates/engine/
git commit -m "feat(engine): add TOML rule classifier with case-insensitive process matching"
```

---

## Task 4: Context Builder

**Files:**
- Create: `src-tauri/crates/engine/src/context.rs`
- Create: `src-tauri/crates/engine/src/tests/context.rs`
- Modify: `src-tauri/crates/engine/src/lib.rs`、`tests/mod.rs`

**Interfaces:**
- Consumes: `activity_core::{Event, EventType}`、`ActivityContext`
- Produces:
  - `activity_engine::ContextBuilder::apply(&self, ctx: &mut ActivityContext, event: &Event)`
  - `activity_engine::context_of(event: &Event, is_idle: bool, input_active: bool) -> ActivityContext`（给 `reduce` 用的一次性版本）

**规则:**
| Event | 效果 |
|---|---|
| `WindowFocus` / `WindowTitleChange` | 更新 `application`、`window_title` |
| `InputHeartbeat` | `input_active = active_seconds > 0` |
| `SystemIdle` | 由 `is_idle` 参数决定（`reduce` 负责翻转状态） |
| `SystemResume` | 同上 |
| `SessionLock` / `SessionUnlock` | 无 |

- [ ] **Step 1: 写失败测试 `src-tauri/crates/engine/src/tests/context.rs`**

```rust
use crate::{context_of, ContextBuilder};
use activity_core::{Event, EventType, InputHeartbeatPayload, WindowFocusPayload,
                    WindowTitleChangePayload};

fn focus(app: &str, title: &str) -> Event {
    Event::new(
        EventType::WindowFocus(WindowFocusPayload {
            process_name: app.into(),
            window_title: Some(title.into()),
            exe_path: None,
        }),
        1000,
    )
}

fn title(app: &str, title: &str) -> Event {
    Event::new(
        EventType::WindowTitleChange(WindowTitleChangePayload {
            process_name: app.into(),
            window_title: Some(title.into()),
        }),
        2000,
    )
}

fn heartbeat(secs: u8) -> Event {
    Event::new(
        EventType::InputHeartbeat(InputHeartbeatPayload { active_seconds: secs }),
        3000,
    )
}

#[test]
fn window_focus_sets_application_and_title() {
    let e = focus("Code.exe", "main.rs");
    let c = context_of(&e, false, true);
    assert_eq!(c.application.as_deref(), Some("Code.exe"));
    assert_eq!(c.window_title.as_deref(), Some("main.rs"));
    assert!(!c.is_idle);
    assert!(c.input_active);
}

#[test]
fn title_change_updates_application_and_title() {
    let mut c = context_of(&focus("Code.exe", "a.rs"), false, true);
    ContextBuilder::apply(&mut c, &title("chrome.exe", "New Tab"));
    assert_eq!(c.application.as_deref(), Some("chrome.exe"));
    assert_eq!(c.window_title.as_deref(), Some("New Tab"));
}

#[test]
fn null_title_does_not_erase_previous_title() {
    // 切到无标题窗口时不该把上一个窗口的标题带过来
    let mut c = context_of(&focus("Code.exe", "main.rs"), false, true);
    let e = Event::new(
        EventType::WindowTitleChange(WindowTitleChangePayload {
            process_name: "explorer.exe".into(),
            window_title: None,
        }),
        2000,
    );
    ContextBuilder::apply(&mut c, &e);
    assert_eq!(c.application.as_deref(), Some("explorer.exe"));
    assert_eq!(c.window_title, None);
}

#[test]
fn heartbeat_updates_input_active() {
    let mut c = context_of(&focus("Code.exe", "x"), false, false);
    ContextBuilder::apply(&mut c, &heartbeat(0));
    assert!(!c.input_active, "active_seconds=0 表示这窗口没输入");
    ContextBuilder::apply(&mut c, &heartbeat(3));
    assert!(c.input_active);
}

#[test]
fn idle_flag_comes_from_caller_not_from_event() {
    // SystemIdle/Resume 的状态翻转是 reduce 的职责，ContextBuilder 只接收结果
    let e = Event::new(EventType::SystemIdle, 5000);
    assert!(context_of(&e, true, false).is_idle);
    assert!(!context_of(&e, false, false).is_idle);
}

#[test]
fn session_events_leave_context_untouched() {
    let base = context_of(&focus("Code.exe", "x"), false, true);
    for ev in [
        Event::new(EventType::SessionLock, 9000),
        Event::new(EventType::SessionUnlock, 9000),
    ] {
        let c = context_of(&ev, false, true);
        assert_eq!(c, base, "{:?} 不应改动 context", ev.event_type);
    }
}

#[test]
fn builder_is_idempotent_for_the_same_event() {
    let mut a = context_of(&focus("Code.exe", "x"), false, true);
    let mut b = a.clone();
    let e = focus("Code.exe", "x");
    ContextBuilder::apply(&mut a, &e);
    ContextBuilder::apply(&mut b, &e);
    assert_eq!(a, b);
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p activity-engine`
Expected: ERROR（`ContextBuilder` 未定义）。

- [ ] **Step 3: 实现 `src-tauri/crates/engine/src/context.rs`**

```rust
use crate::ActivityContext;
use activity_core::{Event, EventType};

/// 把一个 Event 应用到上下文上。
///
/// 纯函数集合，**无状态**（spec §7.1：Context 是临时状态，不落库）。
pub struct ContextBuilder;

impl ContextBuilder {
    pub fn apply(ctx: &mut ActivityContext, event: &Event) {
        match &event.event_type {
            EventType::WindowFocus(p) | EventType::WindowTitleChange(p) => {
                ctx.application = Some(p.process_name.clone());
                ctx.window_title = p.window_title.clone();
            }
            EventType::InputHeartbeat(p) => {
                ctx.input_active = p.active_seconds > 0;
            }
            // idle 状态由 reduce 翻转后作为参数传进来；会话事件 Phase 1 不采集
            EventType::SystemIdle
            | EventType::SystemResume
            | EventType::SessionLock
            | EventType::SessionUnlock => {}
        }
    }
}

/// 由一个 Event 加上当前 idle/输入状态，算出"处理完这个 Event 之后"的完整上下文。
pub fn context_of(event: &Event, is_idle: bool, input_active: bool) -> ActivityContext {
    let mut ctx = ActivityContext {
        application: None,
        window_title: None,
        input_active,
        is_idle,
    };
    ContextBuilder::apply(&mut ctx, event);
    ctx
}
```

- [ ] **Step 4: 导出并注册测试模块**

`lib.rs` 加 `pub mod context;` 与 `pub use context::{context_of, ContextBuilder};`；
`tests/mod.rs` 加 `mod context;`。

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p activity-engine`
Expected: PASS（29 tests）。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/crates/engine/
git commit -m "feat(engine): add ContextBuilder deriving activity context from events"
```

---

## Task 5: Segmenter 状态机（reduce）

**Files:**
- Create: `src-tauri/crates/engine/src/segmenter.rs`
- Create: `src-tauri/crates/engine/src/tests/segmenter.rs`
- Modify: `src-tauri/crates/engine/src/lib.rs`、`tests/mod.rs`

**Interfaces:**
- Consumes: `Event`、`ActivityContext`、`ActivitySegment`、`OpenSegment`、`Classification`、`RuleSet`、`EngineConfig`
- Produces:
  - `activity_engine::EngineState { current_segment: Option<OpenSegment>, current_context: Option<ActivityContext>, is_idle: bool, pending: Vec<ActivitySegment>, last_switch_at: Option<i64> }`
  - `activity_engine::EngineOutput { state: EngineState, closed_segments: Vec<ActivitySegment> }`
  - `activity_engine::reduce(state: &EngineState, event: &Event, rules: &RuleSet, config: &EngineConfig) -> EngineOutput`
  - `EngineState::initial()`
  - `EngineState::open_segment_snapshot(&self, rules, config) -> Option<ActivitySegment>`（把正在生长的段投影成可展示的 Segment，供 `get_segments` 返回**当前**这一段）

**状态机（spec §7.3 的直接翻译 + 三处裁定）:**

> ⚠️ **本节已在实施后修订。** 初稿的 grace 分支写的是"不切段，只延长"，实测是错的
> （见下方裁定 C）。以下为最终实现。

```
reduce(state, event, rules, config):

  1. 翻转 idle 状态：SystemIdle → is_idle = true；SystemResume → is_idle = false
  2. new_ctx = **clone(current_context)** 置 is_idle，再 apply(event)
     （不能从空 context 起算，否则心跳事件会把 application 冲成 None）
  3. classification = rules.classify(new_ctx.application)
  4. 若是 idle 相关（SystemIdle / SystemResume）→ 走 idle 分支
     否则走 context 分支

  context 分支:
    - 没有当前段            → 开新段（category = Idle if is_idle else classification）
    - current 同 new_ctx    → 延长：end_at = max(end_at, event.ts), 追加 evidence
    - 不同：
        · 若满足吸收条件（裁定 C）→ **复活前一段**，丢弃当前段，延长复活的那段
        · 否则关闭旧段（进 pending），记 previous_application = 旧段 application，开新段

  idle 分支:
    - SystemIdle  → 关闭当前段，开一个 Category::Idle 的段
    - SystemResume→ 关闭 Idle 段，后续由 context 分支开新段

  5. 把 pending 里 end_at 距 event.ts 超过 max(min_segment_duration, grace_period)
     的段搬到 closed_segments
```

**裁定 A（min_segment_duration）:** 段不能"事后合并"——一旦落库就改不了。所以引擎内部维护
`pending` 缓冲：只有当某段已经结束且**距今超过落库门槛**才交给上层落库。

> ⚠️ **裁定 A 只做了一半，实施后补齐。** 它只把落库**往后推**，
> 并没有实现 spec §7.3 的"并入相邻段"。初稿写"Task 6 负责合并"，
> 但 Task 6 最后是纯 storage 层、什么也没合并。缺失的那一半是 **裁定 E**。

**裁定 B（open segment 不落库）:** 正在生长的段不进 `closed_segments`。`get_segments` 时用
`open_segment_snapshot` 单独取出来附在结果末尾，所以时间线是实时的。

**裁定 C（grace 吸收要"复活"前一段，不能只是"不切段"）** — *实施后新增，推翻了初稿设计。*

spec §7.3 原文是"若距上次切换 < GRACE_PERIOD 且新 context 与**再前一个**相同 → 视为短暂切换，
不切段"。初稿把它简化成一个 `returns_to_previous` 代理判定。**这是错的**：

```
Code -> chrome(2s) -> Code
"不切段"的结果是留下 chrome 段 —— 但用户实际全程在 Code
```

正确语义是"抖动"：那个一闪而过的 context 从未真正发生。所以吸收时要

1. 丢弃刚开的那段（一闪而过的错误 context）
2. 从 `pending` 里**弹出并复活**前一段，把它延长到当前时间

实现上用**独立的 `previous_application: Option<String>` 字段**记住"再前一个"，
而不是拿 `pending` 末尾当记忆 —— `min_segment_duration = 0` 时 `pending` 会被立即清空，
拿它当记忆会让 grace 判定永远失效。

**裁定 E（短段要真的并掉）** — *实施后新增，补齐裁定 A 缺失的一半。*

`fold_short_segments` 在释放前把短于 `min_segment_duration` 的段并进相邻长段：

- 优先并入**前驱**长段；没有前驱则并入**后继**长段
- 正在生长的当前段也可作后继，但**仅当**该短段是 ready 的最后一个且没有别的段
  还卡在 pending 里——否则会跨过中间那段时间被并错（这是实现时踩到的一个真 bug）
- 两侧都找不到长邻居时**放回 pending 等下一轮**，而不是硬并：宁可不并，不可并错
- 等满 3 倍落库门槛仍无邻居，则原样放行，避免孤立短段永远卡在 pending 里

合并会把两段的 `start_at`/`end_at` 取并集边界、`evidence_event_ids` 拼接。

**裁定 D（落库门槛 = max(min_segment_duration, grace_period)）** — *实施后新增。*

吸收只能复活"还没落库"的段，所以落库门槛必须覆盖整个 grace 窗口，
否则宽限期内回来的段已经落库、改不了了。这也修掉了 spec 自身默认值的一个隐患：
`min_segment_duration(30s) < grace_period(60s)`，按字面实现会让 grace 吸收不可靠。

- [ ] **Step 1: 写失败测试 `src-tauri/crates/engine/src/tests/segmenter.rs`**

```rust
use crate::{reduce, EngineConfig, EngineState, RuleSet, DEFAULT_RULES_TOML};
use activity_core::{Event, EventType, InputHeartbeatPayload, WindowFocusPayload};

fn rules() -> RuleSet {
    RuleSet::from_toml(DEFAULT_RULES_TOML).unwrap()
}

fn focus(ts: i64, app: &str) -> Event {
    Event::new(
        EventType::WindowFocus(WindowFocusPayload {
            process_name: app.into(),
            window_title: None,
            exe_path: None,
        }),
        ts,
    )
}

fn heartbeat(ts: i64, secs: u8) -> Event {
    Event::new(
        EventType::InputHeartbeat(InputHeartbeatPayload { active_seconds: secs }),
        ts,
    )
}

/// 取全部段：已关闭的 + 正在生长的当前段。
fn all_segments(events: &[Event], rs: &RuleSet, cfg: &EngineConfig) -> Vec<crate::ActivitySegment> {
    let mut st = EngineState::initial();
    let mut all = Vec::new();
    for e in events {
        let out = reduce(&st, e, rs, cfg);
        all.extend(out.closed_segments);
        st = out.state;
    }
    if let Some(open) = st.open_segment_snapshot(rs, cfg) {
        all.push(open);
    }
    all
}

fn cats(events: &[Event]) -> Vec<String> {
    all_segments(events, &rules(), &EngineConfig::default().with_min_segment_duration_s(0))
        .iter()
        .map(|s| s.category.as_str().to_string())
        .collect()
}

/// 立即落库：min 和 grace 都设 0，落库门槛 max(0,0)=0。
/// 只想验证"分段的形状"而不涉及抖动语义时用它。
/// 涉及 grace 的测试必须保留默认 grace（见下面那几个）。
fn emit_now() -> EngineConfig {
    EngineConfig::default()
        .with_min_segment_duration_s(0)
        .with_grace_period_s(0)
}

// --- 基本：同应用延长 ---

#[test]
fn single_event_creates_one_segment() {
    let segs = all_segments(&[focus(1_000, "Code.exe")], &rules(), &emit_now());
    assert_eq!(segs.len(), 1, "只有 1 条 Event 也要出 1 段");
    assert_eq!(segs[0].category.as_str(), "work");
    assert_eq!(segs[0].start_at, 1_000);
    assert_eq!(segs[0].application.as_deref(), Some("Code.exe"));
}

#[test]
fn same_app_extends_one_segment() {
    let segs = all_segments(
        &[
            focus(1_000, "Code.exe"),
            focus(10_000, "Code.exe"),
            focus(20_000, "Code.exe"),
        ],
        &rules(),
        &emit_now(),
    );
    assert_eq!(segs.len(), 1, "同一应用不应切段");
    assert_eq!(segs[0].start_at, 1_000);
    assert_eq!(segs[0].end_at, 20_000);
    assert_eq!(segs[0].evidence_event_ids.len(), 3, "每个 Event 都应成为证据");
}

#[test]
fn segment_end_is_always_at_or_after_start() {
    // 跨零点 / 乱序时间戳也不能产生负长度段
    let segs = all_segments(
        &[focus(5_000, "Code.exe"), focus(1_000, "Code.exe")],
        &rules(),
        &emit_now(),
    );
    for s in &segs {
        assert!(s.end_at >= s.start_at, "负长度段: {s:?}");
        assert!(s.duration_ms() >= 0);
    }
}

// --- 切段 ---

#[test]
fn different_app_splits_segment() {
    let segs = all_segments(
        &[focus(1_000, "Code.exe"), focus(600_000, "chrome.exe")],
        &rules(),
        &emit_now(),
    );
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].application.as_deref(), Some("Code.exe"));
    assert_eq!(segs[1].application.as_deref(), Some("chrome.exe"));
    assert_eq!(segs[0].end_at, 600_000, "旧段应在切换点收尾");
    assert_eq!(segs[1].start_at, 600_000);
}

#[test]
fn unknown_app_gets_unknown_category() {
    let segs = all_segments(&[focus(1_000, "MysteryApp.exe")], &rules(), &emit_now());
    assert_eq!(segs[0].category.as_str(), "unknown");
    assert_eq!(segs[0].confidence, 0.0);
}

#[test]
fn segments_are_ordered_and_non_overlapping() {
    let events = vec![
        focus(0, "Code.exe"),
        focus(600_000, "chrome.exe"),
        focus(1_200_000, "Code.exe"),
        focus(1_800_000, "QQ.exe"),
    ];
    let segs = all_segments(&events, &rules(), &emit_now());
    for w in segs.windows(2) {
        assert!(w[0].end_at <= w[1].start_at, "段不应重叠: {:?} {:?}", w[0], w[1]);
    }
}

// --- grace period（spec §7.3）---

#[test]
fn grace_period_absorbs_a_quick_switch_back() {
    // Code -> chrome(2s) -> Code，三段应合并成一个 Code 段。
    // 关键是"复活"前一个 Code 段，而不是把 chrome 段留下来。
    let cfg = EngineConfig::default()
        .with_min_segment_duration_s(0)
        .with_grace_period_s(60);
    let segs = all_segments(
        &[
            focus(0, "Code.exe"),
            focus(2_000, "chrome.exe"),
            focus(4_000, "Code.exe"),
        ],
        &rules(),
        &cfg,
    );
    assert_eq!(segs.len(), 1, "grace 内的往返切换不该留下多段，实际 {:?}", cats(&[
        focus(0, "Code.exe"), focus(2_000, "chrome.exe"), focus(4_000, "Code.exe")
    ]));
    assert_eq!(segs[0].application.as_deref(), Some("Code.exe"));
    assert_eq!(segs[0].start_at, 0);
    assert_eq!(segs[0].end_at, 4_000);
}

#[test]
fn grace_period_does_not_absorb_a_slow_switch_back() {
    let cfg = EngineConfig::default()
        .with_min_segment_duration_s(0)
        .with_grace_period_s(60);
    let segs = all_segments(
        &[
            focus(0, "Code.exe"),
            focus(10_000, "chrome.exe"),
            focus(200_000, "Code.exe"),
        ],
        &rules(),
        &cfg,
    );
    assert!(segs.len() >= 2, "超出 grace 的往返应保留为多段，实际 {}", segs.len());
}

#[test]
fn zero_grace_disables_absorption() {
    let cfg = EngineConfig::default()
        .with_min_segment_duration_s(0)
        .with_grace_period_s(0);
    let segs = all_segments(
        &[focus(0, "Code.exe"), focus(1_000, "chrome.exe"), focus(2_000, "Code.exe")],
        &rules(),
        &cfg,
    );
    assert_eq!(segs.len(), 3, "grace=0 时每次切换都切段");
}

#[test]
fn grace_absorbs_the_evidence_of_the_dropped_segment() {
    // 被判为抖动的那个段，其证据应并回复活段，而不是凭空消失
    let cfg = EngineConfig::default()
        .with_min_segment_duration_s(0)
        .with_grace_period_s(60);
    let segs = all_segments(
        &[
            focus(0, "Code.exe"),
            focus(2_000, "chrome.exe"),
            focus(4_000, "Code.exe"),
        ],
        &rules(),
        &cfg,
    );
    assert_eq!(segs[0].evidence_event_ids.len(), 3, "三个 Event 都应留下证据");
}

// --- idle ---

#[test]
fn idle_starts_an_idle_segment() {
    let segs = all_segments(
        &[
            focus(0, "Code.exe"),
            Event::new(EventType::SystemIdle, 400_000),
            Event::new(EventType::SystemResume, 700_000),
        ],
        &rules(),
        &emit_now(),
    );
    let c = segs.iter().map(|s| s.category.as_str()).collect::<Vec<_>>();
    assert!(c.contains(&"idle"), "应有 idle 段，实际={:?}", c);
    let idle = segs.iter().find(|s| s.category.as_str() == "idle").unwrap();
    assert!(idle.start_at >= 400_000);
    assert!(idle.end_at >= idle.start_at);
}

#[test]
fn idle_closes_the_previous_work_segment() {
    let segs = all_segments(
        &[focus(0, "Code.exe"), Event::new(EventType::SystemIdle, 400_000)],
        &rules(),
        &emit_now(),
    );
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].category.as_str(), "work");
    assert_eq!(segs[0].end_at, 400_000, "工作段应在进入 idle 时收尾");
    assert_eq!(segs[1].category.as_str(), "idle");
}

#[test]
fn heartbeats_during_idle_do_not_extend_the_work_segment() {
    // spec §7.3：idle 期间的心跳不计入活跃
    let segs = all_segments(
        &[
            focus(0, "Code.exe"),
            heartbeat(1_000, 0),
            Event::new(EventType::SystemIdle, 400_000),
            heartbeat(410_000, 0),
            heartbeat(420_000, 0),
        ],
        &rules(),
        &emit_now(),
    );
    let work = segs.iter().find(|s| s.category.as_str() == "work").unwrap();
    assert_eq!(work.end_at, 400_000, "工作段不应被 idle 期的心跳撑长");
}

// --- 纯函数性（spec §7.4 可重放）---

#[test]
fn reduce_is_pure_same_input_same_output() {
    let events = vec![focus(0, "Code.exe"), focus(600_000, "chrome.exe")];
    let (rs, cfg) = (rules(), emit_now());
    let run = || {
        let mut st = EngineState::initial();
        for e in &events {
            st = reduce(&st, e, &rs, &cfg).state;
        }
        st
    };
    assert_eq!(run(), run(), "同一串事件必须得到同一状态");
}

#[test]
fn reduce_does_not_mutate_input_state() {
    let rs = rules();
    let cfg = EngineConfig::default();
    let st = EngineState::initial();
    let _ = reduce(&st, &focus(0, "Code.exe"), &rs, &cfg);
    assert!(st.current_segment.is_none());
    assert!(st.current_context.is_none());
}

#[test]
fn replay_after_rules_change_produces_different_categories() {
    let events = vec![focus(0, "Code.exe")];
    let before = all_segments(&events, &rules(), &emit_now());
    let other = RuleSet::from_toml(
        r#"
[[rule]]
id = "everything"
process = ["Code.exe"]
category = "entertainment"
confidence = 0.1
"#,
    )
    .unwrap();
    let after = all_segments(&events, &other, &emit_now());
    assert_eq!(before[0].category.as_str(), "work");
    assert_eq!(after[0].category.as_str(), "entertainment");
    assert_ne!(before[0].classifier_version, after[0].classifier_version);
}

// --- open segment ---

#[test]
fn open_segment_is_not_in_closed_segments() {
    let (rs, cfg) = (rules(), emit_now());
    let out = reduce(&EngineState::initial(), &focus(0, "Code.exe"), &rs, &cfg);
    assert!(out.closed_segments.is_empty(), "未关闭的段不该出现在 closed 里");
    assert!(out.state.current_segment.is_some());
}

#[test]
fn open_segment_snapshot_mirrors_the_growing_segment() {
    let (rs, cfg) = (rules(), EngineConfig::default());
    let mut st = reduce(&EngineState::initial(), &focus(0, "Code.exe"), &rs, &cfg).state;
    st = reduce(&st, &focus(5_000, "Code.exe"), &rs, &cfg).state;
    let snap = st.open_segment_snapshot(&rs, &cfg).expect("应有正在生长的段");
    assert_eq!(snap.start_at, 0);
    assert_eq!(snap.end_at, 5_000);
    assert_eq!(snap.category.as_str(), "work");
    assert_eq!(snap.classifier, crate::CLASSIFIER_RULE);
}

#[test]
fn open_segment_snapshot_is_none_when_idle_and_empty() {
    let (rs, cfg) = (rules(), EngineConfig::default());
    assert!(EngineState::initial().open_segment_snapshot(&rs, &cfg).is_none());
}

// --- min_segment_duration 缓冲 ---

#[test]
fn short_segments_stay_pending_until_min_duration_elapses() {
    let (rs, cfg) = (rules(), EngineConfig::default().with_min_segment_duration_s(30));
    let mut st = reduce(&EngineState::initial(), &focus(0, "Code.exe"), &rs, &cfg).state;
    st = reduce(&st, &focus(1_000, "chrome.exe"), &rs, &cfg).state;
    assert!(st.current_segment.is_some(), "应该有正在生长的 chrome 段");
    let snapshot = st.open_segment_snapshot(&rs, &cfg).unwrap();
    assert_eq!(snapshot.application.as_deref(), Some("chrome.exe"));
    assert_eq!(st.pending.len(), 1);
    assert_eq!(st.pending[0].application.as_deref(), Some("Code.exe"));
}

#[test]
fn pending_segments_are_released_once_old_enough() {
    let (rs, cfg) = (rules(), EngineConfig::default().with_min_segment_duration_s(30));
    let mut st = reduce(&EngineState::initial(), &focus(0, "Code.exe"), &rs, &cfg).state;
    st = reduce(&st, &focus(1_000, "chrome.exe"), &rs, &cfg).state;
    let out = reduce(&st, &heartbeat(100_000, 3), &rs, &cfg);
    assert!(
        !out.closed_segments.is_empty(),
        "超过 min_segment_duration 的 pending 段应被释放"
    );
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p activity-engine`
Expected: ERROR（`reduce` / `EngineState` 未定义）。

- [ ] **Step 3: 实现 `src-tauri/crates/engine/src/segmenter.rs`**

```rust
use crate::classifier::RuleSet;
use crate::config::EngineConfig;
use crate::context::ContextBuilder;
use crate::{ActivityContext, ActivitySegment, Category, OpenSegment, CLASSIFIER_RULE};
use activity_core::{Event, EventType};

/// 引擎的完整状态。**只有这个结构体是可变的**，其余全是纯函数。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EngineState {
    pub current_segment: Option<OpenSegment>,
    pub current_context: Option<ActivityContext>,
    pub is_idle: bool,
    /// 已切掉但还没到"可落库年龄"的段。
    /// 同时是 grace 抖动时**可回溯复活**的来源——已落库的段无法复活。
    pub pending: Vec<ActivitySegment>,
    /// 上一次**真正**切段时离开的那个应用。spec §7.3 的"再前一个"就是它。
    ///
    /// 必须独立于 `pending`：`min_segment_duration = 0` 时 pending 会被立刻清空，
    /// 若拿它当"再前一个"，grace 判定就永远失效。
    pub previous_application: Option<String>,
    /// 上一次**真正**切段的时刻。被判为抖动的吸收不更新它。
    pub last_switch_at: Option<i64>,
}

impl EngineState {
    pub fn initial() -> Self {
        Self::default()
    }

    /// 把正在生长的段投影成一个可展示的 ActivitySegment。
    ///
    /// **它还没落库** —— `get_segments` 用它把"当前这一段"附在结果末尾，
    /// 时间线因此是实时的。
    pub fn open_segment_snapshot(
        &self,
        rules: &RuleSet,
        _config: &EngineConfig,
    ) -> Option<ActivitySegment> {
        let open = self.current_segment.as_ref()?;
        Some(ActivitySegment {
            id: format!("open-{}", open.start_at),
            start_at: open.start_at,
            end_at: open.end_at,
            category: open.category,
            application: open.application.clone(),
            confidence: open.confidence,
            classifier: CLASSIFIER_RULE.to_string(),
            classifier_version: rules.version.clone(),
            evidence_event_ids: open.evidence_event_ids.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EngineOutput {
    pub state: EngineState,
    /// 已够老、可以落库的段
    pub closed_segments: Vec<ActivitySegment>,
}

/// 逐个吞 Event，吐出可落库的 Segment。**无 IO、无副作用**（spec §7.4）。
pub fn reduce(
    state: &EngineState,
    event: &Event,
    rules: &RuleSet,
    config: &EngineConfig,
) -> EngineOutput {
    let mut st = state.clone();
    let ts = event.timestamp;

    // 1. idle 状态翻转
    match event.event_type {
        EventType::SystemIdle => st.is_idle = true,
        EventType::SystemResume => st.is_idle = false,
        _ => {}
    }

    // 2. 新上下文：**沿用上一次的**再应用这个 Event。
    //    不能每次从空 context 起算——心跳事件不携带 application，
    //    重建会把 application 冲成 None，导致"同一应用"判定失败、每来一个心跳就切一段。
    let mut new_ctx = st.current_context.clone().unwrap_or_default();
    new_ctx.is_idle = st.is_idle;
    ContextBuilder::apply(&mut new_ctx, event);

    match event.event_type {
        // --- idle 分支 ---
        EventType::SystemIdle => {
            close_current(&mut st, rules, ts);
            let mut open = new_open_segment(ts, Category::Idle, None, 0.0);
            open.evidence_event_ids.push(event.id.clone());
            st.current_segment = Some(open);
            st.current_context = Some(new_ctx);
            st.last_switch_at = Some(ts);
        }
        EventType::SystemResume => {
            // idle 段在 resume 时收尾；下一条事件会开新的 context 段
            close_current(&mut st, rules, ts);
            st.current_context = Some(new_ctx);
        }
        // --- context 分支 ---
        _ => {
            let cls = rules.classify(new_ctx.application.as_deref());
            let category = if st.is_idle {
                Category::Idle
            } else {
                cls.category
            };

            match st.current_segment.clone() {
                None => {
                    let mut open = new_open_segment(
                        ts,
                        category,
                        new_ctx.application.clone(),
                        cls.confidence,
                    );
                    open.evidence_event_ids.push(event.id.clone());
                    st.current_segment = Some(open);
                    st.last_switch_at = Some(ts);
                }
                Some(cur) => {
                    let same = cur.application == new_ctx.application
                        && cur.category == category;
                    if same {
                        extend(&mut st, cur, ts, event.id.clone());
                    } else if is_absorption(&st, &new_ctx, ts, config) {
                        absorb(&mut st, ts, event.id.clone());
                    } else {
                        close_current(&mut st, rules, ts);
                        st.previous_application = cur.application.clone();
                        let mut open = new_open_segment(
                            ts,
                            category,
                            new_ctx.application.clone(),
                            cls.confidence,
                        );
                        open.evidence_event_ids.push(event.id.clone());
                        st.current_segment = Some(open);
                        st.last_switch_at = Some(ts);
                    }
                }
            }
            st.current_context = Some(new_ctx);
        }
    }

    // 3. 释放够老的 pending 段
    let hold_ms = (config
        .min_segment_duration_s
        .max(config.grace_period_s) as i64)
        * 1000;
    let mut closed = Vec::new();
    let mut still = Vec::new();
    for seg in st.pending.drain(..) {
        if ts - seg.end_at >= hold_ms {
            closed.push(seg);
        } else {
            still.push(seg);
        }
    }
    st.pending = still;

    EngineOutput {
        state: st,
        closed_segments: closed,
    }
}

/// spec §7.3 的 grace 判定：距上次切换在宽限窗口内，**且新 context 与"再前一个"相同**。
///
/// 附加条件：前一段还留在 `pending` 里（没落库）。已落库的段无法复活，
/// 那时再判定为抖动也来不及了——落库门槛是 `max(min_segment_duration, grace_period)`，
/// 正好覆盖整个宽限窗口，所以正常情况下这个条件恒成立。
fn is_absorption(
    st: &EngineState,
    new_ctx: &ActivityContext,
    ts: i64,
    config: &EngineConfig,
) -> bool {
    if st.previous_application.as_deref() != new_ctx.application.as_deref() {
        return false;
    }
    let revisitable = st
        .pending
        .last()
        .map(|p| p.application == st.previous_application)
        .unwrap_or(false);
    if !revisitable {
        return false;
    }
    st.last_switch_at
        .map(|t| (ts - t).abs() < (config.grace_period_s as i64) * 1000)
        .unwrap_or(false)
}

/// 抖动吸收：丢弃刚开的那段（错误的 context），**复活** pending 末尾的前一段并延长。
///
/// 关键：如果只是"不切段"而把当前段留着，活下来的会是那个一闪而过的错误 context
/// （Code -> chrome(2s) -> Code 会留下一个 chrome 段），与用户实际经历不符。
fn absorb(st: &mut EngineState, ts: i64, evidence_id: String) {
    let Some(mut prev) = st.pending.pop() else {
        return;
    };
    let dropped = st.current_segment.take();

    prev.end_at = prev.end_at.max(ts);
    prev.evidence_event_ids.push(evidence_id);
    if let Some(d) = dropped {
        // 被丢弃段的证据并回来，别凭空消失
        prev.evidence_event_ids.extend(d.evidence_event_ids);
    }
    st.current_segment = Some(OpenSegment {
        start_at: prev.start_at,
        end_at: prev.end_at,
        category: prev.category,
        application: prev.application,
        confidence: prev.confidence,
        evidence_event_ids: prev.evidence_event_ids,
        last_evidence_at: ts,
    });
    // 抖动不是真切换：last_switch_at 与 previous_application 都保持不动
}

fn new_open_segment(
    ts: i64,
    category: Category,
    application: Option<String>,
    confidence: f32,
) -> OpenSegment {
    OpenSegment {
        start_at: ts,
        end_at: ts,
        category,
        application,
        confidence,
        evidence_event_ids: Vec::new(),
        last_evidence_at: ts,
    }
}

fn extend(st: &mut EngineState, mut cur: OpenSegment, ts: i64, evidence_id: String) {
    cur.end_at = cur.end_at.max(ts);
    cur.last_evidence_at = ts;
    cur.evidence_event_ids.push(evidence_id);
    st.current_segment = Some(cur);
}

fn close_current(st: &mut EngineState, rules: &RuleSet, ts: i64) {
    let Some(mut open) = st.current_segment.take() else {
        return;
    };
    open.end_at = open.end_at.max(ts);
    st.pending.push(ActivitySegment {
        id: format!("seg-{}", open.start_at),
        start_at: open.start_at,
        end_at: open.end_at,
        category: open.category,
        application: open.application,
        confidence: open.confidence,
        classifier: CLASSIFIER_RULE.to_string(),
        classifier_version: rules.version.clone(),
        evidence_event_ids: open.evidence_event_ids,
    });
}
```

- [ ] **Step 4: 导出并注册测试模块**

`lib.rs`：

```rust
use crate::classifier::RuleSet;
use crate::config::EngineConfig;
use crate::context::ContextBuilder;
use crate::{ActivityContext, ActivitySegment, Category, OpenSegment, CLASSIFIER_RULE};
use activity_core::{Event, EventType};

/// 引擎的完整状态。**只有这个结构体是可变的**，其余全是纯函数。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EngineState {
    pub current_segment: Option<OpenSegment>,
    pub current_context: Option<ActivityContext>,
    pub is_idle: bool,
    /// 已切掉但还没到"可落库年龄"的段。
    /// 同时是 grace 抖动时**可回溯复活**的来源——已落库的段无法复活。
    pub pending: Vec<ActivitySegment>,
    /// 上一次**真正**切段时离开的那个应用。spec §7.3 的"再前一个"就是它。
    ///
    /// 必须独立于 `pending`：`min_segment_duration = 0` 时 pending 会被立刻清空，
    /// 若拿它当"再前一个"，grace 判定就永远失效。
    pub previous_application: Option<String>,
    /// 上一次**真正**切段的时刻。被判为抖动的吸收不更新它。
    pub last_switch_at: Option<i64>,
}

impl EngineState {
    pub fn initial() -> Self {
        Self::default()
    }

    /// 把正在生长的段投影成一个可展示的 ActivitySegment。
    ///
    /// **它还没落库** —— `get_segments` 用它把"当前这一段"附在结果末尾，
    /// 时间线因此是实时的。
    pub fn open_segment_snapshot(
        &self,
        rules: &RuleSet,
        _config: &EngineConfig,
    ) -> Option<ActivitySegment> {
        let open = self.current_segment.as_ref()?;
        Some(ActivitySegment {
            id: format!("open-{}", open.start_at),
            start_at: open.start_at,
            end_at: open.end_at,
            category: open.category,
            application: open.application.clone(),
            confidence: open.confidence,
            classifier: CLASSIFIER_RULE.to_string(),
            classifier_version: rules.version.clone(),
            evidence_event_ids: open.evidence_event_ids.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EngineOutput {
    pub state: EngineState,
    /// 已够老、可以落库的段
    pub closed_segments: Vec<ActivitySegment>,
}

/// 逐个吞 Event，吐出可落库的 Segment。**无 IO、无副作用**（spec §7.4）。
pub fn reduce(
    state: &EngineState,
    event: &Event,
    rules: &RuleSet,
    config: &EngineConfig,
) -> EngineOutput {
    let mut st = state.clone();
    let ts = event.timestamp;

    // 1. idle 状态翻转
    match event.event_type {
        EventType::SystemIdle => st.is_idle = true,
        EventType::SystemResume => st.is_idle = false,
        _ => {}
    }

    // 2. 新上下文：**沿用上一次的**再应用这个 Event。
    //    不能每次从空 context 起算——心跳事件不携带 application，
    //    重建会把 application 冲成 None，导致"同一应用"判定失败、每来一个心跳就切一段。
    let mut new_ctx = st.current_context.clone().unwrap_or_default();
    new_ctx.is_idle = st.is_idle;
    ContextBuilder::apply(&mut new_ctx, event);

    match event.event_type {
        // --- idle 分支 ---
        EventType::SystemIdle => {
            close_current(&mut st, rules, ts);
            let mut open = new_open_segment(ts, Category::Idle, None, 0.0);
            open.evidence_event_ids.push(event.id.clone());
            st.current_segment = Some(open);
            st.current_context = Some(new_ctx);
            st.last_switch_at = Some(ts);
        }
        EventType::SystemResume => {
            // idle 段在 resume 时收尾；下一条事件会开新的 context 段
            close_current(&mut st, rules, ts);
            st.current_context = Some(new_ctx);
        }
        // --- context 分支 ---
        _ => {
            let cls = rules.classify(new_ctx.application.as_deref());
            let category = if st.is_idle {
                Category::Idle
            } else {
                cls.category
            };

            match st.current_segment.clone() {
                None => {
                    let mut open = new_open_segment(
                        ts,
                        category,
                        new_ctx.application.clone(),
                        cls.confidence,
                    );
                    open.evidence_event_ids.push(event.id.clone());
                    st.current_segment = Some(open);
                    st.last_switch_at = Some(ts);
                }
                Some(cur) => {
                    let same = cur.application == new_ctx.application
                        && cur.category == category;
                    if same {
                        extend(&mut st, cur, ts, event.id.clone());
                    } else if is_absorption(&st, &new_ctx, ts, config) {
                        absorb(&mut st, ts, event.id.clone());
                    } else {
                        close_current(&mut st, rules, ts);
                        st.previous_application = cur.application.clone();
                        let mut open = new_open_segment(
                            ts,
                            category,
                            new_ctx.application.clone(),
                            cls.confidence,
                        );
                        open.evidence_event_ids.push(event.id.clone());
                        st.current_segment = Some(open);
                        st.last_switch_at = Some(ts);
                    }
                }
            }
            st.current_context = Some(new_ctx);
        }
    }

    // 3. 释放够老的 pending 段
    let hold_ms = (config
        .min_segment_duration_s
        .max(config.grace_period_s) as i64)
        * 1000;
    let mut closed = Vec::new();
    let mut still = Vec::new();
    for seg in st.pending.drain(..) {
        if ts - seg.end_at >= hold_ms {
            closed.push(seg);
        } else {
            still.push(seg);
        }
    }
    st.pending = still;

    EngineOutput {
        state: st,
        closed_segments: closed,
    }
}

/// spec §7.3 的 grace 判定：距上次切换在宽限窗口内，**且新 context 与"再前一个"相同**。
///
/// 附加条件：前一段还留在 `pending` 里（没落库）。已落库的段无法复活，
/// 那时再判定为抖动也来不及了——落库门槛是 `max(min_segment_duration, grace_period)`，
/// 正好覆盖整个宽限窗口，所以正常情况下这个条件恒成立。
fn is_absorption(
    st: &EngineState,
    new_ctx: &ActivityContext,
    ts: i64,
    config: &EngineConfig,
) -> bool {
    if st.previous_application.as_deref() != new_ctx.application.as_deref() {
        return false;
    }
    let revisitable = st
        .pending
        .last()
        .map(|p| p.application == st.previous_application)
        .unwrap_or(false);
    if !revisitable {
        return false;
    }
    st.last_switch_at
        .map(|t| (ts - t).abs() < (config.grace_period_s as i64) * 1000)
        .unwrap_or(false)
}

/// 抖动吸收：丢弃刚开的那段（错误的 context），**复活** pending 末尾的前一段并延长。
///
/// 关键：如果只是"不切段"而把当前段留着，活下来的会是那个一闪而过的错误 context
/// （Code -> chrome(2s) -> Code 会留下一个 chrome 段），与用户实际经历不符。
fn absorb(st: &mut EngineState, ts: i64, evidence_id: String) {
    let Some(mut prev) = st.pending.pop() else {
        return;
    };
    let dropped = st.current_segment.take();

    prev.end_at = prev.end_at.max(ts);
    prev.evidence_event_ids.push(evidence_id);
    if let Some(d) = dropped {
        // 被丢弃段的证据并回来，别凭空消失
        prev.evidence_event_ids.extend(d.evidence_event_ids);
    }
    st.current_segment = Some(OpenSegment {
        start_at: prev.start_at,
        end_at: prev.end_at,
        category: prev.category,
        application: prev.application,
        confidence: prev.confidence,
        evidence_event_ids: prev.evidence_event_ids,
        last_evidence_at: ts,
    });
    // 抖动不是真切换：last_switch_at 与 previous_application 都保持不动
}

fn new_open_segment(
    ts: i64,
    category: Category,
    application: Option<String>,
    confidence: f32,
) -> OpenSegment {
    OpenSegment {
        start_at: ts,
        end_at: ts,
        category,
        application,
        confidence,
        evidence_event_ids: Vec::new(),
        last_evidence_at: ts,
    }
}

fn extend(st: &mut EngineState, mut cur: OpenSegment, ts: i64, evidence_id: String) {
    cur.end_at = cur.end_at.max(ts);
    cur.last_evidence_at = ts;
    cur.evidence_event_ids.push(evidence_id);
    st.current_segment = Some(cur);
}

fn close_current(st: &mut EngineState, rules: &RuleSet, ts: i64) {
    let Some(mut open) = st.current_segment.take() else {
        return;
    };
    open.end_at = open.end_at.max(ts);
    st.pending.push(ActivitySegment {
        id: format!("seg-{}", open.start_at),
        start_at: open.start_at,
        end_at: open.end_at,
        category: open.category,
        application: open.application,
        confidence: open.confidence,
        classifier: CLASSIFIER_RULE.to_string(),
        classifier_version: rules.version.clone(),
        evidence_event_ids: open.evidence_event_ids,
    });
}
pub mod classifier;
pub mod config;
pub mod context;
pub mod segmenter;
pub mod types;

pub use classifier::{Classification, Rule, RuleError, RuleSet, DEFAULT_RULES_TOML};
pub use config::EngineConfig;
pub use context::{context_of, ContextBuilder};
pub use segmenter::{reduce, EngineOutput, EngineState};
pub use types::{ActivityContext, ActivitySegment, Category, OpenSegment, CLASSIFIER_RULE};
```

`tests/mod.rs`：

```rust
mod classifier;
mod config;
mod context;
mod segmenter;
mod types;
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p activity-engine`
Expected: PASS（53 tests）。

> **已实施。** 计划初稿的 grace 代理判定被推翻了，理由与最终实现见 Task 5 开头的
> 裁定 C / 裁定 D，以及 Self-Review 的"已修正的计划缺陷"。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/crates/engine/
git commit -m "feat(engine): add reduce() segmenter state machine with grace period and pending buffer"
```

---

## Task 6: storage — 活动段与证据的写入/查询

**Files:**
- Create: `src-tauri/crates/storage/src/activity.rs`
- Create: `src-tauri/crates/storage/tests/activity.rs`
- Modify: `src-tauri/crates/storage/src/lib.rs`

**Interfaces:**
- Consumes: `activity_core::Event`（取 id 存 evidence）
- Produces:
  - `activity_storage::StoredSegment`（`#[serde(rename_all = "camelCase")]` 的前端友好形态，字段与 TS 对齐）
  - `activity_storage::insert_segments(conn: &Connection, segs: &[(&StoredSegment, &[String])]) -> rusqlite::Result<()>`（单事务，写 activities + activity_evidence）
  - `activity_storage::delete_segments_for_day(conn, start_ms, end_ms) -> rusqlite::Result<()>`（重算某天时先清空）
  - `activity_storage::get_segments_in_range(conn, start_ms, end_ms) -> rusqlite::Result<Vec<StoredSegment>>`

**为什么需要 `delete_segments_for_day`:** spec §7.4 说 engine 可重放。重放某一天 =
清空那天的 activities + evidence，重跑引擎，再写回去。没有这个函数，重放会不断累积重复段。

- [ ] **Step 1: 写失败测试 `src-tauri/crates/storage/tests/activity.rs`**

```rust
use activity_storage::{
    delete_segments_for_day, get_segments_in_range, insert_segments, open_in_memory,
    StoredSegment,
};

fn seg(id: &str, start: i64, end: i64, cat: &str) -> StoredSegment {
    StoredSegment {
        id: id.into(),
        start_at: start,
        end_at: end,
        category: cat.into(),
        application: Some("Code.exe".into()),
        confidence: 0.9,
        classifier: "rule".into(),
        classifier_version: "rules:3".into(),
        evidence_event_ids: vec![],
    }
}

#[test]
fn insert_and_read_back_a_segment() {
    let conn = open_in_memory();
    let s = seg("s1", 0, 60_000, "work");
    insert_segments(&conn, &[(s.clone(), vec!["e1".into(), "e2".into()])]).unwrap();
    let rows = get_segments_in_range(&conn, 0, 100_000).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "s1");
    assert_eq!(rows[0].category, "work");
    assert_eq!(rows[0].application.as_deref(), Some("Code.exe"));
    assert_eq!(rows[0].evidence_event_ids.len(), 2);
}

#[test]
fn range_is_half_open_and_ordered() {
    let conn = open_in_memory();
    insert_segments(
        &conn,
        &[
            (seg("b", 60_000, 120_000, "work"), vec![]),
            (seg("a", 0, 60_000, "work"), vec![]),
        ],
    )
    .unwrap();
    let rows = get_segments_in_range(&conn, 0, 120_000).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, "a", "应按 start_at 升序");
    assert_eq!(rows[1].id, "b");
    // 恰好等于 end 的不收
    assert_eq!(get_segments_in_range(&conn, 0, 60_000).unwrap().len(), 1);
}

#[test]
fn evidence_is_deduplicated() {
    let conn = open_in_memory();
    insert_segments(
        &conn,
        &[(seg("s1", 0, 1000, "work"), vec!["e1".into(), "e1".into()])],
    )
    .unwrap();
    let rows = get_segments_in_range(&conn, 0, 2000).unwrap();
    assert_eq!(rows[0].evidence_event_ids.len(), 1, "PRIMARY KEY 应去重");
}

#[test]
fn reinserting_same_segment_id_replaces() {
    let conn = open_in_memory();
    insert_segments(&conn, &[(seg("s1", 0, 1000, "work"), vec![])]).unwrap();
    let mut updated = seg("s1", 0, 2000, "browsing");
    updated.evidence_event_ids.clear();
    insert_segments(&conn, &[(updated, vec!["e9".into()])]).unwrap();
    let rows = get_segments_in_range(&conn, 0, 5000).unwrap();
    assert_eq!(rows.len(), 1, "同 id 应覆盖而非重复");
    assert_eq!(rows[0].category, "browsing");
    assert_eq!(rows[0].end_at, 2000);
}

#[test]
fn insert_empty_slice_is_noop() {
    let conn = open_in_memory();
    insert_segments(&conn, &[]).unwrap();
    assert!(get_segments_in_range(&conn, 0, i64::MAX).unwrap().is_empty());
}

#[test]
fn delete_removes_segments_and_their_evidence_only() {
    let conn = open_in_memory();
    insert_segments(
        &conn,
        &[
            (seg("old", 0, 1000, "work"), vec!["e1".into()]),
            (seg("new", 5_000_000, 5_001_000, "work"), vec!["e2".into()]),
        ],
    )
    .unwrap();
    delete_segments_for_day(&conn, 0, 1_000_000).unwrap();
    let rows = get_segments_in_range(&conn, 0, i64::MAX).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "new");
    // 孤儿的 evidence 行应被清掉（外键 ON DELETE CASCADE 不覆盖这种手工删除）
    let orphans: i64 = conn
        .query_row("SELECT COUNT(*) FROM activity_evidence", [], |r| r.get(0))
        .unwrap();
    assert_eq!(orphans, 1, "只应剩下 new 的证据");
}

#[test]
fn delete_is_idempotent() {
    let conn = open_in_memory();
    insert_segments(&conn, &[(seg("s1", 0, 1000, "work"), vec![])]).unwrap();
    delete_segments_for_day(&conn, 0, 1_000_000).unwrap();
    delete_segments_for_day(&conn, 0, 1_000_000).unwrap();
    assert!(get_segments_in_range(&conn, 0, i64::MAX).unwrap().is_empty());
}

#[test]
fn segments_with_null_application_roundtrip() {
    let conn = open_in_memory();
    let mut s = seg("s1", 0, 1000, "idle");
    s.application = None;
    insert_segments(&conn, &[(s, vec![])]).unwrap();
    assert!(get_segments_in_range(&conn, 0, 2000).unwrap()[0].application.is_none());
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p activity-storage --test activity`
Expected: ERROR（`insert_segments` 等未定义）。

- [ ] **Step 3: 实现 `src-tauri/crates/storage/src/activity.rs`**

```rust
use rusqlite::Connection;

/// 落库后再读出的 ActivitySegment。
/// `#[serde(rename_all = "camelCase")]` 让前端 TS 侧直接同名对接。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSegment {
    pub id: String,
    pub start_at: i64,
    pub end_at: i64,
    pub category: String,
    pub application: Option<String>,
    pub confidence: f32,
    pub classifier: String,
    pub classifier_version: String,
    #[serde(default)]
    pub evidence_event_ids: Vec<String>,
}

/// 单事务写入 segments 及其 evidence。`(段, 该段的 evidence event id)` 配对。
pub fn insert_segments(
    conn: &Connection,
    segments: &[(&StoredSegment, Vec<String>)],
) -> rusqlite::Result<()> {
    if segments.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    {
        let mut ins = tx.prepare(
            "INSERT OR REPLACE INTO activities
                (id, start_at, end_at, category, application, confidence, classifier, version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        let mut ev = tx.prepare(
            "INSERT OR IGNORE INTO activity_evidence (activity_id, event_id) VALUES (?1, ?2)",
        )?;
        for (s, evidence) in segments {
            ins.execute(rusqlite::params![
                &s.id,
                s.start_at,
                s.end_at,
                &s.category,
                &s.application,
                s.confidence,
                &s.classifier,
                &s.classifier_version,
            ])?;
            // 先删后插：覆盖写时旧证据不该残留
            ev.execute(rusqlite::params![&s.id, ""])?;
            for e in evidence {
                ev.execute(rusqlite::params![&s.id, e])?;
            }
        }
    }
    tx.commit()
}

/// 清掉 `[start_ms, end_ms)` 内的段及其证据。**重放某一天前必须调用**（spec §7.4）。
pub fn delete_segments_for_day(
    conn: &Connection,
    start_ms: i64,
    end_ms: i64,
) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        // evidence 没有 ON DELETE CASCADE（外键指向 activities 的是引用方），
        // SQLite 的外键方向在这里是 activity -> event，所以要手动清。
        let mut del_ev = tx.prepare(
            "DELETE FROM activity_evidence
             WHERE activity_id IN (
                 SELECT id FROM activities WHERE start_at >= ?1 AND start_at < ?2
             )",
        )?;
        del_ev.execute(rusqlite::params![start_ms, end_ms])?;
        let mut del = tx.prepare(
            "DELETE FROM activities WHERE start_at >= ?1 AND start_at < ?2",
        )?;
        del.execute(rusqlite::params![start_ms, end_ms])?;
    }
    tx.commit()
}

pub fn get_segments_in_range(
    conn: &Connection,
    start_ms: i64,
    end_ms: i64,
) -> rusqlite::Result<Vec<StoredSegment>> {
    let mut stmt = conn.prepare(
        "SELECT id, start_at, end_at, category, application, confidence, classifier, version
         FROM activities
         WHERE start_at >= ?1 AND start_at < ?2
         ORDER BY start_at ASC",
    )?;
    let rows = stmt.query_map(rusqlite::params![start_ms, end_ms], |row| {
        let id: String = row.get(0)?;
        Ok(StoredSegment {
            id: id.clone(),
            start_at: row.get(1)?,
            end_at: row.get(2)?,
            category: row.get(3)?,
            application: row.get(4)?,
            confidence: row.get(5)?,
            classifier: row.get(6)?,
            classifier_version: row.get(7)?,
            evidence_event_ids: evidence_for(conn, &id)?,
        })
    })?;
    rows.collect()
}

fn evidence_for(conn: &Connection, segment_id: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT event_id FROM activity_evidence WHERE activity_id = ?1 ORDER BY event_id",
    )?;
    let rows = stmt.query_map(rusqlite::params![segment_id], |r| r.get::<_, String>(0))?;
    rows.collect()
}
```

> ⚠️ `evidence_for` 在 `query_map` 的闭包里再次借用 `conn`，而外层还有未 drop 的
> `stmt`。若编译报 borrow 冲突，把外层 `stmt` 先 `drop` 掉再收集。

- [ ] **Step 4: 导出**

`lib.rs` 加：

```rust
pub mod activity;
pub use activity::{delete_segments_for_day, get_segments_in_range, insert_segments, StoredSegment};
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p activity-storage`
Expected: PASS（migration 9 + query 8 + writer 8 + activity 8）。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/crates/storage/
git commit -m "feat(storage): add activity segment and evidence persistence with day-level delete"
```

---

## Task 7: app — 规则文件加载与落盘

**Files:**
- Create: `src-tauri/src/rules.rs`
- Create: `src-tauri/src/rules_test.rs`（或 `#[cfg(test)] mod` 内联）
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/Cargo.toml`（加 `toml` 不需要——engine 已用；这里需要 `dirs` 吗？**不需要**，`APPDATA` 已有）

**Interfaces:**
- Consumes: `activity_engine::{RuleSet, DEFAULT_RULES_TOML, RuleError}`
- Produces:
  - `fn rules_path() -> PathBuf`（`%APPDATA%/time-scope/rules.toml`）
  - `fn load_rules(path: &Path) -> (RuleSet, RulesSource)`——**首次启动从内置默认拷贝一份**，之后读用户的；解析失败退回内置默认并 `eprintln!`
  - `enum RulesSource { Created, Loaded, FellBackToDefault }`

- [ ] **Step 1: 写失败测试 `src-tauri/src/rules.rs`（内联 `#[cfg(test)]`）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ts-rules-{}-{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d.join("rules.toml")
    }

    #[test]
    fn creates_default_rules_file_on_first_run() {
        let p = tmp("first");
        let (rs, src) = load_rules(&p);
        assert_eq!(src, RulesSource::Created);
        assert!(p.exists(), "首次启动应写出规则文件");
        assert!(!rs.rules.is_empty());
        assert_eq!(rs.classify(Some("Code.exe")).category.as_str(), "work");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn second_run_reads_the_user_file() {
        let p = tmp("second");
        let _ = load_rules(&p);
        fs::write(&p, "[[rule]]\nid=\"x\"\nprocess=[\"a.exe\"]\ncategory=\"life\"\nconfidence=0.5\n").unwrap();
        let (rs, src) = load_rules(&p);
        assert_eq!(src, RulesSource::Loaded);
        assert_eq!(rs.classify(Some("a.exe")).category.as_str(), "life");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn broken_user_file_falls_back_to_defaults_without_crashing() {
        // Review Focus #5：用户把 rules.toml 改坏了应用不能崩
        let p = tmp("broken");
        fs::write(&p, "[[[ this is not toml").unwrap();
        let (rs, src) = load_rules(&p);
        assert_eq!(src, RulesSource::FellBackToDefault);
        assert!(!rs.rules.is_empty());
        assert_eq!(rs.classify(Some("Code.exe")).category.as_str(), "work");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn semantically_invalid_file_falls_back_too() {
        let p = tmp("badcat");
        fs::write(
            &p,
            "[[rule]]\nid=\"x\"\nprocess=[\"a.exe\"]\ncategory=\"nonsense\"\nconfidence=0.5\n",
        )
        .unwrap();
        let (_, src) = load_rules(&p);
        assert_eq!(src, RulesSource::FellBackToDefault);
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn broken_file_is_not_overwritten() {
        // 退回默认是内存行为，磁盘上用户的文件要留着给他自己修
        let p = tmp("keep");
        let original = "[[[ broken";
        fs::write(&p, original).unwrap();
        let _ = load_rules(&p);
        assert_eq!(fs::read_to_string(&p).unwrap(), original);
        let _ = fs::remove_file(&p);
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p time-scope --lib`
Expected: ERROR（`load_rules` 未定义）。

- [ ] **Step 3: 实现 `src-tauri/src/rules.rs`**

```rust
//! 规则文件的加载与首次拷贝（spec §7.2）。
//!
//! IO 全部在这一层；engine 只见到 `RuleSet` 这个值。

use activity_engine::{RuleSet, DEFAULT_RULES_TOML};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RulesSource {
    /// 首次启动，已从内置默认写盘
    Created,
    /// 读到了用户自己的文件
    Loaded,
    /// 文件坏了，内存里退回内置默认（磁盘上不动）
    FellBackToDefault,
}

pub fn rules_path() -> PathBuf {
    db_path()
        .parent()
        .map(|d| d.join("rules.toml"))
        .unwrap_or_else(|| PathBuf::from("rules.toml"))
}

fn db_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").expect("APPDATA env var");
    PathBuf::from(appdata)
        .join("time-scope")
        .join("time-scope.db")
}

fn default_rule_set() -> RuleSet {
    RuleSet::from_toml(DEFAULT_RULES_TOML).expect("内置默认规则必须始终可解析")
}

/// 加载规则集。**任何失败都不 panic**，一律退回内置默认。
pub fn load_rules(path: &Path) -> (RuleSet, RulesSource) {
    match std::fs::read_to_string(path) {
        Ok(text) => match RuleSet::from_toml(&text) {
            Ok(rs) => (rs, RulesSource::Loaded),
            Err(e) => {
                eprintln!(
                    "[time-scope] {} 解析失败（{e}），本次使用内置默认规则；\
                     修好后重启生效",
                    path.display()
                );
                (default_rule_set(), RulesSource::FellBackToDefault)
            }
        },
        Err(_) => {
            // 首次启动：把内置默认写出去，用户可以改
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if std::fs::write(path, DEFAULT_RULES_TOML).is_ok() {
                (default_rule_set(), RulesSource::Created)
            } else {
                (default_rule_set(), RulesSource::FellBackToDefault)
            }
        }
    }
}
```

- [ ] **Step 4: 在 `src-tauri/src/lib.rs` 加 `mod rules;`**

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p time-scope --lib`
Expected: PASS（原 11 + 新 5 = 16 tests）。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/
git commit -m "feat(app): load rules.toml with first-run default copy and safe fallback"
```

---

## Task 8: app — 引擎运行时与 get_segments

**Files:**
- Create: `src-tauri/src/engine_runtime.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `activity_engine::{reduce, EngineState, EngineConfig, RuleSet}`、`activity_storage::{get_events_in_range, get_segments_in_range, insert_segments, StoredSegment}`、`BatchWriter`
- Produces:
  - `EngineRuntime { state: EngineState, rules: RuleSet, config: EngineConfig }`
  - `EngineRuntime::bootstrap(conn, rules, config) -> Self`——**把当天已有的 Event 重放一遍**，让 UI 一上来就有历史
  - `EngineRuntime::ingest(&mut self, event: &Event)`
  - `EngineRuntime::segments_for_day(&self, conn, start_ms, end_ms) -> rusqlite::Result<Vec<StoredSegment>>`——查库结果 + 正在生长的当前段
  - Tauri command `get_segments(date: String) -> Result<Vec<StoredSegment>, String>`

- [ ] **Step 1: 写失败测试 `src-tauri/src/engine_runtime.rs`（内联 `#[cfg(test)]`）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use activity_core::{Event, EventType, WindowFocusPayload};
    use activity_storage::{insert_events, open_in_memory_shared};

    fn focus(ts: i64, app: &str) -> Event {
        Event::new(
            EventType::WindowFocus(WindowFocusPayload {
                process_name: app.into(),
                window_title: None,
                exe_path: None,
            }),
            ts,
        )
    }

    fn runtime() -> EngineRuntime {
        let rules = activity_engine::RuleSet::from_toml(activity_engine::DEFAULT_RULES_TOML).unwrap();
        EngineRuntime::new(rules, EngineConfig::default().with_min_segment_duration_s(0).with_grace_period_s(0))
    }

    #[test]
    fn ingest_builds_segments_in_memory() {
        let mut rt = runtime();
        rt.ingest(&focus(0, "Code.exe"));
        rt.ingest(&focus(600_000, "chrome.exe"));
        let snap = rt.state().open_segment_snapshot(rt.rules(), rt.config());
        assert!(snap.is_some());
    }

    #[test]
    fn bootstrap_replays_existing_events_of_the_day() {
        // 冷启动时 DB 里已经有今天的 Event，UI 应当立刻看得到段
        let conn = open_in_memory_shared();
        let events: Vec<Event> = vec![
            focus(0, "Code.exe"),
            focus(600_000, "chrome.exe"),
            focus(1_200_000, "Code.exe"),
        ];
        insert_events(&conn.lock().unwrap(), &events).unwrap();

        let rt = EngineRuntime::bootstrap(&conn, runtime().rules().clone(), EngineConfig::default().with_min_segment_duration_s(0).with_grace_period_s(0), 0, 2_000_000).unwrap();

        let segs = rt.segments_for_day(&conn, 0, 2_000_000).unwrap();
        assert!(segs.len() >= 2, "重放应产出多段，实际={}", segs.len());
        let cats: Vec<&str> = segs.iter().map(|s| s.category.as_str()).collect();
        assert!(cats.contains(&"work"), "{:?}", cats);
        assert!(cats.contains(&"browsing"), "{:?}", cats);
    }

    #[test]
    fn replay_is_idempotent_across_restarts() {
        // 重放两次不应产生两倍的段
        let conn = open_in_memory_shared();
        insert_events(
            &conn.lock().unwrap(),
            &[focus(0, "Code.exe"), focus(600_000, "chrome.exe")],
        )
        .unwrap();
        let cfg = EngineConfig::default().with_min_segment_duration_s(0).with_grace_period_s(0);
        for _ in 0..2 {
            let rt = EngineRuntime::bootstrap(
                &conn,
                activity_engine::RuleSet::from_toml(activity_engine::DEFAULT_RULES_TOML).unwrap(),
                cfg,
                0,
                2_000_000,
            )
            .unwrap();
            let n = rt.segments_for_day(&conn, 0, 2_000_000).unwrap().len();
            assert!((1..=4).contains(&n), "重放后段数应稳定，实际={n}");
        }
    }

    #[test]
    fn segments_are_sorted_by_start() {
        let conn = open_in_memory_shared();
        insert_events(
            &conn.lock().unwrap(),
            &[focus(1_200_000, "Code.exe"), focus(0, "chrome.exe"), focus(600_000, "Code.exe")],
        )
        .unwrap();
        let rt = EngineRuntime::bootstrap(
            &conn,
            activity_engine::RuleSet::from_toml(activity_engine::DEFAULT_RULES_TOML).unwrap(),
            EngineConfig::default().with_min_segment_duration_s(0).with_grace_period_s(0),
            0,
            2_000_000,
        )
        .unwrap();
        let segs = rt.segments_for_day(&conn, 0, 2_000_000).unwrap();
        for w in segs.windows(2) {
            assert!(w[0].start_at <= w[1].start_at, "{:?} {:?}", w[0], w[1]);
        }
    }

    #[test]
    fn live_ingest_persists_closed_segments() {
        let conn = open_in_memory_shared();
        let mut rt = runtime();
        rt.persist_target = Some(0);
        rt.ingest(&focus(0, "Code.exe"));
        rt.ingest(&focus(600_000, "chrome.exe"));
        // 第二次 ingest 关闭了 Code 段，应已落库
        let rows = activity_storage::get_segments_in_range(&conn.lock().unwrap(), 0, 2_000_000).unwrap();
        assert!(!rows.is_empty(), "关闭的段应被写入 activities");
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p time-scope --lib`
Expected: ERROR（`EngineRuntime` 未定义）。

- [ ] **Step 3: 实现 `src-tauri/src/engine_runtime.rs`**

```rust
//! 驱动 engine 的运行时：持有状态、喂 Event、把关掉的段落库。
//!
//! 这一层是"纯引擎"和"有 IO 的世界"之间唯一的桥。

use activity_core::Event;
use activity_engine::{reduce, EngineConfig, EngineState, RuleSet};
use activity_storage::{StoredSegment, SharedConn};
use rusqlite::Connection;
use std::sync::Arc;

pub struct EngineRuntime {
    state: EngineState,
    rules: RuleSet,
    config: EngineConfig,
    /// 落库用的连接
    conn: SharedConn,
    /// 正在写库的段的目标时间范围（当前天）
    persist_target: Option<(i64, i64)>,
}

impl EngineRuntime {
    pub fn new(rules: RuleSet, config: EngineConfig) -> Self {
        Self {
            state: EngineState::initial(),
            rules,
            config,
            conn: Arc::new(std::sync::Mutex::new(
                activity_storage::open_in_memory(),
            )),
            persist_target: None,
        }
    }

    pub fn state(&self) -> &EngineState {
        &self.state
    }

    pub fn rules(&self) -> &RuleSet {
        &self.rules
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// 冷启动：把当天已有 Event 重放一遍，并清掉那天旧的 activities。
    ///
    /// 先删后重放（spec §7.4）：不清会累积重复段。
    pub fn bootstrap(
        conn: &SharedConn,
        rules: RuleSet,
        config: EngineConfig,
        start_ms: i64,
        end_ms: i64,
    ) -> rusqlite::Result<Self> {
        {
            let c = conn.lock().unwrap();
            activity_storage::delete_segments_for_day(&c, start_ms, end_ms)?;
        }
        let events = {
            let c = conn.lock().unwrap();
            activity_storage::get_events_in_range(&c, start_ms, end_ms)?
        };
        let mut rt = Self {
            state: EngineState::initial(),
            rules,
            config,
            conn: Arc::clone(conn),
            persist_target: Some((start_ms, end_ms)),
        };
        for e in &events {
            rt.ingest(e);
        }
        Ok(rt)
    }

    /// 喂一个 Event，并把因此关闭的段落库。
    pub fn ingest(&mut self, event: &Event) {
        let out = reduce(&self.state, event, &self.rules, &self.config);
        self.state = out.state;

        if !out.closed_segments.is_empty() {
            if let Ok(c) = self.conn.lock() {
                let rows = to_stored(&out.closed_segments);
                if let Err(e) = activity_storage::insert_segments(&c, &rows) {
                    eprintln!("[time-scope] 写 activities 失败: {e}");
                }
            }
        }
    }

    /// 查某天的全部段 = 已落库的 + 正在生长的当前段（裁定 B）。
    pub fn segments_for_day(
        &self,
        conn: &SharedConn,
        start_ms: i64,
        end_ms: i64,
    ) -> rusqlite::Result<Vec<StoredSegment>> {
        let mut segs = {
            let c = conn.lock().unwrap();
            activity_storage::get_segments_in_range(&c, start_ms, end_ms)?
        };
        if let Some(open) = self.state.open_segment_snapshot(&self.rules, &self.config) {
            let ts = open.end_at;
            if ts >= start_ms && ts < end_ms {
                let mut s = to_stored_one(&open);
                s.id = format!("open-{}", s.start_at);
                segs.push(s);
            }
        }
        segs.sort_by_key(|s| s.start_at);
        Ok(segs)
    }
}

fn to_stored(segments: &[activity_engine::ActivitySegment]) -> Vec<(StoredSegment, Vec<String>)> {
    segments.iter().map(to_stored_one).collect()
}

fn to_stored_one(s: &activity_engine::ActivitySegment) -> (StoredSegment, Vec<String>) {
    (
        StoredSegment {
            id: s.id.clone(),
            start_at: s.start_at,
            end_at: s.end_at,
            category: s.category.as_str().to_string(),
            application: s.application.clone(),
            confidence: s.confidence,
            classifier: s.classifier.clone(),
            classifier_version: s.classifier_version.clone(),
            evidence_event_ids: Vec::new(),
        },
        s.evidence_event_ids.clone(),
    )
}
```

> 测试里用到了 `rt.persist_target` 这个字段，它是 `pub(crate)` 可见的内部字段；
> 若 `ingest` 的落库断言不稳，把 `live_ingest_persists_closed_segments` 改为直接查
> `rt.state().pending` 也能覆盖同一件事。

- [ ] **Step 4: 加 `get_segments` command 并改 `lib.rs`**

在 `lib.rs`：

```rust
mod engine_runtime;
mod rules;

use engine_runtime::EngineRuntime;

struct AppState {
    writer: Arc<BatchWriter>,
    runtime: std::sync::Mutex<EngineRuntime>,
}

#[tauri::command]
fn get_segments(
    state: tauri::State<'_, AppState>,
    date: String,
) -> Result<Vec<StoredSegment>, String> {
    let (start_ms, end_ms) = day_range_ms(&date, local_offset()?)?;
    let rt = state.runtime.lock().map_err(|e| e.to_string())?;
    rt.segments_for_day(&state.writer.conn(), start_ms, end_ms)
        .map_err(|e| e.to_string())
}
```

并在 `setup` 里替换原来的三线程裸调用为带引擎的版本：

```rust
let conn = open_file_shared(&path).expect("open db");
let writer = Arc::new(BatchWriter::new(Arc::clone(&conn), 5_000, 100));

let rules_path = rules::rules_path();
let (rule_set, source) = rules::load_rules(&rules_path);
match source {
    rules::RulesSource::Created => eprintln!("[time-scope] 已生成默认规则 {}", rules_path.display()),
    rules::RulesSource::Loaded => eprintln!("[time-scope] 已加载规则 {}", rules_path.display()),
    rules::RulesSource::FellBackToDefault => eprintln!("[time-scope] 规则文件不可用，用内置默认"),
}

// 只重放"今天"；引擎会在事件到来时持续推进
let today = day_range_ms(&today_string(), local_offset().unwrap_or(time::UtcOffset::UTC))?;
let runtime = EngineRuntime::bootstrap(
    &conn,
    rule_set,
    EngineConfig::default(),
    today.0,
    today.1,
)
.expect("bootstrap engine");

let (tx, rx) = channel::<RawSignal>();
activity_collector::window::spawn_window_watcher(tx.clone());
activity_collector::input::spawn_input_poller(tx.clone(), 300, 10);
activity_collector::consumer::spawn_consumer(rx, Arc::clone(&writer), Arc::clone(&conn));

app.manage(AppState {
    writer,
    runtime: std::sync::Mutex::new(runtime),
});
```

- [ ] **Step 5: 给 `consumer` 加落库回调**

`activity_collector::consumer::spawn_consumer` 增加第三个参数 `conn: SharedConn`，
在 `signal_to_event` 之后额外调 `runtime.ingest`。若不想改 collector 的签名，
也可以在 app 层用第二个 channel 把 Event 转发给一个 engine 线程。**推荐后者**，
因为它保持 collector 不认识 engine（spec §4 的边界）：

```rust
// src-tauri/src/engine_thread.rs
pub fn spawn_engine_thread(
    rx: std::sync::mpsc::Receiver<activity_core::Event>,
    runtime: Arc<std::sync::Mutex<EngineRuntime>>,
) { /* 循环 ingest */ }
```

- [ ] **Step 6: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p time-scope --lib`
Expected: PASS（16 + 5 = 21 tests）。

- [ ] **Step 7: 全量回归 + 提交**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --workspace`
Expected: 全绿。

```bash
git add src-tauri/
git commit -m "feat(app): add engine runtime with day replay and get_segments command"
```

---

## Task 9: 前端 — 分桶与类型

**Files:**
- Create: `src/lib/bucket.ts`
- Create: `src/lib/bucket.test.ts`
- Modify: `src/types.ts`

**Interfaces:**
- Produces:
  - `bucketStart(ts: number, intervalMs: number): number`（spec §8.2 的纯函数）
  - `bucketSegments(segments: ActivitySegment[], intervalMs: number): Bucket[]`
  - `Bucket { start: number, end: number, segments: ActivitySegment[] }`
  - `GRANULARITIES: [10, 30, 60, 120]`（分钟），默认 30
  - `ActivitySegment`（TS 镜像 Rust 的 `StoredSegment`）
  - `summarize(segments, dayStartMs): SummaryRow[]`（当日汇总，spec §9 说在前端算）

- [ ] **Step 1: 写失败测试 `src/lib/bucket.test.ts`**

```ts
import { describe, it, expect } from "vitest";
import { bucketStart, bucketSegments, summarize, GRanularITIES, DEFAULT_GRANULARITY } from "./bucket";
import type { ActivitySegment } from "../types";

const MIN = 60_000;
const HOUR = 60 * MIN;

function seg(id: string, start: number, end: number, category: string, app = "Code.exe"): ActivitySegment {
  return {
    id, startAt: start, endAt: end, category,
    application: app, confidence: 0.9,
    classifier: "rule", classifierVersion: "rules:3",
    evidenceEventIds: [],
  };
}

describe("bucketStart", () => {
  it("floors to the interval boundary", () => {
    expect(bucketStart(HOUR + 12345, HOUR)).toBe(HOUR);
    expect(bucketStart(HOUR - 1, HOUR)).toBe(0);
    expect(bucketStart(0, HOUR)).toBe(0);
  });

  it("works for 10/30/60/120 minute intervals", () => {
    for (const m of GRANULARITIES) {
      const iv = m * MIN;
      expect(bucketStart(iv * 3 + 1, iv)).toBe(iv * 3);
      expect(bucketStart(iv * 3 - 1, iv)).toBe(iv * 2);
    }
  });

  it("handles negative timestamps without throwing", () => {
    expect(bucketStart(-1, HOUR)).toBe(-HOUR);
  });
});

describe("bucketSegments", () => {
  it("returns one bucket per interval, ascending", () => {
    const bs = bucketSegments([seg("a", 0, HOUR, "work")], HOUR);
    expect(bs.length).toBe(1);
    expect(bs[0].start).toBe(0);
  });

  it("puts a segment into every bucket it spans", () => {
    const s = seg("a", 0, HOUR * 2 + 30 * MIN, "work");
    const bs = bucketSegments([s], HOUR);
    expect(bs.length).toBe(3, "跨 2.5 小时的段应落进 3 个小时桶");
    expect(bs.every((b) => b.segments.some((x) => x.id === "a"))).toBe(true);
  });

  it("a segment starting exactly on a boundary lands in the later bucket", () => {
    const s = seg("a", HOUR, HOUR * 2, "work");
    const bs = bucketSegments([s], HOUR);
    expect(bs[0].start).toBe(HOUR);
    expect(bs.length).toBe(1);
  });

  it("empty input yields empty buckets", () => {
    expect(bucketSegments([], HOUR)).toEqual([]);
  });

  it("switching granularity does not change which buckets a segment touches", () => {
    const s = seg("a", 0, HOUR * 2 + 30 * MIN, "work");
    for (const m of GRANULARITIES) {
      const bs = bucketSegments([s], m * MIN);
      expect(bs.length).toBeGreaterThan(0);
      const last = bs[bs.length - 1];
      expect(last.end).toBeGreaterThan(s.startAt);
    }
  });
});

describe("summarize", () => {
  it("totals duration per category, largest first", () => {
    const rows = summarize(
      [
        seg("a", 0, HOUR, "work"),
        seg("b", HOUR, HOUR * 2, "browsing"),
        seg("c", HOUR * 2, HOUR * 2 + 30 * MIN, "work"),
      ],
      0,
    );
    expect(rows[0].category).toBe("work");
    expect(rows[0].durationMs).toBe(HOUR + 30 * MIN);
    expect(rows[1].category).toBe("browsing");
    expect(rows[1].durationMs).toBe(HOUR);
  });

  it("includes an idle row computed from idle segments only", () => {
    const rows = summarize([seg("a", 0, HOUR, "work")], 0);
    expect(rows.find((r) => r.category === "idle")).toBeUndefined();
    const withIdle = summarize([seg("i", 0, HOUR, "idle")], 0);
    expect(withIdle[0].category).toBe("idle");
  });

  it("clamps segments to the requested day", () => {
    const rows = summarize([seg("a", 0, HOUR * 30, "work")], 0);
    expect(rows[0].durationMs).toBe(HOUR * 24, "应被截到一天");
  });

  it("empty input yields no rows", () => {
    expect(summarize([], 0)).toEqual([]);
  });
});

describe("granularity constants", () => {
  it("default is 30 minutes per spec", () => {
    expect(DEFAULT_GRANULARITY).toBe(30);
    expect(GRANULARITIES).toEqual([10, 30, 60, 120]);
  });
});
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm test src/lib/bucket.test.ts`
Expected: FAIL（模块不存在）。

- [ ] **Step 3: 在 `src/types.ts` 加 `ActivitySegment`**

```ts
/** Rust `StoredSegment` 的镜像。Rust 侧是 camelCase 序列化。 */
export interface ActivitySegment {
  id: string;
  startAt: number;
  endAt: number;
  category: Category;
  application: string | null;
  confidence: number;
  classifier: string;
  classifierVersion: string;
  evidenceEventIds: string[];
}

export type Category =
  | "work" | "study" | "entertainment" | "communication"
  | "browsing" | "life" | "idle" | "unknown";

export async function getSegments(date: string): Promise<ActivitySegment[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<ActivitySegment[]>("get_segments", { date });
}
```

- [ ] **Step 4: 实现 `src/lib/bucket.ts`**

```ts
import type { ActivitySegment, Category } from "../types";

export const GRANULARITIES = [10, 30, 60, 120] as const;
export const DEFAULT_GRANULARITY = 30;
const MINUTE = 60_000;
const DAY_MS = 86_400_000;

/** spec §8.2：把时间戳向下取整到区间起点。 */
export function bucketStart(ts: number, intervalMs: number): number {
  return Math.floor(ts / intervalMs) * intervalMs;
}

export interface Bucket {
  start: number;
  end: number;
  segments: ActivitySegment[];
}

/** 把段切进它跨越的各个桶。跨桶的段会在多个桶里各出现一次。 */
export function bucketSegments(
  segments: ActivitySegment[],
  intervalMs: number,
): Bucket[] {
  const byStart = new Map<number, ActivitySegment[]>();
  for (const s of segments) {
    const first = bucketStart(Math.max(s.startAt, 0), intervalMs);
    const last = bucketStart(Math.min(s.endAt, DAY_MS) - 1, intervalMs);
    for (let b = first; b <= last; b += intervalMs) {
      const arr = byStart.get(b);
      if (arr) arr.push(s);
      else byStart.set(b, [s]);
    }
  }
  return Array.from(byStart.entries())
    .sort((a, b) => a[0] - b[0])
    .map(([start, segs]) => ({ start, end: start + intervalMs, segments: segs }));
}

export interface SummaryRow {
  category: Category;
  durationMs: number;
  ratio: number;
}

/** 当日汇总。spec §9：后端不提供 summary 接口，由前端从 segments 聚合。 */
export function summarize(segments: ActivitySegment[], dayStartMs: number): SummaryRow[] {
  const dayEnd = dayStartMs + DAY_MS;
  const totals = new Map<Category, number>();
  for (const s of segments) {
    const start = Math.max(s.startAt, dayStartMs);
    const end = Math.min(s.endAt, dayEnd);
    const d = Math.max(0, end - start);
    if (d === 0) continue;
    totals.set(s.category, (totals.get(s.category) ?? 0) + d);
  }
  const total = Array.from(totals.values()).reduce((a, b) => a + b, 0);
  return Array.from(totals.entries())
    .map(([category, durationMs]) => ({
      category,
      durationMs,
      ratio: total > 0 ? durationMs / total : 0,
    }))
    .sort((a, b) => b.durationMs - a.durationMs);
}
```

- [ ] **Step 5: 运行测试确认通过**

Run: `pnpm test src/lib/bucket.test.ts`
Expected: PASS（16 tests）。

- [ ] **Step 6: 提交**

```bash
git add src/
git commit -m "feat(frontend): add time bucketing and day summary pure functions"
```

---

## Task 10: 前端 — 分类着色时间线 + 粒度切换 + 汇总

**Files:**
- Create: `src/components/GranularityPicker.tsx`
- Create: `src/components/DaySummary.tsx`
- Create: `src/components/SegmentTimeline.tsx`
- Modify: `src/App.tsx`
- Create: `src/components/SegmentTimeline.test.tsx`
- Modify: `src/components/EventDetail.tsx`

**Interfaces:**
- Consumes: `getSegments`、`bucketSegments`、`summarize`、`ActivitySegment`
- Produces: 分类配色的时间线、10/30/60/120 切换器、当日汇总条

- [ ] **Step 1: 写失败测试 `src/components/SegmentTimeline.test.tsx`**

```tsx
import { describe, it, expect } from "vitest";
import { render } from "@testing-library/react";
import SegmentTimeline from "./SegmentTimeline";
import type { ActivitySegment } from "../types";

const HOUR = 3_600_000;

function seg(id: string, start: number, end: number, category: ActivitySegment["category"]): ActivitySegment {
  return {
    id, startAt: start, endAt: end, category,
    application: "Code.exe", confidence: 0.9,
    classifier: "rule", classifierVersion: "rules:3", evidenceEventIds: [],
  };
}

describe("SegmentTimeline", () => {
  it("renders one rect per segment", () => {
    const { container } = render(
      <SegmentTimeline segments={[seg("a", 0, HOUR, "work"), seg("b", HOUR, HOUR * 2, "idle")]} dayStartMs={0} onSelect={() => {}} />,
    );
    expect(container.querySelectorAll("rect").length).toBe(2);
  });

  it("width is proportional to duration, not fixed", () => {
    const { container } = render(
      <SegmentTimeline
        segments={[seg("short", 0, 60_000, "work"), seg("long", 0, HOUR, "work")]}
        dayStartMs={0} onSelect={() => {}} />,
    );
    const r = Array.from(container.querySelectorAll("rect")) as SVGRectElement[];
    const byId = Object.fromEntries(r.map((x) => [x.getAttribute("data-id"), Number(x.getAttribute("width"))]));
    expect(byId["long"]).toBeGreaterThan(byId["short"]);
  });

  it("colors by category and gives each category a distinct color", () => {
    const cats = ["work", "browsing", "idle", "entertainment"] as const;
    const { container } = render(
      <SegmentTimeline
        segments={cats.map((c, i) => seg(c, i * HOUR, (i + 1) * HOUR, c))}
        dayStartMs={0} onSelect={() => {}} />,
    );
    const fills = Array.from(container.querySelectorAll("rect")).map((r) => r.getAttribute("fill"));
    expect(new Set(fills).size).toBe(cats.length);
  });

  it("clamps segments extending past the day", () => {
    const { container } = render(
      <SegmentTimeline segments={[seg("a", -HOUR, 86_400_000 + HOUR, "work")]} dayStartMs={0} onSelect={() => {}} />,
    );
    const r = container.querySelector("rect")!;
    expect(Number(r.getAttribute("x"))).toBeGreaterThanOrEqual(0);
    expect(Number(r.getAttribute("width"))).toBeLessThanOrEqual(1000);
  });

  it("calls onSelect on click", () => {
    let picked: string | null = null;
    const { container } = render(
      <SegmentTimeline segments={[seg("a", 0, HOUR, "work")]} dayStartMs={0} onSelect={(s) => { picked = s.id; }} />,
    );
    (container.querySelector("rect") as SVGRectElement)
      .dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(picked).toBe("a");
  });

  it("shows a hint for an empty day", () => {
    const { container } = render(
      <SegmentTimeline segments={[]} dayStartMs={0} onSelect={() => {}} />,
    );
    expect(container.textContent).toMatch(/还没有活动段/);
  });
});
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm test src/components/SegmentTimeline.test.tsx`
Expected: FAIL（组件不存在）。

- [ ] **Step 3: 实现 `src/components/SegmentTimeline.tsx`**

```tsx
import type { ActivitySegment, Category } from "../types";

const DAY_MS = 86_400_000;
const WIDTH = 1000;
const HEIGHT = 56;

export const CATEGORY_COLOR: Record<Category, string> = {
  work: "#4c8dff",
  study: "#7c6cff",
  entertainment: "#f0605f",
  communication: "#26a69a",
  browsing: "#ffb74d",
  life: "#8d9e6c",
  idle: "#b0b6bd",
  unknown: "#e0e0e0",
};

export function colorForCategory(c: Category): string {
  return CATEGORY_COLOR[c] ?? "#e0e0e0";
}

interface Props {
  segments: ActivitySegment[];
  dayStartMs: number;
  onSelect: (s: ActivitySegment) => void;
}

export default function SegmentTimeline({ segments, dayStartMs, onSelect }: Props) {
  if (segments.length === 0) {
    return <p style={{ color: "#666" }}>这一天还没有活动段。</p>;
  }
  const dayEnd = dayStartMs + DAY_MS;
  return (
    <svg width="100%" height={HEIGHT} viewBox={`0 0 ${WIDTH} ${HEIGHT}`} preserveAspectRatio="none"
         role="img" aria-label="24h 活动时间线" style={{ display: "block", background: "#f1f3f5", borderRadius: 4 }}>
      {segments.map((s) => {
        const start = Math.max(s.startAt, dayStartMs);
        const end = Math.min(s.endAt, dayEnd);
        const w = Math.max(((end - start) / DAY_MS) * WIDTH, 1);
        return (
          <rect key={s.id} data-id={s.id} x={((start - dayStartMs) / DAY_MS) * WIDTH} y={4}
                width={w} height={HEIGHT - 8} fill={colorForCategory(s.category)}
                style={{ cursor: "pointer" }} onClick={() => onSelect(s)}>
            <title>{`${new Date(s.startAt).toLocaleTimeString()} – ${new Date(s.endAt).toLocaleTimeString()} · ${s.category}`}</title>
          </rect>
        );
      })}
    </svg>
  );
}
```

- [ ] **Step 4: 实现 `GranularityPicker.tsx`**

```tsx
import { GRANULARITIES } from "../lib/bucket";

interface Props {
  value: number;
  onChange: (m: number) => void;
}

export default function GranularityPicker({ value, onChange }: Props) {
  return (
    <div role="group" aria-label="粒度" style={{ display: "flex", gap: 4 }}>
      {GRANULARITIES.map((m) => (
        <button key={m} onClick={() => onChange(m)}
                aria-pressed={value === m}
                style={{
                  fontWeight: value === m ? 700 : 400,
                  background: value === m ? "#4c8dff" : undefined,
                  color: value === m ? "#fff" : undefined,
                }}>
          {m}分
        </button>
      ))}
    </div>
  );
}
```

- [ ] **Step 5: 实现 `DaySummary.tsx`**

```tsx
import { summarize } from "../lib/bucket";
import { colorForCategory } from "./SegmentTimeline";
import type { ActivitySegment } from "../types";

export default function DaySummary({ segments, dayStartMs }: { segments: ActivitySegment[]; dayStartMs: number }) {
  const rows = summarize(segments, dayStartMs);
  if (rows.length === 0) return null;
  return (
    <section aria-label="当日汇总" style={{ marginTop: 16 }}>
      <h2 style={{ fontSize: 14, margin: "0 0 8px" }}>当日汇总</h2>
      {rows.map((r) => (
        <div key={r.category} style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 4 }}>
          <span style={{ width: 110, color: "#555" }}>{r.category}</span>
          <div style={{ flex: 1, height: 10, background: "#eceff1", borderRadius: 5, overflow: "hidden" }}>
            <div style={{ width: `${Math.round(r.ratio * 100)}%`, height: "100%", background: colorForCategory(r.category) }} />
          </div>
          <span style={{ width: 90, textAlign: "right", color: "#666" }}>
            {formatDuration(r.durationMs)}
          </span>
        </div>
      ))}
    </section>
  );
}

export function formatDuration(ms: number): string {
  const m = Math.round(ms / 60_000);
  if (m < 60) return `${m} 分`;
  const h = Math.floor(m / 60);
  const rem = m % 60;
  return rem ? `${h} 时 ${rem} 分` : `${h} 时`;
}
```

- [ ] **Step 6: 改 `EventDetail.tsx` 支持 segment**

新增一个 `SegmentDetail` 组件（可放同文件）：显示起止时间、应用、类别、置信度、evidence 数量。

- [ ] **Step 7: 改 `App.tsx` 串起来**

用 `getSegments` 取代 `getEvents`，加 `granularity` state（默认 `DEFAULT_GRANULARITY`），
渲染 `SegmentTimeline` + `GranularityPicker` + `DaySummary`。保留事件时间线作为第二行（可选）。

- [ ] **Step 8: 运行全部前端测试**

Run: `pnpm test`
Expected: 全绿（原 24 + 新 6 + bucket 16）。

- [ ] **Step 9: 提交**

```bash
git add src/
git commit -m "feat(frontend): category-colored segment timeline with granularity picker and day summary"
```

---

## Task 11: 边界检查、端到端与文档

**Files:**
- Create: `src-tauri/crates/engine/tests/boundaries.rs`（**边界检查测试**）
- Create: `docs/superpowers/plans/2026-10-01-phase1-engine-verification.md`
- Modify: `README.md`、`docs/superpowers/STATUS.md`

- [ ] **Step 1: 写 engine 边界检查测试**

```rust
//! spec §4 的硬边界：engine 是纯库。
//!
//! 这不是"检查代码风格"，而是防止未来某次改动悄悄把 IO 引进引擎——
//! 一旦 engine 开始读文件或连数据库，可重放（spec §7.4）和独立测试就没了。

#[test]
fn engine_crate_does_not_depend_on_io_or_tauri() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).expect("read Cargo.toml");

    for forbidden in ["tauri", "rusqlite", "activity-storage", "std::fs", "reqwest"] {
        assert!(
            !text.contains(forbidden),
            "engine 的 Cargo.toml 不该出现 {forbidden}（spec §4：engine 是纯库）"
        );
    }
}

#[test]
fn engine_source_never_mentions_io() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() { walk(&p, out); }
            else if p.extension().map(|x| x == "rs").unwrap_or(false) { out.push(p); }
        }
    }
    let mut files = Vec::new();
    walk(&src, &mut files);
    for f in files {
        let t = std::fs::read_to_string(&f).unwrap();
        for bad in ["std::fs::", "std::net::", "std::process::Command"] {
            assert!(!t.contains(bad), "{} 里出现了 {bad}（engine 不该做 IO）", f.display());
        }
    }
}
```

- [ ] **Step 2: 运行测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p activity-engine --test boundaries`
Expected: PASS。若 FAIL，说明 Task 1–5 引入了 IO，必须回退。

- [ ] **Step 3: 全量测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --workspace && pnpm test && pnpm build`
Expected: 全绿。

- [ ] **Step 4: 真实运行验证**

```bash
pnpm tauri dev
```

- [ ] 开 1 小时，期间切换若干程序
- [ ] 时间线出现**多种颜色**（work / browsing / communication），不再是单一蓝色
- [ ] 点段看详情：起止时间、应用、类别、置信度、evidence 条数
- [ ] 切粒度 10/30/60/120，形状随分桶变化
- [ ] 当日汇总各 category 时长之和 ≈ 有效时长
- [ ] 检查 `%APPDATA%/time-scope/rules.toml` 已生成，改一条规则重启看颜色变化

- [ ] **Step 5: 写验证清单**

`docs/superpowers/plans/2026-10-01-phase1-engine-verification.md`，把上面第 4 步展开成带勾选框的清单，
并列出"已自动验证"的部分（全量测试、engine 边界检查、重放幂等）。

- [ ] **Step 6: 更新 README 与 STATUS**

README 的"还没做什么"删掉 engine、分类、分桶、汇总；
STATUS.md 更新为"Phase 1 第二步完成"。

- [ ] **Step 7: 提交**

```bash
git add -A
git commit -m "test(engine): assert the pure-library boundary; docs: engine verification checklist"
```

---

## Self-Review

**1. Spec 覆盖：**

| Spec 章节 | 覆盖 task | 状态 |
|---|---|---|
| §4 代码结构（engine 纯库约束） | Task 1、Task 11（边界测试强制） | ✅ |
| §6 ActivitySegment / Category | Task 1 | ✅ |
| §7.1 Context Builder | Task 4 | ✅ |
| §7.2 Rule Classifier + TOML 外置 + 默认拷贝 | Task 3、Task 7 | ✅ |
| §7.3 Segmenter 状态机（切段/grace/idle/resume） | Task 5 | ✅ |
| §7.3 min_segment_duration | Task 5（pending 缓冲） | ✅ |
| §7.4 可重放（无副作用、classifier 版本） | Task 5（纯函数测试 + 重放不同结果）、Task 8（bootstrap 幂等） | ✅ |
| §8 activities / activity_evidence 写入 | Task 6 | ✅ |
| §8.2 前端分桶 | Task 9 | ✅ |
| §9 `get_segments` command | Task 8 | ✅ |
| §9 前端聚合汇总 | Task 9、Task 10 | ✅ |
| §10 分类着色 / 粒度切换 / 汇总 / hover 详情 | Task 10 | ✅ |
| §13 测试策略（engine 表驱动单测） | Task 1–5 | ✅ |
| §11 脱敏 | **未覆盖** | ⚠️ 见下 |
| §12 托盘/自启/单实例 | **未覆盖** | ⚠️ 见下 |
| §9 `segment-updated` 推送 | **未覆盖** | ⚠️ 见下 |

**本计划明确不覆盖：**

- **窗口标题脱敏（§11）** — 标题仍明文落库。骨架阶段可接受，**导出数据前必须先做**。
- **`segment-updated` 事件（§9）** — 本计划让前端在日期切换时整表重查。实时性差一点，
  但对"看今天干了什么"够用。推送留给后续。
- **托盘/自启/单实例（§12）** — 属 Phase 1 第三步"打磨"。
- **SessionLock/Unlock 采集（§5.3）** — 仍只在 `EventType` 里预留。

**2. 占位符扫描：** 无 TBD/TODO。Task 8 Step 5 给了两种 engine 接线方式（改 collector 签名
vs 开 engine 线程），推荐后者；这是**实现选择**而非未决问题。

**3. 类型一致性：**

| 类型 / 函数 | 定义 | 使用 |
|---|---|---|
| `Category` | Task 1 | Task 3/5/6/9/10 |
| `ActivitySegment`（Rust） | Task 1 | Task 5/6/8 |
| `ActivityContext::same_app_and_category` | Task 1 | Task 5 |
| `EngineConfig` | Task 2 | Task 5/8 |
| `RuleSet::from_toml` / `classify` | Task 3 | Task 5/7/8 |
| `Classification::unknown` | Task 3 | Task 3 内部 |
| `context_of` / `ContextBuilder` | Task 4 | Task 5 |
| `reduce` / `EngineState` / `EngineOutput` | Task 5 | Task 8 |
| `open_segment_snapshot` | Task 5 | Task 8 |
| `StoredSegment`（camelCase） | Task 6 | Task 8 |
| `ActivitySegment`（TS, camelCase） | Task 9 | Task 9/10 |

**4. Review Focus 覆盖：**

| # | 关注点 | 测试 |
|---|---|---|
| 1 | 段时长不为负 | `segment_end_is_always_at_or_after_start`、`segments_are_ordered_and_non_overlapping` |
| 2 | 只有 1 条 Event / 1 次切换 | `single_event_creates_one_segment`、`zero_grace_disables_absorption` |
| 3 | 进程名大小写 | `process_match_is_case_insensitive`（含 `code.exe`/`CODE.EXE`/`CoDe.ExE`） |
| 4 | min_segment 与落库不可回溯 | `short_segments_stay_pending_until_min_duration_elapses`、`pending_segments_are_released_once_old_enough` |
| 5 | 规则文件坏掉 | `broken_user_file_falls_back_to_defaults_without_crashing`、`broken_file_is_not_overwritten` |

**已修正的计划缺陷（实施中实际触发，已同步回上文代码块）：**

- **grace 代理判定是错的。** 初稿用 `returns_to_previous`（= "最近切过段"）代理
  spec §7.3 的"新 context 与再前一个相同"，并只做"不切段"。实测
  `Code -> chrome(2s) -> Code` 会留下一个 **chrome** 段，而用户实际全程在 Code。
  已改为：吸收时**丢弃**一闪而过的段、**复活** pending 里的前一段。
- **`previous_application` 必须独立于 `pending`。** 初稿想拿 `pending` 末尾当"再前一个"，
  但 `min_segment_duration = 0` 时 pending 立即被清空，grace 判定永远失效。
  已加 `EngineState::previous_application`。
- **上下文不能每事件重建。** 初稿用 `context_of()` 从空 context 起算，
  而心跳事件不携带 `application`，会把它冲成 `None` → "同一应用"判定失败 →
  **每来一个心跳就切一段**。已改为 clone `current_context` 再 apply 事件。
- **落库门槛应是 `max(min_segment_duration, grace_period)`。** 吸收只能复活未落库的段，
  门槛必须覆盖整个 grace 窗口。这也修掉了 spec 自身默认值
  （`min 30s < grace 60s`）导致的吸收不可靠。
- **`min_segment_duration` 当时没实现"合并"。** 裁定 A 只把落库往后推，
  spec §7.3 的"并入相邻段"实际由新写的 `fold_short_segments` 才实现（裁定 E）。
  真实运行里跑出了一个 64ms 的 `explorer.exe` 段才暴露这一点。
- **无应用的事件不该开段。** 应用启动后第一个事件通常是心跳，心跳不携带
  `application`，之前会凭空开一个 "unknown / 无应用" 的幽灵段。

**仍未验证的假设：**

- `insert_segments` 里 `evidence_for` 的嵌套借用可能触发 borrow checker 冲突，
  报错时把外层 `stmt` 提前 drop（Task 6）。
- Task 5–8 的时间断言都建立在"Event 按时间升序送达"之上。
  collector 的 `mpsc::channel` 是 FIFO，实际满足；但如果将来有人改成多线程并发 ingest，
  Task 5 的时间不变量测试会失败——那正是这些测试的价值。

---

## 执行结果（2026-10-01）

11 个 task 全部完成。实施中推翻了本计划的四处设计，已同步回上文：
裁定 C（grace 吸收要复活前一段）、裁定 D（落库门槛取 max）、裁定 E（短段合并）、
以及"上下文沿用而非重建"。

```
Rust  151 passed / 0 failed / 0 warning
前端  57 passed（6 文件）· pnpm build 通过
```

人工验证清单：`2026-10-01-phase1-engine-verification.md`

## 执行交接

计划已实施完毕，文档与实际代码同步。上述"执行结果"一节记录了实施中推翻了本计划的哪些设计。
