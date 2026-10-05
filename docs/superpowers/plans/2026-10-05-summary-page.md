# 汇总页 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 侧边栏新增「汇总」页，提供跨日/跨周/跨月的指标、GitHub 式监控热力图、范围下钻与窗口标题排名。

**Architecture:** 三个 Tauri command 按数据成本分层——`get_daily_calendar`（无参数，供热力图，只取一次）、`get_summary`（范围指标，随选中变）、`get_top_titles`（贵层，懒加载）。热力图是导航器：固定渲染全部数据，点击只选出一个范围（跨格整块矩形框出），顶部指标随之重算。全部聚合走 SQL `GROUP BY` 与 Rust 侧遍历，不引入图表库、不改 schema。

**Tech Stack:** Tauri 2 / Rust 1.96（workspace，4 crates）/ React 19 / TypeScript / Tailwind v4 / lucide-react / Vitest / rusqlite 0.32

**Spec:** [`docs/superpowers/specs/2026-10-05-summary-page-design.md`](../specs/2026-10-05-summary-page-design.md)

---

## Global Constraints

- **不引入任何新依赖。** `package.json` 的 `dependencies` 与 `devDependencies` 一行不加。不引入 d3（spec §1.2、§10）。
- **不改 schema。** `SCHEMA_VERSION` 保持 1，不加列、不加表、不加迁移（spec §1.2、§3.4）。
- **不新增任何颜色 token。** 全部引用 `src/styles/theme.css` 已有变量（spec §7.2）。热力图/24h 条用 `--color-scale-1..5`（**紫色阶**），0 档用 `--color-surface-2`。**蓝相是类别色**（`--color-cat-*`），绝不用来画连续量。
- **`cargo test` 必须带 `--workspace`**，否则只跑根包、0 个测试、显示绿色（STATUS §7.2）。
- **验证命令**：`cargo test --manifest-path src-tauri/Cargo.toml --workspace` / `pnpm test` / `pnpm build` / `node design-system/time-scope/verify-palette.mjs`（exit 0 才算过）。
- **注释用中文**，解释「为什么」而不是「做了什么」，与 `theme.css` / `segmenter.rs` / `day_replay.rs` 现有风格一致。
- **热力图口径**：7 行 = 周日→周六（`row 0` 是**周日**），1 列 = 1 周（自周日起），首列往前补齐到周日，末列往后补到周六（spec §5.1）。
- **热力图不随选中范围变化**，永远渲染 `get_daily_calendar()` 的返回值（spec §2.3、§7.3）。
- **选中框是跨格的一整块绝对定位矩形**，不是逐格描边；框外**不压暗**（spec §5.3）。
- **窗口标题归一化在 Rust 侧**，四条规则硬编码，不做配置（spec §4.1）。
- **`storage` crate 不引入 `time` 依赖**；需要本地小时时传 `offset_secs: i32` 做算术。
- 提交信息末尾带 `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`。

---

## Review Focus

以下是 spec 暗示、但没有任何一条任务测试会覆盖的输入/失败模式。按「最可能先咬人」排序。**每一条的测试写进拥有那段代码的任务里。**

1. **空库**（应用装完还没跑满一天，或 `day_replay` 尚未产出任何段）——`get_daily_calendar` 无行可返回。汇总页必须显示一个说得过去的空状态，不能崩、不能画出一面全是 `--color-surface-2` 的假墙、不能出现 `NaN` 或 `0/0`。**这是新装用户看到的第一屏。**
2. **只有 1 天数据**——`buildWall` 的补齐逻辑会把它展开成 7 列 1 行有数据、6 列全透明。看起来像坏了。
3. **`from > to` 的非法范围**——界面上点不出这种范围，但 IPC 是公开的，直接调会炸或返回垃圾。
4. **归一化后变成空串的标题**（`"和另外 31 个页面"` 剥完什么都不剩）——必须跳过，不能排进 Top 10 占一个坑。
5. **`无标题` / `New Tab` 归一化后仍在，且占了大头**——这是 spec §4.2 记录的已知限制，**不是 bug**。但要有一条测试把它们如实排出，防止将来有人「顺手」加个过滤把它们藏起来。

---

## File Structure

**新增（前端）**

| 文件 | 职责 |
|---|---|
| `src/lib/summary.ts` | 纯函数：补齐日期序列、色阶分档、热力图布局、选中框计算、范围推导 |
| `src/lib/summary.test.ts` | 上面这些纯函数的测试 |
| `src/views/SummaryPage.tsx` | 范围状态 + 三段布局 + 两个数据 hook |
| `src/components/MetricRow.tsx` | 顶部指标横条（4 张卡，其中一张是圆环） |
| `src/components/DonutChart.tsx` | 圆环（4 类，手写 SVG）+ 图例 |
| `src/components/ContributionWall.tsx` | 热力图：7×N 网格、月标签、周条、跨格选中框 |
| `src/components/DayPartChart.tsx` | 24h 细柱条 |
| `src/components/RankedList.tsx` | 排名列表（应用 Top 与标题排名共用） |
| `src/components/*.test.tsx` | 上述组件各自的测试 |

**新增（Rust）**

| 文件 | 职责 |
|---|---|
| `src-tauri/src/title_norm.rs` | 窗口标题归一化（纯函数，无 IO） |
| `src-tauri/crates/storage/src/summary.rs` | 三个聚合查询 + `DailyCalendar` / `Summary` 数据结构 |
| `src-tauri/crates/storage/tests/summary.rs` | 上面三个查询的集成测试 |

**修改**

| 文件 | 改什么 |
|---|---|
| `src-tauri/crates/storage/src/lib.rs` | `pub mod summary;` + re-export |
| `src-tauri/src/lib.rs` | 三个 `#[tauri::command]` + `generate_handler!` 注册 |
| `src/types.ts` | 三个 IPC 封装 + 对应 TS 类型 |
| `src/components/Sidebar.tsx` | `View` 加 `"summary"` + 一个 `SidebarItem` |
| `src/components/Sidebar.test.tsx` | 按钮数断言 3 → 4 |
| `src/App.tsx` | `view` 状态 + 渲染分支 |

---

## Task 1: 窗口标题归一化

最独立的一块：纯函数、无 IO、无依赖，先做它。

**Files:**
- Create: `src-tauri/src/title_norm.rs`
- Modify: `src-tauri/src/lib.rs`（加 `mod title_norm;`）

**Interfaces:**
- Consumes: 无
- Produces: `pub fn normalize_title(raw: &str) -> Option<String>` —— 剥壳后的标题；剥空返回 `None`。Task 5 调用。

- [ ] **Step 1: 写失败的测试**

`src-tauri/src/title_norm.rs` 末尾加内联测试（与 `titles.rs` / `date_range.rs` 同样的 `#[cfg(test)] mod tests` 风格）：

```rust
#[cfg(test)]
mod tests {
    use super::normalize_title;

    fn n(s: &str) -> String {
        normalize_title(s).expect("应归一化出非空结果")
    }

    #[test]
    fn edge_tab_group_counter_is_stripped() {
        assert_eq!(n("无标题 和另外 31 个页面 - 个人 - Microsoft Edge"), "无标题");
    }

    #[test]
    fn edge_new_tab_counter_is_stripped() {
        assert_eq!(n("New Tab 和另外 33 个页面 - 个人 - Microsoft Edge"), "New Tab");
    }

    #[test]
    fn leading_status_symbol_is_stripped() {
        assert_eq!(n("◐ 项目与 uv Python 管理"), "项目与 uv Python 管理");
        assert_eq!(n("◑ 项目与 uv Python 管理"), "项目与 uv Python 管理");
    }

    #[test]
    fn real_code_title_passes_through_unchanged() {
        // 最关键的一条：规则只吃前导状态符和尾部配置名，中间的 " - " 不许碰。
        assert_eq!(n("main.rs - Visual Studio Code"), "main.rs - Visual Studio Code");
        assert_eq!(n("registry.ts - time-scope - Visual Studio Code"), "registry.ts - time-scope - Visual Studio Code");
    }

    #[test]
    fn middle_status_symbol_is_kept() {
        // 规则 2 只吃前导。中间的吃不到是有意的（spec §4.2），
        // 这条测试守着「不要为了多剥一点而放宽锚点」。
        assert_eq!(n("⏳ 待处理 · 了解该项目概况"), "⏳ 待处理 · 了解该项目概况");
    }

    #[test]
    fn blank_and_whitespace_return_none() {
        assert_eq!(normalize_title(""), None);
        assert_eq!(normalize_title("   "), None);
        assert_eq!(normalize_title("\t\n"), None);
    }

    #[test]
    fn title_that_becomes_empty_returns_none() {
        // 剥完什么都不剩：不能返回空串占掉 Top 10 的一个坑。
        assert_eq!(normalize_title("和另外 31 个页面"), None);
    }

    #[test]
    fn redacted_placeholder_survives_normalization() {
        // 脱敏标记必须在归一化后还在，否则 `redacted` 判定会失效
        // （spec §3.3 要求复用 titles.rs 的现成判定）。
        let out = normalize_title("客户 42 号项目 - Visual Studio Code - [redacted]").unwrap();
        assert!(out.contains("[redacted]"), "占位符必须原样保留，实际得到：{out}");
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace title_norm
```

Expected: 编译失败，`cannot find function normalize_title`（或 `cannot find value` in this scope）。

- [ ] **Step 3: 写实现**

`src-tauri/src/title_norm.rs` 的非测试部分：

```rust
//! 窗口标题归一化。
//!
//! 原始标题里混着大量「同一个东西的不同写法」：Edge 的折叠标签组标题带计数器
//! （`和另外 31 个页面`），计数器每变一次就算一条新标题；uvicorn 的标题带状态符
//! （`◐` / `◑`）。直接按原样排名，Top N 会被这些计数器占满——
//! 实测 2026-10-05，Top 6 全部是 Edge 计数器变体（spec §4）。
//!
//! 这里只做**剥壳**，不改语义：真实标题（`main.rs - Visual Studio Code`）
//! 必须原样通过。剥不掉的东西（`无标题`、`New Tab`、标题中间的 `⏳`）如实留着，
//! 见 spec §4.2。

use regex::Regex;
use std::sync::LazyLock;

/// 剥掉 Edge 折叠标签组的计数器：`和另外 31 个页面`
static TAB_GROUP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"和另外\s*\d+\s*个页面").expect("编译期常量正则"));
/// 剥掉前导状态符。**只吃前导**——标题中间的 `⏳` 不动，
/// 因为把锚点放宽会连带吃掉 `main.rs - Visual Studio Code` 的开头。
static LEADING_SYMBOL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\s\u{25D0}\u{25D1}\u{25D2}\u{25D3}\u{25CF}\u{25CB}\u{25C9}\u{2B50}*+!\-]+").expect("编译期常量正则"));
/// 剥掉 Edge 标题尾部的配置文件名：` - 个人` / ` - 工作` / ` - 私人`
static EDGE_PROFILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*-\s*(个人|工作|私人)\s*$").expect("编译期常量正则"));
/// 连续空白压成一个
static WHITESPACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+").expect("编译期常量正则"));

/// 归一化一条窗口标题。返回 `None` 表示「剥完没有内容」，调用方应跳过它。
///
/// 规则按顺序应用（spec §4.1），顺序不能换：先剥计数器再剥前导符，
/// 否则 Edge 标题里 `和另外` 前面的空白会先被前导规则吃掉一部分。
pub fn normalize_title(raw: &str) -> Option<String> {
    let s = TAB_GROUP.replace_all(raw, "");
    let s = LEADING_SYMBOL.replace_all(&s, "");
    let s = EDGE_PROFILE.replace_all(&s, "");
    let s = WHITESPACE.replace_all(&s, " ");
    let trimmed = s.trim().trim_matches(['-', '·', ':', ' ']);
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}
```

在 `src-tauri/src/lib.rs` 的模块声明区加一行 `mod title_norm;`（与 `mod titles;` 并列）。

- [ ] **Step 4: 运行测试确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace title_norm
```

Expected: 8 passed。

若 `middle_status_symbol_is_kept` 失败，检查 `LEADING_SYMBOL` 的 `^` 锚点是否还在。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/title_norm.rs src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat: normalize window titles so rankings aren't counter noise

Edge 的折叠标签组标题带计数器（`和另外 31 个页面`），计数器每变一次
就算一条新标题；uvicorn 带 `◐`/`◑` 状态符。实测 Top 6 全是这些变体，
排名统计的是「计数器变过几次」而不是「你在看什么」。

四条剥壳规则，只吃前导状态符与尾部配置名，真实标题原样通过。
剥空返回 None，调用方跳过——空串会占掉 Top 10 的坑。

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: `get_daily_calendar` — 热力图的唯一数据源

**Files:**
- Create: `src-tauri/crates/storage/src/summary.rs`
- Create: `src-tauri/crates/storage/tests/summary.rs`
- Modify: `src-tauri/crates/storage/src/lib.rs`（`pub mod summary;`）

**Interfaces:**
- Consumes: `crate::open_in_memory`（测试用）
- Produces:
  ```rust
  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct DayCell { pub date: String, pub total_ms: i64 }

  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct DailyCalendar { pub first: String, pub last: String, pub days: Vec<DayCell> }

  pub fn daily_calendar(conn: &Connection) -> Result<Option<DailyCalendar>, rusqlite::Error>
  ```
  Task 6 调用 `daily_calendar`。

**注意**：`days` **只含有记录的日期**（不补空缺）。补齐是前端 `src/lib/summary.ts` 的 `fillDays` 的事（Task 8）——`GROUP BY` 会跳过没开机的那天，直接用它的结果排格子会整片错位。

- [ ] **Step 1: 写失败的测试**

`src-tauri/crates/storage/tests/summary.rs`：

```rust
use activity_storage::{daily_calendar, insert_segments, open_in_memory, StoredSegment};

fn seg(id: &str, start_ms: i64, end_ms: i64) -> StoredSegment {
    StoredSegment {
        id: id.into(),
        start_at: start_ms,
        end_at: end_ms,
        category: "work".into(),
        application: Some("Code.exe".into()),
        confidence: 1.0,
        classifier: "rule".into(),
        classifier_version: "1".into(),
        evidence_event_ids: Vec::new(),
    }
}

fn put(conn: &rusqlite::Connection, segs: Vec<StoredSegment>) {
    let refs: Vec<(StoredSegment, Vec<String>)> =
        segs.into_iter().map(|s| (s, Vec::new())).collect();
    insert_segments(conn, &refs).unwrap();
}

/// 某个 Unix 毫秒所在**本地日**的零点。借 SQLite 自己换算，
/// 避免测试里复刻时区 / 夏令时逻辑（spec §3.4 说那个口径是已知的近似）。
fn local_midnight_ms(conn: &rusqlite::Connection, ms: i64) -> i64 {
    conn.query_row(
        "SELECT CAST(strftime('%s', datetime(?/1000, 'unixepoch', 'localtime', 'start of day')) AS INTEGER) * 1000",
        rusqlite::params![ms],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
}

/// 三个连续的本地午夜：d0、d0+1天、d0+2天。
fn three_days(conn: &rusqlite::Connection) -> (i64, i64, i64) {
    let now_ms: i64 = conn
        .query_row("SELECT CAST(strftime('%s','now') AS INTEGER) * 1000", [], |r| r.get(0))
        .unwrap();
    let d0 = local_midnight_ms(conn, now_ms);
    (d0, d0 + 86_400_000, d0 + 2 * 86_400_000)
}

#[test]
fn empty_db_yields_none() {
    // Review Focus #1：空库是「刚装完还没跑满一天」的真实状态。
    // 必须返回 None，而不是 first/last 为空串、days 为空的半成品。
    let conn = open_in_memory();
    assert_eq!(daily_calendar(&conn).unwrap(), None, "空库应返回 None");
}

#[test]
fn single_day_is_returned() {
    let conn = open_in_memory();
    let (d0, _, _) = three_days(&conn);
    put(&conn, vec![seg("a", d0 + 3_600_000, d0 + 7_200_000)]);
    let cal = daily_calendar(&conn).unwrap().expect("有段就该有日历");
    assert_eq!(cal.days.len(), 1);
    assert_eq!(cal.days[0].total_ms, 3_600_000);
    assert_eq!(cal.first, cal.last);
    assert_eq!(cal.first, cal.days[0].date);
}

#[test]
fn same_day_segments_are_merged_into_one_row() {
    let conn = open_in_memory();
    let (d0, d1, _) = three_days(&conn);
    put(&conn, vec![
        seg("a", d0 + 3_600_000, d0 + 7_200_000),   // 第 1 天，1 小时
        seg("b", d0 + 7_300_000, d0 + 7_600_000),   // 同一天，再 5 分钟
        seg("c", d1 + 1_000, d1 + 61_000),          // 第 2 天，1 分钟
    ]);
    let cal = daily_calendar(&conn).unwrap().unwrap();
    assert_eq!(cal.days.len(), 2, "同一天的段必须合并成一行");
    assert_eq!(cal.days[0].total_ms, 3_600_000 + 300_000);
    assert_eq!(cal.days[1].total_ms, 60_000);
    assert!(cal.days[0].date < cal.days[1].date, "必须按日期升序");
}

#[test]
fn days_with_no_segments_are_absent() {
    // 中间那天没段 -> SQL 不返回它。补齐是前端 fillDays 的活。
    let conn = open_in_memory();
    let (d0, _, d2) = three_days(&conn);
    put(&conn, vec![
        seg("a", d0 + 3_600_000, d0 + 7_200_000),
        seg("c", d2 + 3_600_000, d2 + 7_200_000),
    ]);
    let cal = daily_calendar(&conn).unwrap().unwrap();
    assert_eq!(cal.days.len(), 2, "中间那天没段就不该出现在 days 里");
}

#[test]
fn segments_are_bucketed_by_start_at_not_by_overlap() {
    // 跨零点的段整体算在**开始**那天。这与 get_segments_in_range 的
    // `start_at >= ?1 AND start_at < ?2` 口径一致 —— spec §7.3 要求两者一致，
    // 这条测试把口径钉死，防止有人只把其中一个改成区间相交。
    let conn = open_in_memory();
    let (d0, _, _) = three_days(&conn);
    let start = d0 + 86_700_000;   // 当天 23:50
    let end = d0 + 90_000_000;     // 次日 00:10
    put(&conn, vec![seg("cross", start, end)]);
    let cal = daily_calendar(&conn).unwrap().unwrap();
    assert_eq!(cal.days.len(), 1, "跨零点段只落在开始那天");
    assert_eq!(cal.days[0].total_ms, end - start, "整段时长算给开始那天");
}
```

- [ ] **Step 2: 运行确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace daily_calendar
```

Expected: 编译失败，`cannot find function daily_calendar`。

- [ ] **Step 3: 写实现**

`src-tauri/crates/storage/src/summary.rs`：

```rust
//! 汇总页的聚合查询。
//!
//! 全部聚合都在这里，不散落到 app 层——app 层只做时区换算和 JSON 序列化。
//!
//! **按天分组用 SQL 的 `localtime`**，这依赖进程时区，且在夏令时切换日
//! 会差一小时。全项目已经建立在「单个固定 offset」的假设上
//! （`date_range::day_range_ms` 同款），单独给汇总页做 DST 正确的分组
//! 会制造「只有汇总页对、其他页错」的不一致。见 spec §3.4 与 §9 第 1 条。

use crate::activity::StoredSegment;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// 热力图的一格：某一天的监控总时长（**含 idle**）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayCell {
    /// 本地日 `YYYY-MM-DD`
    pub date: String,
    pub total_ms: i64,
}

/// 热力图的数据源。**`days` 只含有记录的日期**——补齐空缺是前端的活，
/// 因为 `GROUP BY` 会跳过没开机的那天，直接用它的结果排格子会整片错位。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyCalendar {
    /// 首个有记录的日子
    pub first: String,
    /// 最后一天（通常是今天）
    pub last: String,
    pub days: Vec<DayCell>,
}

/// 取全部数据的逐日监控时长。**无参数**——热力图永远渲染全部数据，
/// 不受选中范围影响（spec §2.3）。
///
/// 库为空时返回 `Ok(None)`：那是「刚装完还没跑满一天」的真实状态，
/// 调用方据此显示空状态，而不是拿到一个 first/last 为空串的半成品。
pub fn daily_calendar(conn: &Connection) -> Result<Option<DailyCalendar>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT date(start_at/1000,'unixepoch','localtime') AS d,
                SUM(end_at - start_at) AS ms
         FROM activities
         GROUP BY d
         ORDER BY d ASC",
    )?;
    let mut days = Vec::new();
    let mut rows = stmt.query_map([], |row| {
        Ok(DayCell {
            date: row.get(0)?,
            total_ms: row.get::<_, i64>(1)?,
        })
    })?;
    while let Some(r) = rows.next()? {
        days.push(r?);
    }
    let Some(first) = days.first() else {
        return Ok(None);
    };
    let last = days
        .last()
        .map(|d| d.date.clone())
        .unwrap_or_default();
    Ok(Some(DailyCalendar {
        first: first.date.clone(),
        last,
        days,
    }))
}
```

在 `src-tauri/crates/storage/src/lib.rs` 顶部加 `pub mod summary;`，并加 re-export：

```rust
pub use summary::{daily_calendar, DailyCalendar, DayCell};
```

- [ ] **Step 4: 运行确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace daily_calendar
```

Expected: 5 passed。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/crates/storage/src/summary.rs src-tauri/crates/storage/src/lib.rs src-tauri/crates/storage/tests/summary.rs
git commit -m "$(cat <<'EOF'
feat: per-day totals for the heatmap

get_daily_calendar 无参数，只按 start_at 的本地日分组 —— 口径与
get_segments_in_range 的 `start_at >= ? AND start_at < ?` 一致，
跨零点的段在热力图和 Summary 里都整体算在开始那天。

days 只含有记录的日期：补齐交给前端，GROUP BY 会跳过没开机的那天。

空库返回 None 而不是半成品，那是「刚装完」的真实状态。

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: `range_totals` — SQL 能一次算完的那部分

`Summary` 拆成两个函数，各自职责单一、各自可独立测试。app 层（Task 6）把两者并成 `Summary`。

**Files:**
- Modify: `src-tauri/crates/storage/src/summary.rs`
- Modify: `src-tauri/crates/storage/tests/summary.rs`

**Interfaces:**
- Consumes: Task 2 的 `summary.rs` 文件
- Produces:
  ```rust
  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct RangeTotals {
      pub total_ms: i64,
      pub active_ms: i64,
      pub idle_ms: i64,
      pub segment_count: i64,
      pub donut: Vec<DonutSlice>,           // 恒为 4 项，顺序固定
      pub top_apps: Vec<AppSlice>,          // 最多 5 项，按 ms 降序
  }
  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct DonutSlice { pub key: String, pub ms: i64 }
  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct AppSlice { pub name: String, pub ms: i64 }

  pub fn range_totals(conn: &Connection, start_ms: i64, end_ms: i64)
      -> Result<RangeTotals, rusqlite::Error>
  ```

**关于「空范围」**：`from > to` 时 SQL 自然返回零行，函数返回全 0 的结构体（**不返回 `Err`、不返回 `None`**）。公开的 IPC 可能收到这种输入，让它炸没有好处。见 Review Focus #3。

- [ ] **Step 1: 写失败的测试**

追加到 `src-tauri/crates/storage/tests/summary.rs`：

```rust
fn segc(id: &str, start_ms: i64, end_ms: i64, category: &str, app: Option<&str>) -> StoredSegment {
    StoredSegment {
        id: id.into(),
        start_at: start_ms,
        end_at: end_ms,
        category: category.into(),
        application: app.map(|a| a.into()),
        confidence: 1.0,
        classifier: "rule".into(),
        classifier_version: "1".into(),
        evidence_event_ids: Vec::new(),
    }
}

#[test]
fn totals_split_active_and_idle() {
    let conn = open_in_memory();
    put(&conn, vec![
        segc("a", 0, 3_600_000, "work", Some("Code.exe")),
        segc("b", 3_600_000, 5_400_000, "idle", None),
    ]);
    let t = range_totals(&conn, 0, 10_000_000).unwrap();
    assert_eq!(t.total_ms, 5_400_000);
    assert_eq!(t.active_ms, 3_600_000);
    assert_eq!(t.idle_ms, 1_800_000);
    assert_eq!(t.segment_count, 2);
}

#[test]
fn donut_always_has_exactly_four_slices_in_fixed_order() {
    // 前端按数组下标取色，顺序不能随数据变。
    let conn = open_in_memory();
    let t = range_totals(&conn, 0, 10_000_000).unwrap();
    let keys: Vec<&str> = t.donut.iter().map(|d| d.key.as_str()).collect();
    assert_eq!(keys, vec!["work", "browsing", "idle", "unknown"]);
    assert!(t.donut.iter().all(|d| d.ms == 0), "空范围时四档都该是 0");
}

#[test]
fn donut_folds_four_categories_into_unknown() {
    // spec §6：学习/娱乐/社交/生活 并入「未分类」那一档，环才是完整 360°。
    let conn = open_in_memory();
    put(&conn, vec![
        segc("a", 0, 1_000, "work", Some("Code.exe")),
        segc("b", 2_000, 3_000, "study", Some("Zed.exe")),
        segc("c", 4_000, 5_000, "entertainment", Some("cloudmusic.exe")),
        segc("d", 6_000, 7_000, "communication", Some("WeChat.exe")),
        segc("e", 8_000, 9_000, "life", Some("explorer.exe")),
        segc("f", 10_000, 11_000, "unknown", Some("mstsc.exe")),
    ]);
    let t = range_totals(&conn, 0, 20_000_000).unwrap();
    let by = |k: &str| t.donut.iter().find(|d| d.key == k).unwrap().ms;
    assert_eq!(by("work"), 1_000);
    assert_eq!(by("unknown"), 5_000, "study+entertainment+communication+life+unknown 都要并进来");
    assert_eq!(t.donut.iter().map(|d| d.ms).sum::<i64>(), t.total_ms, "并档不改变总时长");
}

#[test]
fn top_apps_ranked_desc_and_nulls_dropped() {
    let conn = open_in_memory();
    put(&conn, vec![
        segc("a", 0, 1_000, "browsing", Some("msedge.exe")),
        segc("b", 2_000, 12_000, "work", Some("Code.exe")),
        segc("c", 14_000, 17_000, "work", Some("Code.exe")),
        segc("d", 20_000, 30_000, "idle", None),   // 没有 application
    ]);
    let t = range_totals(&conn, 0, 40_000_000).unwrap();
    assert_eq!(t.top_apps.len(), 2, "application IS NULL 的段要丢掉");
    assert_eq!(t.top_apps[0].name, "Code.exe");
    assert_eq!(t.top_apps[0].ms, 13_000);
    assert_eq!(t.top_apps[1].name, "msedge.exe");
}

#[test]
fn empty_range_yields_zeros_not_error() {
    // Review Focus #3：from > to 是公开 IPC 能收到的输入，不许炸。
    let conn = open_in_memory();
    put(&conn, vec![segc("a", 5_000_000, 6_000_000, "work", Some("Code.exe"))]);
    let t = range_totals(&conn, 9_000_000, 1_000_000).unwrap();
    assert_eq!(t.total_ms, 0);
    assert_eq!(t.segment_count, 0);
    assert!(t.top_apps.is_empty());
}

#[test]
fn range_is_half_open() {
    // 终点上的段不计入：与 get_segments_in_range 同一个半开约定。
    let conn = open_in_memory();
    put(&conn, vec![segc("a", 1_000_000, 2_000_000, "work", Some("Code.exe"))]);
    assert_eq!(range_totals(&conn, 0, 2_000_000).unwrap().segment_count, 1,
               "start_at 等于 end 时不计入");
    assert_eq!(range_totals(&conn, 2_000_000, 3_000_000).unwrap().segment_count, 0,
               "start_at 恰好等于 end 时不计入");
}
```

同时把文件头的 import 改成：

```rust
use activity_storage::{daily_calendar, insert_segments, open_in_memory, range_totals, StoredSegment};
```

- [ ] **Step 2: 运行确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace range_totals
```

Expected: 编译失败，`cannot find function range_totals`。

- [ ] **Step 3: 写实现**

追加到 `src-tauri/crates/storage/src/summary.rs`：

```rust
/// 圆环的一档。**只有 4 种 key**，前端的 `DonutChart` 按下标取色，
/// 所以顺序固定为 work / browsing / idle / unknown。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DonutSlice {
    pub key: String,
    pub ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSlice {
    pub name: String,
    pub ms: i64,
}

/// 范围内 SQL 能一次算完的那些指标。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeTotals {
    pub total_ms: i64,
    pub active_ms: i64,
    pub idle_ms: i64,
    pub segment_count: i64,
    pub donut: Vec<DonutSlice>,
    pub top_apps: Vec<AppSlice>,
}

/// 圆环只画 4 档。其余四个类别（学习/娱乐/社交/生活）并入 `unknown` ——
/// spec §6：那样环才是完整 360°，且 `rules.toml` 命中它们时不至于无处可去。
const DONUT_KEYS: [&str; 4] = ["work", "browsing", "idle", "unknown"];

const TOP_APPS_LIMIT: i64 = 5;

/// `[start_ms, end_ms)` 半开区间内的时长三件套、段数、圆环、应用 Top。
///
/// 区间倒置（`start_ms >= end_ms`）不是错误，返回全 0 —— 公开的 IPC
/// 可能收到这种输入，让它炸没有好处。
pub fn range_totals(
    conn: &Connection,
    start_ms: i64,
    end_ms: i64,
) -> Result<RangeTotals, rusqlite::Error> {
    if start_ms >= end_ms {
        return Ok(RangeTotals {
            total_ms: 0,
            active_ms: 0,
            idle_ms: 0,
            segment_count: 0,
            donut: empty_donut(),
            top_apps: Vec::new(),
        });
    }

    let mut totals: Vec<(String, i64)> = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT category, SUM(end_at - start_at) AS ms FROM activities
             WHERE start_at >= ?1 AND start_at < ?2
             GROUP BY category",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![start_ms, end_ms], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        while let Some(r) = rows.next()? {
            totals.push(r?);
        }
    }

    let total_ms: i64 = totals.iter().map(|(_, ms)| ms).sum();
    let idle_ms: i64 = totals.iter().filter(|(c, _)| c == "idle").map(|(_, ms)| ms).sum();

    // 四档恒定存在，缺的补 0 —— 前端不用处理「档位数量会变」。
    let donut: Vec<DonutSlice> = DONUT_KEYS
        .iter()
        .map(|key| DonutSlice {
            key: (*key).to_string(),
            ms: totals
                .iter()
                .filter(|(cat, _)| donut_bucket(cat) == *key)
                .map(|(_, ms)| ms)
                .sum(),
        })
        .collect();

    let top_apps: Vec<AppSlice> = {
        let mut stmt = conn.prepare(
            "SELECT application, SUM(end_at - start_at) AS ms FROM activities
             WHERE start_at >= ?1 AND start_at < ?2 AND application IS NOT NULL
             GROUP BY application ORDER BY ms DESC LIMIT ?3",
        )?;
        let mut rows = stmt.query_map(
            rusqlite::params![start_ms, end_ms, TOP_APPS_LIMIT],
            |r| Ok(AppSlice { name: r.get::<_, String>(0)?, ms: r.get::<_, i64>(1)? }),
        )?;
        let mut v = Vec::new();
        while let Some(r) = rows.next()? {
            v.push(r?);
        }
        v
    };

    let segment_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM activities WHERE start_at >= ?1 AND start_at < ?2",
        rusqlite::params![start_ms, end_ms],
        |r| r.get(0),
    )?;

    Ok(RangeTotals {
        total_ms,
        active_ms: total_ms - idle_ms,
        idle_ms,
        segment_count,
        donut,
        top_apps,
    })
}

/// 类别落在圆环的哪一档。`work` / `browsing` / `idle` 各归各位，
/// **其余全部并入 `unknown`**（含 `unknown` 自己）。
fn donut_bucket(category: &str) -> &'static str {
    match category {
        "work" => "work",
        "browsing" => "browsing",
        "idle" => "idle",
        _ => "unknown",
    }
}

/// 四档全 0。空范围时用。
fn empty_donut() -> Vec<DonutSlice> {
    DONUT_KEYS.iter().map(|k| DonutSlice { key: (*k).to_string(), ms: 0 }).collect()
}
```

在 `src-tauri/crates/storage/src/lib.rs` 的 re-export 里补上：

```rust
pub use summary::{
    daily_calendar, range_totals, AppSlice, DailyCalendar, DayCell, DonutSlice, RangeTotals,
};
```

- [ ] **Step 4: 运行确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace range_totals
```

Expected: 6 passed。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/crates/storage/src/summary.rs src-tauri/crates/storage/src/lib.rs src-tauri/crates/storage/tests/summary.rs
git commit -m "$(cat <<'EOF'
feat: range totals — durations, donut split, top apps

圆环固定四档（work/browsing/idle/unknown），其余四类并入 unknown，
这样 rules.toml 命中它们时环仍是完整 360°。并档不改变总时长，有测试守。

区间倒置返回全 0 而不是 Err：公开的 IPC 会收到这种输入。

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: `range_pacing` — 必须逐段算的两项

`switchCount` 和 `hourlyMs` 都得按时间顺序走一遍段，SQL 干不了（前者看相邻间隔，后者要把跨小时的段劈开）。

**Files:**
- Modify: `src-tauri/crates/storage/src/summary.rs`
- Modify: `src-tauri/crates/storage/tests/summary.rs`

**Interfaces:**
- Consumes: Task 2 的 `summary.rs`、`crate::activity::get_segments_in_range`
- Produces:
  ```rust
  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct RangePacing {
      pub switch_count: i64,
      /// 24 个桶，按**本地**小时切
      pub hourly_ms: [i64; 24],
  }

  pub fn range_pacing(conn: &Connection, start_ms: i64, end_ms: i64, offset_secs: i32)
      -> Result<RangePacing, rusqlite::Error>
  ```

`offset_secs` 由 app 层从 `UtcOffset::whole_seconds()` 传进来——storage crate 不依赖 `time`，不该为了取小时数多引一个时间库。

- [ ] **Step 1: 写失败的测试**

追加到 `src-tauri/crates/storage/tests/summary.rs`：

```rust
use activity_storage::range_pacing;

const CST: i32 = 8 * 3600;   // 本组测试固定东八区，不依赖运行机器的时区

#[test]
fn short_gaps_are_not_switches() {
    let conn = open_in_memory();
    // 间隔 4 分钟、间隔 6 分钟 -> 只算 1 次切换
    put(&conn, vec![
        segc("a", 0,             60_000, "work", Some("Code.exe")),
        segc("b", 4 * 60_000,    5 * 60_000, "work", Some("Code.exe")),
        segc("c", 11 * 60_000,   12 * 60_000, "work", Some("Code.exe")),
    ]);
    assert_eq!(range_pacing(&conn, 0, 3_600_000, CST).unwrap().switch_count, 1,
               "4 分钟不算，6 分钟算");
}

#[test]
fn single_segment_has_no_switches() {
    let conn = open_in_memory();
    put(&conn, vec![segc("a", 0, 60_000, "work", Some("Code.exe"))]);
    assert_eq!(range_pacing(&conn, 0, 3_600_000, CST).unwrap().switch_count, 0);
}

#[test]
fn empty_range_yields_zero_switches_and_flat_profile() {
    let conn = open_in_memory();
    put(&conn, vec![segc("a", 5_000_000, 6_000_000, "work", Some("Code.exe"))]);
    let p = range_pacing(&conn, 9_000_000, 1_000_000, CST).unwrap();
    assert_eq!(p.switch_count, 0);
    assert_eq!(p.hourly_ms, [0i64; 24]);
}

#[test]
fn hourly_buckets_use_local_hour_not_utc() {
    // 东八区下，一个 UTC 03:20 的段必须落进本地 11 号桶。
    let ts = 1_770_000_000_000i64;
    let conn = open_in_memory();
    put(&conn, vec![segc("a", ts, ts + 600_000, "work", Some("Code.exe"))]);
    let p = range_pacing(&conn, 0, i64::MAX, CST).unwrap();
    let expect = (((ts + (CST as i64) * 1000).div_euclid(3_600_000)).rem_euclid(24)) as usize;
    assert_eq!(p.hourly_ms[expect], 600_000, "UTC 偏移必须换算成东八区");
    assert_eq!(p.hourly_ms.iter().sum::<i64>(), 600_000, "总量不因换算丢失或重复");
}

#[test]
fn segment_spanning_hour_boundaries_is_split() {
    // 01:00 -> 04:20，跨 3 个小时桶
    let conn = open_in_memory();
    put(&conn, vec![segc("a", 3_600_000, 4 * 3_600_000 + 20 * 60_000,
                        "work", Some("Code.exe"))]);
    let p = range_pacing(&conn, 0, i64::MAX, CST).unwrap();
    assert_eq!(p.hourly_ms[1], 60 * 60_000);
    assert_eq!(p.hourly_ms[2], 60 * 60_000);
    assert_eq!(p.hourly_ms[3], 60 * 60_000);
    assert_eq!(p.hourly_ms[4], 20 * 60_000);
    assert_eq!(p.hourly_ms.iter().sum::<i64>(), 3 * 3_600_000 + 20 * 60_000);
}

#[test]
fn idle_segments_do_not_enter_the_hourly_profile() {
    // spec §3.2：hourlyMs 只记非 idle 时长。
    let conn = open_in_memory();
    put(&conn, vec![segc("i", 0, 3_600_000, "idle", None)]);
    assert_eq!(range_pacing(&conn, 0, i64::MAX, CST).unwrap().hourly_ms, [0i64; 24],
               "1 小时挂机不该把「活跃时间段」画满");
}
```

- [ ] **Step 2: 运行确认失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace range_pacing
```

Expected: 编译失败，`cannot find function range_pacing`。

- [ ] **Step 3: 写实现**

追加到 `src-tauri/crates/storage/src/summary.rs`：

```rust
use crate::activity::get_segments_in_range;

/// 相邻两个段之间空多久算「切换了一次」。
///
/// 阈值**固定 5 分钟，不读 `config.toml`** —— `idle_threshold_s` 改的是
/// 「多久算空闲」，会改变哪些段被归成 idle、进而改变段本身。让两个指标
/// 耦合在一个可调参数上，改一次配置会让两个数字的历史不可比（spec §3.2）。
const SWITCH_GAP_MS: i64 = 5 * 60_000;

const HOUR_MS: i64 = 3_600_000;

/// 范围内必须逐段算的两项。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RangePacing {
    pub switch_count: i64,
    pub hourly_ms: [i64; 24],
}

/// 切换次数与 24h 活跃分布。
///
/// `offset_secs` 是本地 UTC 偏移的**秒数**，由 app 层从
/// `UtcOffset::whole_seconds()` 传进来 —— storage crate 不依赖 `time`。
pub fn range_pacing(
    conn: &Connection,
    start_ms: i64,
    end_ms: i64,
    offset_secs: i32,
) -> Result<RangePacing, rusqlite::Error> {
    let mut hourly_ms = [0i64; 24];
    if start_ms >= end_ms {
        return Ok(RangePacing { switch_count: 0, hourly_ms });
    }

    // 已按 start_at 升序（get_segments_in_range 保证）。
    let segs = get_segments_in_range(conn, start_ms, end_ms)?;

    let mut switch_count = 0i64;
    for pair in segs.windows(2) {
        if pair[1].start_at - pair[0].end_at >= SWITCH_GAP_MS {
            switch_count += 1;
        }
    }

    // 24h 分布：逐段按本地小时边界劈开，跨小时的段分给两桶。
    for s in &segs {
        if s.category == "idle" {
            continue;
        }
        add_to_hours(&mut hourly_ms, s.start_at, s.end_at, offset_secs);
    }

    Ok(RangePacing { switch_count, hourly_ms })
}

/// 把 `[start_ms, end_ms)` 按本地小时边界劈开，累加进 `buckets`。
fn add_to_hours(buckets: &mut [i64; 24], start_ms: i64, end_ms: i64, offset_secs: i32) {
    let shift = (offset_secs as i64) * 1000;
    let mut t = start_ms;
    while t < end_ms {
        let local = t + shift;
        // 到「当前这一小时」的末尾，不是到下一个整点起点
        let to_boundary = t + (HOUR_MS - local.rem_euclid(HOUR_MS));
        let seg_end = to_boundary.min(end_ms);
        buckets[local.div_euclid(HOUR_MS).rem_euclid(24) as usize] += seg_end - t;
        t = seg_end;
    }
}
```

在 `src-tauri/crates/storage/src/lib.rs` 的 re-export 里补上 `RangePacing`：

```rust
pub use summary::{
    daily_calendar, range_pacing, range_totals, AppSlice, DailyCalendar, DayCell, DonutSlice,
    RangePacing, RangeTotals,
};
```

- [ ] **Step 4: 运行确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace range_pacing
```

Expected: 6 passed。

`segment_spanning_hour_boundaries_is_split` 若失败，问题在 `to_boundary`：它必须是「**当前时刻所在那个小时**的末尾」，不是下一个整点的起点。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/crates/storage/src/summary.rs src-tauri/crates/storage/src/lib.rs src-tauri/crates/storage/tests/summary.rs
git commit -m "$(cat <<'EOF'
feat: switch count and the 24h activity profile

两者都得按时间顺序走一遍段，SQL 干不了：前者看相邻间隔，后者要把
跨小时的段劈开。hourlyMs 只收非 idle —— 挂机不该把「活跃时间段」画满，
那正是这个图要回答的问题的反面。

切换阈值固定 5 分钟不读 config.toml：idle_threshold 改的是「多久算空闲」，
会改变段本身；耦合会让两个指标的历史不可比。

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: 窗口标题排名（最贵的那一项）

**分层说明**：SQL 按**原始** title 分组（约 1000 行），**归一化与合并在 app 层**。

为什么不放 storage：归一化在 `src-tauri/src/title_norm.rs`（app crate，`regex` 在那儿），
脱敏判定要 `crate::redact::PLACEHOLDER`（也在 app crate）。storage 不该认识这两件事，
它的职责是「SQL + 数据形状」。

- [ ] **Step 1: 写 storage 侧的失败测试**

追加到 `src-tauri/crates/storage/tests/summary.rs`：

```rust
use activity_core::{Event, EventType, WindowFocusPayload, WindowTitleChangePayload};
use activity_storage::{insert_events, title_counts_in_range};

fn title_event(id: &str, ts: i64, title: Option<&str>) -> Event {
    let mut e = Event::new(
        EventType::WindowTitleChange(WindowTitleChangePayload {
            process_name: "msedge.exe".into(),
            window_title: title.map(|t| t.into()),
        }),
        ts,
    );
    e.id = id.into();
    e
}

#[test]
fn titles_are_grouped_by_raw_string() {
    let conn = open_in_memory();
    insert_events(&conn, &[
        title_event("e1", 1_000, Some("a")),
        title_event("e2", 2_000, Some("a")),
        title_event("e3", 3_000, Some("b")),
    ]).unwrap();
    let counts = title_counts_in_range(&conn, 0, 10_000).unwrap();
    assert_eq!(counts, vec![("a".to_string(), 2), ("b".to_string(), 1)]);
}

#[test]
fn null_titles_are_excluded() {
    let conn = open_in_memory();
    insert_events(&conn, &[
        title_event("e1", 1_000, Some("a")),
        title_event("e2", 2_000, None),
    ]).unwrap();
    assert_eq!(title_counts_in_range(&conn, 0, 10_000).unwrap().len(), 1);
}

#[test]
fn non_window_event_types_are_excluded() {
    // 心跳 / 空闲事件没有窗口标题，进不了排名
    let conn = open_in_memory();
    let mut idle = Event::new(EventType::SystemIdle, 1_000);
    idle.id = "i1".into();
    insert_events(&conn, &[idle, title_event("e1", 2_000, Some("a"))]).unwrap();
    assert_eq!(title_counts_in_range(&conn, 0, 10_000).unwrap(),
               vec![("a".to_string(), 1)]);
}

#[test]
fn window_focus_titles_are_included_too() {
    let conn = open_in_memory();
    let mut e = Event::new(
        EventType::WindowFocus(WindowFocusPayload {
            process_name: "msedge.exe".into(),
            window_title: Some("focus".into()),
            exe_path: None,
        }),
        1_000,
    );
    e.id = "f1".into();
    insert_events(&conn, &[e]).unwrap();
    assert_eq!(title_counts_in_range(&conn, 0, 10_000).unwrap(),
               vec![("focus".to_string(), 1)]);
}

#[test]
fn range_is_half_open() {
    let conn = open_in_memory();
    insert_events(&conn, &[
        title_event("e1", 1_000, Some("inside")),
        title_event("e2", 5_000, Some("outside")),
    ]).unwrap();
    assert_eq!(title_counts_in_range(&conn, 0, 5_000).unwrap(),
               vec![("inside".to_string(), 1)]);
}

#[test]
fn empty_range_yields_empty_vec() {
    let conn = open_in_memory();
    insert_events(&conn, &[title_event("e1", 1_000, Some("a"))]).unwrap();
    assert!(title_counts_in_range(&conn, 9_000, 1_000).unwrap().is_empty());
}
```

- [ ] **Step 2: 写 app 侧的失败测试**

追加到 `src-tauri/src/title_norm.rs` 的 `mod tests` 内：

```rust
    use super::{merge_top_titles, normalize_title};

    #[test]
    fn merge_collapses_counter_variants_into_one_row() {
        let raw = vec![
            ("无标题 和另外 31 个页面 - 个人 - Microsoft Edge".to_string(), 1316),
            ("无标题 和另外 32 个页面 - 个人 - Microsoft Edge".to_string(), 1099),
            ("无标题 和另外 33 个页面 - 个人 - Microsoft Edge".to_string(), 913),
            ("New Tab 和另外 31 个页面 - 个人 - Microsoft Edge".to_string(), 1274),
        ];
        let top = merge_top_titles(raw, 10);
        assert_eq!(top.len(), 2, "四条应塌成两条");
        assert_eq!(top[0].title, "无标题");
        assert_eq!(top[0].hits, 1316 + 1099 + 913, "计数要加起来");
        assert_eq!(top[1].title, "New Tab");
        assert_eq!(top[1].hits, 1274);
    }

    #[test]
    fn merge_runs_before_truncating() {
        // 归一化会把行数变少，所以必须先合并再取 top N ——
        // 否则「取前 10 条原始行再归一化」会把 10 条塌成 2 条。
        let raw: Vec<(String, i64)> = (0..30)
            .map(|i| (format!("和另外 {i} 个页面"), 100 - i))
            .collect();
        let top = merge_top_titles(raw, 10);
        assert_eq!(top.len(), 1, "30 条全塌成同一个空串之外的东西");
        assert!(top[0].hits >= 0);
    }

    #[test]
    fn merge_skips_titles_that_normalize_to_nothing() {
        // Review Focus #4：剥空的不能占坑
        let raw = vec![
            ("和另外 31 个页面".to_string(), 50),
            ("   ".to_string(), 999),
            ("真标题".to_string(), 3),
        ];
        let top = merge_top_titles(raw, 10);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].title, "真标题");
    }

    #[test]
    fn merge_marks_redacted_titles() {
        // 归一化不碰占位符，所以这里能直接判
        let raw = vec![("client 42 - [redacted] - Code".to_string(), 7)];
        let top = merge_top_titles(raw, 10);
        assert!(top[0].redacted, "脱敏标记必须穿透到汇总页（spec §3.3）");
    }

    #[test]
    fn merge_keeps_blank_page_titles_visible() {
        // Review Focus #5：无标题 / New Tab 归一化后仍在，且占大头。
        // 这是 spec §4.2 记录的**已知限制，不是 bug**。这条测试防止将来
        // 有人「顺手」加个过滤把它们藏起来——那会让用户以为时间白记了。
        let raw = vec![
            ("无标题".to_string(), 7086),
            ("New Tab".to_string(), 3722),
        ];
        let top = merge_top_titles(raw, 10);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].title, "无标题");
        assert_eq!(top[0].hits, 7086);
    }

    #[test]
    fn merge_respects_limit() {
        let raw: Vec<(String, i64)> = (0..50)
            .map(|i| (format!("t{i}"), 1000 - i))
            .collect();
        assert_eq!(merge_top_titles(raw, 10).len(), 10);
    }
```

- [ ] **Step 3: 运行确认两侧都失败**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace title_counts_in_range
cargo test --manifest-path src-tauri/Cargo.toml --workspace merge_top_titles
```

Expected: 两侧都编译失败（`cannot find function title_counts_in_range` / `cannot find function merge_top_titles`）。

- [ ] **Step 4: 写 storage 侧实现**

追加到 `src-tauri/crates/storage/src/summary.rs`：

```rust
/// 范围内按**原始**字符串分组的窗口标题及其次数。
///
/// 只返回原始计数，**不做归一化** —— 归一化在 app 层（`title_norm.rs`），
/// 因为它要 `regex` 和脱敏占位符，两者都不属于 storage 的职责。
///
/// 两段式的原因：先在 SQL 里把 3.7M 行压成约 1000 行，再在 Rust 里归一化
/// 合并。反过来（先取全部行到内存再归一化）会把整库拉进进程。
pub fn title_counts_in_range(
    conn: &Connection,
    start_ms: i64,
    end_ms: i64,
) -> Result<Vec<(String, i64)>, rusqlite::Error> {
    if start_ms >= end_ms {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT json_extract(payload,'$.window_title') AS t, COUNT(*) AS n
         FROM events
         WHERE type IN ('window_focus','window_title_change')
           AND timestamp >= ?1 AND timestamp < ?2
           AND json_extract(payload,'$.window_title') IS NOT NULL
         GROUP BY t
         ORDER BY n DESC",
    )?;
    let mut rows = stmt.query_map(rusqlite::params![start_ms, end_ms], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })?;
    let mut v = Vec::new();
    while let Some(r) = rows.next()? {
        v.push(r?);
    }
    Ok(v)
}
```

re-export 补上 `title_counts_in_range`。

- [ ] **Step 5: 写 app 侧实现**

追加到 `src-tauri/src/title_norm.rs`（在 `normalize_title` 之后）：

```rust
/// 归一化 + 合并后的一个标题。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergedTitle {
    pub title: String,
    pub hits: i64,
    /// 该标题是否经过脱敏（含有占位符）。判定复用 `titles.rs` 用的同一个常量，
    /// 前端不该硬编码占位符字符串。
    pub redacted: bool,
}

/// 把「原始标题 + 次数」归一化、合并、取前 `limit` 名。
///
/// **先合并再截断**——反过来的话「取前 10 条原始行再归一化」会把 10 行
/// 塌成 2 行，白白丢掉 8 个名额。
pub fn merge_top_titles(raw: Vec<(String, i64)>, limit: usize) -> Vec<MergedTitle> {
    let mut merged: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for (raw_title, hits) in raw {
        // 剥空则跳过：空串会占掉 Top N 的一个坑（Review Focus #4）
        if let Some(norm) = normalize_title(&raw_title) {
            *merged.entry(norm).or_insert(0) += hits;
        }
    }
    let mut out: Vec<MergedTitle> = merged
        .into_iter()
        .map(|(title, hits)| MergedTitle {
            redacted: title.contains(crate::redact::PLACEHOLDER),
            title,
            hits,
        })
        .collect();
    out.sort_by(|a, b| b.hits.cmp(&a.hits).then_with(|| a.title.cmp(&b.title)));
    out.truncate(limit);
    out
}
```

- [ ] **Step 6: 运行确认通过**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace title
cargo test --manifest-path src-tauri/Cargo.toml --workspace merge_top_titles
```

Expected: 两侧全绿。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/crates/storage/src/summary.rs src-tauri/crates/storage/src/lib.rs src-tauri/crates/storage/tests/summary.rs src-tauri/src/title_norm.rs
git commit -m "$(cat <<'EOF'
feat: window-title ranking, normalized and merged

SQL 按原始标题分组（3.7M 行压成约 1000 行），归一化与合并在 app 层 ——
归一化要 regex 和脱敏占位符，两者都不属于 storage 的职责。

先合并再截断：反过来「取前 10 条原始行再归一化」会把 10 行塌成 2 行。

无标题 / New Tab 归一化后仍如实排出：那是 spec §4.2 记录的已知限制，
有测试守着不被将来「顺手」过滤掉。

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: 三个 Tauri command

**Files:**
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: Task 2/3/4/5 的 `daily_calendar` / `range_totals` / `range_pacing` / `title_counts_in_range` / `merge_top_titles`
- Produces: 三个 `#[tauri::command]`，签名见下。Task 7 的 `types.ts` 按这些签名写封装。

- [ ] **Step 1: 写实现**

在 `src-tauri/src/lib.rs` 里，紧挨着现有的 `get_segment_titles` 之后加：

```rust
/// 汇总页的热力图数据。**无参数** —— 热力图永远渲染全部数据，
/// 不受选中范围影响（spec §2.3）。
///
/// 库为空时返回 `None`：那是「刚装完还没跑满一天」的真实状态，
/// 前端据此显示空状态，而不是拿到 first/last 为空串的半成品。
#[tauri::command]
fn get_daily_calendar(
    state: tauri::State<'_, AppState>,
) -> Result<Option<activity_storage::DailyCalendar>, String> {
    let c = state.writer.conn();
    activity_storage::daily_calendar(&c).map_err(|e| e.to_string())
}

/// 汇总页顶部指标。`from` / `to` 都是 `YYYY-MM-DD` 的**闭区间**
/// （点「某一天」时两者相同），内部转成半开区间。
#[tauri::command]
fn get_summary(
    state: tauri::State<'_, AppState>,
    from: String,
    to: String,
) -> Result<SummaryOut, String> {
    let offset = local_offset()?;
    let (start_ms, _) = day_range_ms(&from, offset)?;
    // `day_range_ms(to)` 的第二个返回值就是「to 那天的次日零点」，
    // 正好是闭区间 [from, to] 转半开区间 [from00:00, to+1 00:00) 的终点。
    // 不必自己给日期加一天 —— 加一天要碰 Date::next_day，day_range_ms 已经做了。
    let (_, end_ms) = day_range_ms(&to, offset)?;
    let offset_secs = offset.whole_seconds();

    let c = state.writer.conn();
    let totals =
        activity_storage::range_totals(&c, start_ms, end_ms).map_err(|e| e.to_string())?;
    let pacing = activity_storage::range_pacing(&c, start_ms, end_ms, offset_secs)
        .map_err(|e| e.to_string())?;

    Ok(SummaryOut {
        total_ms: totals.total_ms,
        active_ms: totals.active_ms,
        idle_ms: totals.idle_ms,
        segment_count: totals.segment_count,
        switch_count: pacing.switch_count,
        // Vec 而非 [i64; 24]：前端拿到的就是 24 个数的普通数组。
        hourly_ms: pacing.hourly_ms.to_vec(),
        donut: totals.donut,
        top_apps: totals.top_apps,
    })
}

/// 窗口标题排名。**懒加载**——底部面板进入视口才发这一发（spec §3.3）。
///
/// 不设范围上限：当前 5 天数据实测 199ms；按每天 10,092 事件外推，
/// 一年约 3.7M 行 → 约 15s。这是 spec §9 第 7 条记录的已知代价。
#[tauri::command]
fn get_top_titles(
    state: tauri::State<'_, AppState>,
    from: String,
    to: String,
    limit: Option<usize>,
) -> Result<Vec<MergedTitle>, String> {
    const DEFAULT_LIMIT: usize = 10;
    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(50);
    let offset = local_offset()?;
    let (start_ms, _) = day_range_ms(&from, offset)?;
    let (_, end_ms) = day_range_ms(&to, offset)?;

    let c = state.writer.conn();
    let raw = activity_storage::title_counts_in_range(&c, start_ms, end_ms)
        .map_err(|e| e.to_string())?;
    Ok(title_norm::merge_top_titles(raw, limit))
}
```

`SummaryOut` 定义在同一个文件里，`DonutSlice` / `AppSlice` 从 storage 直接复用：

```rust
/// `get_summary` 的返回形状。storage 的两个结果结构在这里合成一个 ——
/// 前端要的是一个对象，不是两个。
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SummaryOut {
    total_ms: i64,
    active_ms: i64,
    idle_ms: i64,
    segment_count: i64,
    switch_count: i64,
    hourly_ms: Vec<i64>,
    donut: Vec<activity_storage::DonutSlice>,
    top_apps: Vec<activity_storage::AppSlice>,
}
```

把三个函数注册进 handler（`src-tauri/src/lib.rs:204`）：

```rust
.invoke_handler(tauri::generate_handler![
    get_segments,
    get_segment_titles,
    get_daily_calendar,
    get_summary,
    get_top_titles,
])
```

- [ ] **Step 2: 让它编译**

```bash
cargo build --manifest-path src-tauri/Cargo.toml
```

Expected: 编译通过。若 `use` 缺失，补上 `use title_norm::MergedTitle;`。

- [ ] **Step 3: 跑全量 Rust 测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace
```

Expected: 全部通过，测试数比改动前多 0（这一版只接线，不加测试）。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat: three commands for the summary page

get_daily_calendar 无参数（热力图永远渲染全部数据）；
get_summary / get_top_titles 接 from/to 闭区间，内部转成
[from00:00, to+1 00:00) —— day_range_ms 的第二个返回值就是「to 那天的
次日零点」，不需要自己加一天。

SummaryOut 在 app 层把 storage 的两个结果并成一个对象，前端不必合两次。

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: `types.ts` 的三个 IPC 封装

**Files:**
- Modify: `src/types.ts`

**Interfaces:**
- Consumes: Task 6 的三个 command 名与返回形状
- Produces:
  ```ts
  export interface DayCell { date: string; totalMs: number }
  export interface DailyCalendar { first: string; last: string; days: DayCell[] }
  export type DonutKey = "work" | "browsing" | "idle" | "unknown";
  export interface DonutSlice { key: DonutKey; ms: number }
  export interface AppSlice { name: string; ms: number }
  export interface Summary {
    totalMs: number; activeMs: number; idleMs: number;
    segmentCount: number; switchCount: number;
    hourlyMs: number[]; donut: DonutSlice[]; topApps: AppSlice[];
  }
  export interface MergedTitle { title: string; hits: number; redacted: boolean }

  export function getDailyCalendar(): Promise<DailyCalendar | null>;
  export function getSummary(from: string, to: string): Promise<Summary>;
  export function getTopTitles(from: string, to: string, limit?: number): Promise<MergedTitle[]>;
  ```
  Task 8 之后的所有任务都用这三个函数。

- [ ] **Step 1: 写实现**

在 `src/types.ts` 末尾追加：

```ts
/* ---------- 汇总页（spec 2026-10-05） ---------- */

/** 热力图的一格：某一天的监控总时长（含 idle）。 */
export interface DayCell {
  date: string;
  totalMs: number;
}

/**
 * 热力图的数据源。`days` **只含有记录的日期** ——
 * 空缺日期由 `lib/summary.ts` 的 `fillDays` 补齐（后端的 `GROUP BY`
 * 会跳过没开机的那天，直接用它的结果排格子会整片错位）。
 */
export interface DailyCalendar {
  first: string;
  last: string;
  days: DayCell[];
}

/** 圆环的档位。后端恒返回这四项、顺序固定（前端按 key 取色，不靠下标）。 */
export type DonutKey = "work" | "browsing" | "idle" | "unknown";

export interface DonutSlice {
  key: DonutKey;
  ms: number;
}

export interface AppSlice {
  name: string;
  ms: number;
}

/** 顶部指标。`from` / `to` 都是 `YYYY-MM-DD` 的**闭区间**。 */
export interface Summary {
  totalMs: number;
  activeMs: number;
  idleMs: number;
  segmentCount: number;
  switchCount: number;
  /** 24 个桶，按本地小时切。只含非 idle 时长。 */
  hourlyMs: number[];
  donut: DonutSlice[];
  topApps: AppSlice[];
}

/** 归一化后的窗口标题。`redacted` 由后端判定，前端不硬编码占位符。 */
export interface MergedTitle {
  title: string;
  hits: number;
  redacted: boolean;
}

/** 热力图的数据。**无参数** —— 热力图永远渲染全部数据，不受选中范围影响。 */
export async function getDailyCalendar(): Promise<DailyCalendar | null> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<DailyCalendar | null>("get_daily_calendar");
}

export async function getSummary(from: string, to: string): Promise<Summary> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<Summary>("get_summary", { from, to });
}

export async function getTopTitles(
  from: string,
  to: string,
  limit = 10,
): Promise<MergedTitle[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<MergedTitle[]>("get_top_titles", { from, to, limit });
}
```

- [ ] **Step 2: 类型检查**

```bash
pnpm build
```

Expected: 无 tsc 报错。**此时不需要测试** —— 这三个函数只是 `invoke` 的薄封装，
`App.integration.test.tsx` 里已有 `vi.mock("@tauri-apps/api/core")` 的成熟模式，
Task 13 会用它端到端验。

- [ ] **Step 3: 提交**

```bash
git add src/types.ts
git commit -m "$(cat <<'EOF'
feat: IPC wrappers for the summary page

getDailyCalendar 无参数 —— 热力图永远渲染全部数据。getSummary / getTopTitles
接闭区间，后端自己转半开。

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: `lib/summary.ts` — 全部前端逻辑的纯函数层

热力图的布局算法、色阶分档、选中框、范围推导全在这里。**先做纯函数再写组件** —— 这个仓库的既有习惯（`lib/bucket.ts` / `lib/metrics.ts` 都是纯函数 + 独立测试）。

**Files:**
- Create: `src/lib/summary.ts`
- Create: `src/lib/summary.test.ts`

**Interfaces:**
- Consumes: `shiftDate` / `todayString`（已在 `src/types.ts`）、`DayCell` / `DailyCalendar`（Task 7）
- Produces:
  ```ts
  export type RangeKind = "all" | "day" | "week" | "month";
  export interface DateRange { kind: RangeKind; from: string; to: string; label: string }

  export function weekdayOf(date: string): number;
  export function fillDays(first: string, last: string, counts: Map<string, number>): DayCell[];
  export function scaleStep(ms: number, maxMs: number): number;   // 0..5
  export interface WallCell { date: string; totalMs: number; col: number; row: number;
                              present: boolean; step: number }
  export interface WallLayout { cells: WallCell[]; weeks: number;
                                monthLabels: Array<{ col: number; label: string }> }
  export function buildWall(days: DayCell[]): WallLayout;
  export interface Frame { col: number; row: number; cols: number; rows: number }
  export function selectionFrame(layout: WallLayout, from: string, to: string): Frame | null;
  export function startOfWeek(date: string): string;
  export function endOfMonth(date: string): string;
  export function rangeFor(kind: RangeKind, date: string, all: DateRange): DateRange;
  export function rangeLabel(r: DateRange): string;
  ```
  Task 9/10/13 消费这些。

- [ ] **Step 1: 写失败的测试**

`src/lib/summary.test.ts`：

```ts
import { describe, it, expect } from "vitest";
import {
  buildWall, endOfMonth, fillDays, rangeFor, scaleColor, scaleStep,
  selectionFrame, startOfWeek, weekdayOf,
  type DateRange, type DayCell,
} from "./summary";

const ALL: DateRange = { kind: "all", from: "2026-10-01", to: "2026-10-05", label: "全部" };

function days(spec: Array<[string, number]>): DayCell[] {
  return spec.map(([date, totalMs]) => ({ date, totalMs }));
}

/** 2026-10-01 是周四，10-05 是周一 —— 这组日期能同时验首列补齐与跨列。 */
describe("weekdayOf", () => {
  it("周日返回 0", () => {
    expect(weekdayOf("2026-10-04")).toBe(0);
  });
  it("周六返回 6", () => {
    expect(weekdayOf("2026-10-03")).toBe(6);
  });
});

describe("fillDays", () => {
  it("把没有记录的日期补成 totalMs 0", () => {
    const counts = new Map([["2026-10-01", 100], ["2026-10-04", 200]]);
    const out = fillDays("2026-10-01", "2026-10-05", counts);
    expect(out.map((d) => d.date)).toEqual([
      "2026-10-01", "2026-10-02", "2026-10-03", "2026-10-04", "2026-10-05",
    ]);
    expect(out.map((d) => d.totalMs)).toEqual([100, 0, 0, 200, 0]);
  });

  it("from 晚于 to 时返回空数组而不是死循环", () => {
    expect(fillDays("2026-10-05", "2026-10-01", new Map())).toEqual([]);
  });
});

describe("scaleStep", () => {
  it("0 小时落轨道色档 0", () => {
    expect(scaleStep(0, 10_000)).toBe(0);
  });
  it("四档边界取闭区间下界", () => {
    const max = 1000;
    expect(scaleStep(1, max)).toBe(1);      // 0.1  -> 第 1 档
    expect(scaleStep(200, max)).toBe(1);   // 恰好 0.2
    expect(scaleStep(201, max)).toBe(2);
    expect(scaleStep(400, max)).toBe(2);
    expect(scaleStep(401, max)).toBe(3);
    expect(scaleStep(600, max)).toBe(3);
    expect(scaleStep(601, max)).toBe(4);
    expect(scaleStep(800, max)).toBe(4);
    expect(scaleStep(801, max)).toBe(5);
    expect(scaleStep(max, max)).toBe(5);
  });
  it("maxMs 为 0 时不产生 NaN", () => {
    expect(Number.isFinite(scaleStep(0, 0))).toBe(true);
    expect(scaleStep(0, 0)).toBe(0);
  });
  it("档 0 走轨道色，1..5 走紫阶", () => {
    expect(scaleColor(0)).toBe("var(--color-surface-2)");
    expect(scaleColor(1)).toBe("var(--color-scale-1)");
    expect(scaleColor(5)).toBe("var(--color-scale-5)");
  });
});

describe("buildWall", () => {
  it("第一行是周日", () => {
    const w = buildWall(days([["2026-10-01", 1], ["2026-10-02", 2], ["2026-10-03", 3]]));
    for (const c of w.cells.filter((x) => x.row === 0)) {
      expect(weekdayOf(c.date)).toBe(0);
    }
  });

  it("首列往前补齐到周日：10-01 是周四，首列前 4 格是补齐位", () => {
    const w = buildWall(days([["2026-10-01", 1]]));
    const col0 = w.cells.filter((c) => c.col === 0);
    expect(col0).toHaveLength(7);
    // 周四 -> row 4；row 0..3 是补齐位
    expect(col0.slice(0, 4).every((c) => c.present === false)).toBe(true);
    expect(col0[4]).toMatchObject({ date: "2026-10-01", present: true, row: 4 });
  });

  it("末列往后补齐到周六", () => {
    const w = buildWall(days([["2026-10-05", 1]]));   // 周一
    const lastCol = w.cells.filter((c) => c.col === w.weeks - 1);
    const present = lastCol.filter((c) => c.present);
    expect(present).toHaveLength(1);
    expect(present[0].date).toBe("2026-10-05");
  });

  it("只有一天数据时仍能画出整周（Review Focus #2）", () => {
    const w = buildWall(days([["2026-10-05", 3_600_000]]));
    expect(w.cells.filter((c) => c.present)).toHaveLength(1);
    expect(w.weeks).toBeGreaterThanOrEqual(1);
    // 补齐位不参与色阶，step 全是 0
    expect(w.cells.filter((c) => !c.present).every((c) => c.step === 0)).toBe(true);
  });

  it("空数据返回空布局而不是崩", () => {
    const w = buildWall([]);
    expect(w.cells).toEqual([]);
    expect(w.weeks).toBe(0);
    expect(w.monthLabels).toEqual([]);
  });

  it("色阶按最大值相对分档", () => {
    const w = buildWall(days([["2026-10-01", 10_000], ["2026-10-02", 2_500]]));
    const by = Object.fromEntries(w.cells.filter((c) => c.present).map((c) => [c.date, c.step]));
    expect(by["2026-10-01"]).toBe(5);
    expect(by["2026-10-02"]).toBe(3);
  });

  it("跨月时每月出一次标签", () => {
    const w = buildWall(days([
      ["2026-09-28", 1], ["2026-09-29", 1], ["2026-09-30", 1],
      ["2026-10-01", 1], ["2026-10-02", 1], ["2026-10-03", 1], ["2026-10-04", 1],
    ]));
    const labels = w.monthLabels.map((m) => m.label);
    expect(labels).toContain("9月");
    expect(labels).toContain("10月");
  });

  it("相邻月份列距不足 4 列时后一个不画标签（GitHub 的做法）", () => {
    const w = buildWall(days([["2026-09-30", 1], ["2026-10-01", 1]]));
    const labels = w.monthLabels.map((m) => m.label);
    // 9 月在第 0 列，10 月只隔 1 列 -> 只应有 9 月
    expect(labels).toEqual(["9月"]);
  });
});

describe("selectionFrame", () => {
  const w = buildWall(days([
    ["2026-10-01", 1], ["2026-10-02", 1], ["2026-10-03", 1],
    ["2026-10-08", 1], ["2026-10-09", 1], ["2026-10-10", 1],
  ]));

  it("选中单日 = 1 格", () => {
    expect(selectionFrame(w, "2026-10-02", "2026-10-02")).toEqual({
      col: 0, row: 5, cols: 1, rows: 1,
    });
  });

  it("选中一周 = 1 列 × 7 格", () => {
    const f = selectionFrame(w, "2026-10-01", "2026-10-07");
    expect(f).not.toBeNull();
    expect(f!.cols).toBe(1);
    expect(f!.rows).toBe(7);
  });

  it("选中一个月 = 跨多列 × 7 格", () => {
    const w2 = buildWall(days([
      ["2026-09-01", 1], ["2026-09-15", 1], ["2026-09-30", 1], ["2026-10-01", 1],
    ]));
    const f = selectionFrame(w2, "2026-09-01", "2026-09-30")!;
    expect(f.rows).toBe(7);
    expect(f.cols).toBeGreaterThan(1);
  });

  it("范围里一天都没有时返回 null", () => {
    expect(selectionFrame(w, "2026-11-01", "2026-11-30")).toBeNull();
  });
});

describe("范围推导", () => {
  it("startOfWeek 从周日起算", () => {
    // 2026-10-01 周四 -> 上一个周日 2026-09-27
    expect(startOfWeek("2026-10-01")).toBe("2026-09-27");
    expect(startOfWeek("2026-10-04")).toBe("2026-10-04");  // 本身是周日
  });

  it("endOfMonth 取当月最后一天", () => {
    expect(endOfMonth("2026-10-15")).toBe("2026-10-31");
    expect(endOfMonth("2026-02-10")).toBe("2026-02-28");
  });

  it("rangeFor day 收敛成同一天", () => {
    const r = rangeFor("day", "2026-10-03", ALL);
    expect(r.from).toBe("2026-10-03");
    expect(r.to).toBe("2026-10-03");
  });

  it("rangeFor week 覆盖整周", () => {
    const r = rangeFor("week", "2026-10-01", ALL);
    expect(r.from).toBe("2026-09-27");
    expect(r.to).toBe("2026-10-03");
  });

  it("rangeFor month 覆盖整月", () => {
    const r = rangeFor("month", "2026-10-01", ALL);
    expect(r.from).toBe("2026-10-01");
    expect(r.to).toBe("2026-10-31");
  });

  it("rangeFor all 原样返回传入的 all", () => {
    expect(rangeFor("all", "2026-10-01", ALL)).toEqual(ALL);
  });
});
```

- [ ] **Step 2: 运行确认失败**

```bash
pnpm vitest run src/lib/summary.test.ts
```

Expected: 失败，`Failed to resolve import "./summary"`。

- [ ] **Step 3: 写实现**

`src/lib/summary.ts`：

```ts
/**
 * 汇总页的纯逻辑层：热力图布局、色阶分档、选中框、范围推导。
 *
 * 全部是无 IO 的纯函数——布局算法是这个页面最容易错的地方
 * （周日起始、首列补齐、跨格框），把它从 React 组件里剥出来，
 * 才能一条条地断言。
 *
 * 热力图口径见 spec §5.1：**7 行 = 周日到周六**（row 0 是周日），
 * 1 列 = 1 周（自周日起）。
 */

import { shiftDate, todayString, type DayCell } from "../types";

/** 格子尺寸常量。年的设计以后定，列数策略也只动 buildWall 一处。 */
export const CELL = 11;
export const GAP = 3;
/** 格子在一行里占的宽度（含间隙）—— 选中框用它算绝对定位。 */
export const PITCH = CELL + GAP;

/** 一个月至少隔这么多列才画第二个月份标签，否则标签会叠在一起。 */
const MONTH_LABEL_MIN_GAP = 4;

/** `YYYY-MM-DD` 的本地星期，0 = 周日。 */
export function weekdayOf(date: string): number {
  const [y, m, d] = date.split("-").map(Number);
  return new Date(y, m - 1, d).getDay();
}

/** 从 `from` 到 `to` 的连续日期序列，没记录的填 0。 */
export function fillDays(
  first: string,
  last: string,
  counts: Map<string, number>,
): DayCell[] {
  const out: DayCell[] = [];
  // 上限是防御性的：万一调用方传了乱序的范围，别把这里变成死循环
  for (let i = 0; i < 4000; i++) {
    if (first > last) break;
    out.push({ date: first, totalMs: counts.get(first) ?? 0 });
    first = shiftDate(first, 1);
  }
  return out;
}

/** 分成 0..5 六档。0 是轨道色（0 活跃就该看起来是空的），1..5 走紫阶。 */
export function scaleStep(ms: number, maxMs: number): number {
  if (ms <= 0) return 0;
  const r = ms / Math.max(maxMs, 1);
  if (r <= 0.2) return 1;
  if (r <= 0.4) return 2;
  if (r <= 0.6) return 3;
  if (r <= 0.8) return 4;
  return 5;
}

/** 档位 -> CSS 颜色。0 用轨道色，1..5 用指标模式那套紫阶。 */
export function scaleColor(step: number): string {
  return step === 0 ? "var(--color-surface-2)" : `var(--color-scale-${step})`;
}

export interface WallCell {
  date: string;
  totalMs: number;
  col: number;
  /** 0 = 周日 */
  row: number;
  /** false = 补齐位（数据范围之外），不渲染但占位，保证不整列错位 */
  present: boolean;
  step: number;
}

export interface WallLayout {
  cells: WallCell[];
  weeks: number;
  monthLabels: Array<{ col: number; label: string }>;
}

/**
 * 把连续的日期序列摊成 GitHub 贡献墙的网格。
 *
 * 首列往前补到周日、末列往后补到周六 —— 不补的话第一列和最后一列
 * 会整列错位（spec §5.1）。
 */
export function buildWall(days: DayCell[]): WallLayout {
  if (days.length === 0) return { cells: [], weeks: 0, monthLabels: [] };

  const maxMs = Math.max(...days.map((d) => d.totalMs), 0);
  const first = days[0].date;
  const last = days[days.length - 1].date;
  const lead = weekdayOf(first);
  const total = days.length + lead + (6 - weekdayOf(last)) + 1;

  const cells: WallCell[] = [];
  const monthLabels: Array<{ col: number; label: string }> = [];
  let cursor = shiftDate(first, -lead);
  let lastMonth = -1;
  let lastLabelCol = -MONTH_LABEL_MIN_GAP;

  for (let i = 0; i < total; i++) {
    const col = Math.floor(i / 7);
    const row = i % 7;
    const present = i >= lead && i < lead + days.length;
    const totalMs = present ? (days[i - lead]?.totalMs ?? 0) : 0;
    cells.push({ date: cursor, totalMs, col, row, present, step: scaleStep(totalMs, maxMs) });

    if (present) {
      const month = Number(cursor.slice(5, 7));
      if (month !== lastMonth) {
        // 一个月只标一次；离上一个标签太近就不画（GitHub 的做法）
        if (col - lastLabelCol >= MONTH_LABEL_MIN_GAP) {
          monthLabels.push({ col, label: `${month}月` });
          lastLabelCol = col;
        }
        lastMonth = month;
      }
    }
    cursor = shiftDate(cursor, 1);
  }

  return { cells, weeks: Math.ceil(total / 7), monthLabels };
}

/** 选中框的尺寸，单位是「格」。`cols`/`rows` 决定框跨多大一块。 */
export interface Frame {
  col: number;
  row: number;
  cols: number;
  rows: number;
}

/** 算出 `[from, to]` 在墙里占的那一块。范围内一天都没有时返回 `null`。 */
export function selectionFrame(layout: WallLayout, from: string, to: string): Frame | null {
  const hits = layout.cells.filter((c) => c.present && c.date >= from && c.date <= to);
  if (hits.length === 0) return null;
  const col = Math.min(...hits.map((c) => c.col));
  const row = Math.min(...hits.map((c) => c.row));
  const colEnd = Math.max(...hits.map((c) => c.col));
  const rowEnd = Math.max(...hits.map((c) => c.row));
  return { col, row, cols: colEnd - col + 1, rows: rowEnd - row + 1 };
}

/** 该日期所在周的**周日**。 */
export function startOfWeek(date: string): string {
  return shiftDate(date, -weekdayOf(date));
}

/** 该日期所在月的最后一天。 */
export function endOfMonth(date: string): string {
  const [y, m] = date.split("-").map(Number);
  // m = 0 会退成上个月最后一天（JS 的 Date 惯例）
  return todayString(new Date(y, m, 0));
}

export type RangeKind = "all" | "day" | "week" | "month";

export interface DateRange {
  kind: RangeKind;
  /** `YYYY-MM-DD`，闭区间 */
  from: string;
  to: string;
  label: string;
}

/** 从热力图上的一次点击推导出新的选中范围。 */
export function rangeFor(kind: RangeKind, date: string, all: DateRange): DateRange {
  switch (kind) {
    case "all":
      return all;
    case "day":
      return { kind, from: date, to: date, label: date };
    case "week": {
      const from = startOfWeek(date);
      return { kind, from, to: shiftDate(from, 6), label: "本周" };
    }
    case "month": {
      const from = `${date.slice(0, 7)}-01`;
      return { kind, from, to: endOfMonth(date), label: `${Number(date.slice(5, 7))}月` };
    }
  }
}

/** 给用户看的范围文案。 */
export function rangeLabel(r: DateRange): string {
  if (r.kind === "all") return "全部";
  if (r.kind === "day") return r.from;
  if (r.kind === "week") return `${r.from} – ${r.to}`;
  return r.label;
}
```

- [ ] **Step 4: 运行确认通过**

```bash
pnpm vitest run src/lib/summary.test.ts
```

Expected: 全部通过。

若 `首列往前补齐到周日` 失败，检查 `lead = weekdayOf(first)` 与 `cursor = shiftDate(first, -lead)` 是否同时用了周日起算。

- [ ] **Step 5: 提交**

```bash
git add src/lib/summary.ts src/lib/summary.test.ts
git commit -m "$(cat <<'EOF'
feat: heatmap layout and range arithmetic as pure functions

布局算法是这个页面最容易错的地方（周日起始、首列补齐、跨格框），
从组件里剥出来才能一条条断言。

色阶 0 档走轨道色：0 活跃就该看起来是空的，MASTER §2.5 对「没有活动的桶」
也是同一个处理。

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: 侧边栏加「汇总」+ 页面骨架

**Files:**
- Modify: `src/components/Sidebar.tsx`
- Modify: `src/components/Sidebar.test.tsx`
- Modify: `src/App.tsx`
- Create: `src/views/SummaryPage.tsx`

**Interfaces:**
- Consumes: 无
- Produces: `View` 联合类型加 `"summary"`；`SummaryPage` 组件（本任务只出空壳，Task 13 填内容）

- [ ] **Step 1: 改 `Sidebar.tsx`**

三处：

```ts
import { Activity, BarChart3, Monitor, Moon, Settings, Sun } from "lucide-react";

/** 左侧导航目前有三个落点。 */
export type View = "summary" | "monitor" | "settings";
```

导航区加一项，**排在「监控采集」上面**（spec §2.1）：

```tsx
<SidebarItem
  icon={BarChart3}
  label="汇总"
  active={view === "summary"}
  onClick={() => onViewChange("summary")}
/>
<SidebarItem
  icon={Activity}
  label="监控采集"
  active={view === "monitor"}
  onClick={() => onViewChange("monitor")}
/>
```

- [ ] **Step 2: 改 `Sidebar.test.tsx`（会红）**

`src/components/Sidebar.test.tsx` 里那条 `expect(screen.getAllByRole("button")).toHaveLength(3)`
必须改成 4，否则测试挂。另外把 Harness 的初始 view 改成 `"summary"` 不用动，
它只断言按钮总数。

```tsx
it("上下两层：上层两个导航，下层两个工具", () => {
  const { container } = render(<Harness />);
  const nav = container.querySelector("nav")!;
  expect(nav.getAttribute("aria-label")).toBe("主导航");
  expect(screen.getAllByRole("button")).toHaveLength(4);
});
```

再加一条，钉住顺序（汇总在监控采集**上面**）：

```tsx
it("汇总排在监控采集上面", () => {
  render(<Harness />);
  const labels = screen
    .getAllByRole("button")
    .map((b) => b.getAttribute("aria-label"));
  expect(labels.indexOf("汇总")).toBeLessThan(labels.indexOf("监控采集"));
});
```

- [ ] **Step 3: 跑测试确认通过**

```bash
pnpm vitest run src/components/Sidebar.test.tsx
```

Expected: 通过。

- [ ] **Step 4: 建 `SummaryPage` 空壳**

`src/views/SummaryPage.tsx`（`src/views/` 目录要新建）：

```tsx
/**
 * 汇总页。范围状态 + 三段布局都在这里。
 *
 * 顶栏那个「范围」chip 本轮是**死控件**（spec §1.2）—— 真实的范围切换
 * 全部由热力图上的点击驱动。
 */
export default function SummaryPage() {
  return (
    <section aria-label="汇总" className="panel flex-1 p-4">
      <h1 className="panel-title">汇总</h1>
      <p className="mt-2 text-sm text-ink-faint">还没有内容。</p>
    </section>
  );
}
```

- [ ] **Step 5: `App.tsx` 接上分支**

```tsx
import SummaryPage from "./views/SummaryPage";
```

`view` 状态**保持默认值 `"monitor"`** —— 改默认值会让 `App.integration.test.tsx`
里一批用例失效，而那批守的是既有功能，不该为了新页面改它们。用户从侧边栏点进去即可。

渲染分支：

```tsx
{view === "settings" ? (
  <SettingsPage />
) : view === "summary" ? (
  <SummaryPage />
) : (
  <> ... 现有的监控采集内容原样 ... </>
)}
```

- [ ] **Step 6: 验证**

```bash
pnpm test
pnpm build
```

Expected: 全绿，tsc 无报错。

- [ ] **Step 7: 提交**

```bash
git add src/components/Sidebar.tsx src/components/Sidebar.test.tsx src/App.tsx src/views/SummaryPage.tsx
git commit -m "$(cat <<'EOF'
feat: 侧边栏加「汇总」页，排在监控采集上面

App 的默认 view 保持 monitor —— 改默认值会让既有的一批集成测试失效，
那批守的是已交付功能，不该为了新页面改它们。

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: `ContributionWall` — 热力图与选中框

页面里最复杂的组件。它只做一件事：把 `WallLayout` 画出来，并把点击转成回调。

**Files:**
- Create: `src/components/ContributionWall.tsx`
- Create: `src/components/ContributionWall.test.tsx`

**Interfaces:**
- Consumes: Task 8 的 `buildWall` / `selectionFrame` / `CELL` / `GAP` / `PITCH` / `scaleColor`
- Produces:
  ```tsx
  interface ContributionWallProps {
    days: DayCell[];
    /** null = 当前是「全部」，不框 */
    selection: DateRange | null;
    onSelectDay(date: string): void;
    onSelectWeek(sunday: string): void;
    onSelectMonth(date: string): void;
  }
  export default function ContributionWall(props: ContributionWallProps): JSX.Element
  ```

- [ ] **Step 1: 写失败的测试**

`src/components/ContributionWall.test.tsx`：

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import ContributionWall from "./ContributionWall";
import type { DateRange } from "../lib/summary";
import type { DayCell } from "../types";

/** 2026-10-01(周四) 起三天 —— 首列必然有 4 个补齐位。 */
const DAYS: DayCell[] = [
  { date: "2026-10-01", totalMs: 3_600_000 },
  { date: "2026-10-02", totalMs: 7_200_000 },
  { date: "2026-10-03", totalMs: 0 },
];

const NOOP = () => {};

function setup(selection: DateRange | null = null) {
  const onSelectDay = vi.fn();
  const onSelectWeek = vi.fn();
  const onSelectMonth = vi.fn();
  render(
    <ContributionWall
      days={DAYS}
      selection={selection}
      onSelectDay={onSelectDay}
      onSelectWeek={onSelectWeek}
      onSelectMonth={onSelectMonth}
    />,
  );
  return { onSelectDay, onSelectWeek, onSelectMonth };
}

describe("ContributionWall", () => {
  it("空数据给空状态，不画空墙", () => {
    // Review Focus #1：空库是新装用户的第一屏，不能画出一面假墙
    const { container } = render(
      <ContributionWall days={[]} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    expect(container.querySelector("[role='grid']")).toBeNull();
    expect(screen.getByText(/还没有采集数据/)).toBeTruthy();
  });

  it("有数据时渲染 7 行（周日到周六）", () => {
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    const grid = container.querySelector("[role='grid']") as HTMLElement;
    expect(grid.style.gridTemplateRows.split("repeat")[1]?.trim()).toMatch(/^7/);
  });

  it("补齐位不渲染成格子", () => {
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    // 三天数据 = 7 个有格子的位置 + 4 个补齐位
    expect(container.querySelectorAll("[data-date]")).toHaveLength(3);
  });

  it("点格子回调那一天", () => {
    const { onSelectDay } = setup();
    fireEvent.click(screen.getByTestId("cell-2026-10-02"));
    expect(onSelectDay).toHaveBeenCalledWith("2026-10-02");
  });

  it("点周条回调那一周的周日", () => {
    const { onSelectWeek } = setup();
    fireEvent.click(screen.getAllByTestId("week-strip")[0]);
    // 2026-10-01 是周四，首列的周日是 2026-09-27
    expect(onSelectWeek).toHaveBeenCalledWith("2026-09-27");
  });

  it("点月标签回调那个月的某一天", () => {
    const { onSelectMonth } = setup();
    const label = screen.getByTestId("month-label-9");
    fireEvent.click(label);
    expect(onSelectMonth).toHaveBeenCalled();
  });

  it("选中单日时框是 1 格", () => {
    const { container } = render(
      <ContributionWall
        days={DAYS}
        selection={{ kind: "day", from: "2026-10-02", to: "2026-10-02", label: "" }}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP}
      />,
    );
    const f = container.querySelector("[data-frame]") as HTMLElement;
    // 2026-10-02 是周五 -> 首列 row 5
    expect(f.style.width).toBe("11px");
    expect(f.style.height).toBe("11px");
  });

  it("没有选中时不画框", () => {
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    expect(container.querySelector("[data-frame]")).toBeNull();
  });

  it("框不压暗框外的格子（只描框，MASTER 的 3:1 红线）", () => {
    const { container } = render(
      <ContributionWall
        days={DAYS}
        selection={{ kind: "day", from: "2026-10-02", to: "2026-10-02", label: "" }}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP}
      />,
    );
    // 框外的格子不许有 opacity 变体
    expect(container.innerHTML).not.toContain("opacity");
  });
});
```

- [ ] **Step 2: 运行确认失败**

```bash
pnpm vitest run src/components/ContributionWall.test.tsx
```

Expected: 失败，`Failed to resolve import "./ContributionWall"`。

- [ ] **Step 3: 写实现**

`src/components/ContributionWall.tsx`：

```tsx
import { useMemo } from "react";
import {
  buildWall, selectionFrame, CELL, GAP, PITCH, scaleColor, type DateRange,
} from "../lib/summary";
import type { DayCell } from "../types";

interface ContributionWallProps {
  days: DayCell[];
  /** `null` = 当前是「全部」，不框 */
  selection: DateRange | null;
  onSelectDay(date: string): void;
  onSelectWeek(sunday: string): void;
  onSelectMonth(date: string): void;
}

/**
 * 监控时长的 GitHub 贡献墙。7 行 = 周日到周六，1 列 = 1 周。
 *
 * **框是跨格的一整块绝对定位矩形，不是逐格描边** —— 框的宽度本身就
 * 告诉用户当前框的是日、周还是月（spec §5.3）。
 * 框外**不压暗**：聚光灯方案会让远处月份的对比度跌破 3:1。
 */
export default function ContributionWall({
  days, selection, onSelectDay, onSelectWeek, onSelectMonth,
}: ContributionWallProps) {
  const layout = useMemo(() => buildWall(days), [days]);
  const frame = useMemo(
    () => (selection ? selectionFrame(layout, selection.from, selection.to) : null),
    [layout, selection],
  );

  if (layout.cells.length === 0) {
    return <p className="text-sm text-ink-faint">还没有采集数据。</p>;
  }

  /** 第 ci 列那个周日。没数据的列返回 undefined。 */
  const sundayOf = (ci: number): string | undefined =>
    layout.cells.find((c) => c.col === ci && c.row === 0 && c.present)?.date;

  return (
    <div className="inline-block">
      {/* 月份标签：浮在每月首次出现的那一列上方 */}
      <div className="relative mb-1 h-3" style={{ width: layout.weeks * PITCH - GAP }}>
        {layout.monthLabels.map((m) => (
          <button
            key={`${m.label}-${m.col}`}
            type="button"
            data-testid={`month-label-${m.label.replace("月", "")}`}
            onClick={() => onSelectMonth(m.col === 0 ? layout.cells[0].date : sundayOf(m.col)!)}
            className="absolute cursor-pointer text-[9px] whitespace-nowrap text-ink-faint hover:text-ink"
            style={{ left: m.col * PITCH }}
          >
            {m.label}
          </button>
        ))}
      </div>

      <div className="relative inline-block" style={{ width: layout.weeks * PITCH - GAP }}>
        <div
          role="grid"
          aria-label="监控时长"
          className="grid"
          style={{
            gridTemplateColumns: `repeat(${layout.weeks}, ${CELL}px)`,
            gridTemplateRows: `repeat(7, ${CELL}px)`,
            // 必须列优先：一个格子自上而下填满 7 行才换列，否则周会被打横
            gridAutoFlow: "column",
            gap: GAP,
          }}
        >
          {layout.cells.filter((c) => c.present).map((c) => (
            <button
              key={c.date}
              type="button"
              role="gridcell"
              data-date={c.date}
              data-testid={`cell-${c.date}`}
              title={`${c.date} · ${Math.round(c.totalMs / 3_600_000 * 10) / 10}h`}
              onClick={() => onSelectDay(c.date)}
              className="cursor-pointer rounded-[2px] outline-offset-1 focus-visible:outline-1 focus-visible:outline-ink"
              style={{ background: scaleColor(c.step) }}
            />
          ))}
        </div>

        {/* 选中框：一整块。宽度 = 跨的列数 × PITCH - GAP */}
        {frame && (
          <span
            data-frame=""
            aria-hidden
            className="pointer-events-none absolute rounded-[3px] border-[1.5px] border-ink"
            style={{
              left: frame.col * PITCH,
              top: frame.row * PITCH,
              width: frame.cols * PITCH - GAP,
              height: frame.rows * PITCH - GAP,
            }}
          />
        )}
      </div>

      {/* 周条：格子下方那条空隙。点了就是选一周。 */}
      <div
        data-testid="week-strip"
        className="mt-1.5 grid"
        style={{ gridTemplateColumns: `repeat(${layout.weeks}, ${CELL}px)`, columnGap: GAP }}
      >
        {Array.from({ length: layout.weeks }, (_, ci) => {
          const sunday = sundayOf(ci);
          return (
            <button
              key={ci}
              type="button"
              aria-label={sunday ? `选择 ${sunday} 那周` : "这一周没有数据"}
              disabled={!sunday}
              onClick={() => sunday && onSelectWeek(sunday)}
              className="h-1 cursor-pointer rounded-sm bg-surface-2 transition-colors hover:bg-ink-ghost disabled:cursor-default disabled:opacity-40"
            />
          );
        })}
      </div>

      <p className="mt-1 text-[10px] text-ink-faint">左：周日 → 周六。点格子选一天，点下方细条选一周，点月份选整月。</p>
    </div>
  );
}
```

- [ ] **Step 4: 运行确认通过**

```bash
pnpm vitest run src/components/ContributionWall.test.tsx
```

Expected: 9 passed。

`补齐位不渲染成格子` 若失败，检查 `layout.cells.filter((c) => c.present)` —— 补齐位必须过滤掉，但**在 `buildWall` 里它们仍要占 `col`/`row` 位置**，否则整列错位。

- [ ] **Step 5: 提交**

```bash
git add src/components/ContributionWall.tsx src/components/ContributionWall.test.tsx
git commit -m "$(cat <<'EOF'
feat: the contribution wall, with a cross-cell selection frame

7 行 = 周日到周六，1 列 = 1 周，自周日起。首列往前补到周日、末列往后补到
周六 —— 不补的话首末两列会整列错位。补齐位不渲染但保留 col/row 占位。

选中框是跨格的一整块绝对定位矩形：框的宽度本身就说明当前框的是日、周
还是月。框外不压暗（聚光灯会让远处月份跌破 3:1 对比度）。

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: `DonutChart` + `MetricRow` — 顶部那条

**Files:**
- Create: `src/components/DonutChart.tsx`
- Create: `src/components/DonutChart.test.tsx`
- Create: `src/components/MetricRow.tsx`
- Create: `src/components/MetricRow.test.tsx`

**Interfaces:**
- Consumes: Task 7 的 `DonutSlice` / `Summary`，Task 8 的 `scaleColor` 不需要（本组件用类别色）
- Produces:
  ```tsx
  interface DonutChartProps { donut: DonutSlice[]; totalMs: number }
  interface MetricRowProps { summary: Summary | null; loading: boolean }
  ```

- [ ] **Step 1: 写 `DonutChart.test.tsx`**

```tsx
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import DonutChart from "./DonutChart";
import type { DonutSlice } from "../types";

const DONUT: DonutSlice[] = [
  { key: "work", ms: 5_700_000 },
  { key: "browsing", ms: 20_200_000 },
  { key: "idle", ms: 7_500_000 },
  { key: "unknown", ms: 20_600_000 },
];

describe("DonutChart", () => {
  it("四档各画一个弧段，环是完整的 360°", () => {
    const { container } = render(<DonutChart donut={DONUT} totalMs={54_000_000} />);
    expect(container.querySelectorAll("[data-arc]")).toHaveLength(4);
  });

  it("中心写监控总时长", () => {
    render(<DonutChart donut={DONUT} totalMs={54_000_000} />);
    expect(screen.getByText("15 时")).toBeTruthy();
  });

  it("图例四行，各带时长与百分比", () => {
    render(<DonutChart donut={DONUT} totalMs={54_000_000} />);
    expect(screen.getByText("工作")).toBeTruthy();
    expect(screen.getByText("11%")).toBeTruthy();   // 5.7 / 54
  });

  it("用类别色而不是连续量色阶", () => {
    // 蓝/橙/灰是类别色；紫阶是连续量的。混用会让「蓝 = work」失效。
    const { container } = render(<DonutChart donut={DONUT} totalMs={54_000_000} />);
    const html = container.innerHTML;
    expect(html).toContain("var(--color-cat-work)");
    expect(html).toContain("var(--color-cat-browsing)");
    expect(html).not.toContain("--color-scale-");
  });

  it("总时长为 0 时不产生 NaN（Review Focus #1）", () => {
    const empty: DonutSlice[] = DONUT.map((d) => ({ key: d.key, ms: 0 }));
    const { container } = render(<DonutChart donut={empty} totalMs={0} />);
    expect(container.innerHTML).not.toContain("NaN");
  });
});
```

- [ ] **Step 2: 写 `MetricRow.test.tsx`**

```tsx
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import MetricRow from "./MetricRow";
import type { Summary } from "../types";

const S: Summary = {
  totalMs: 194_400_000,      // 54h
  activeMs: 167_400_000,     // 46.5h
  idleMs: 27_000_000,        // 7.5h
  segmentCount: 458,
  switchCount: 12,
  hourlyMs: new Array(24).fill(0),
  donut: [
    { key: "work", ms: 5_700_000 },
    { key: "browsing", ms: 20_200_000 },
    { key: "idle", ms: 7_500_000 },
    { key: "unknown", ms: 20_600_000 },
  ],
  topApps: [],
};

describe("MetricRow", () => {
  it("四张卡：总时长 / 活跃·空闲 / 圆环 / 段数·切换", () => {
    render(<MetricRow summary={S} loading={false} />);
    expect(screen.getByText("监控总时长")).toBeTruthy();
    expect(screen.getByText("活跃 / 空闲")).toBeTruthy();
    expect(screen.getByText("活动段")).toBeTruthy();
    expect(container.querySelector("[data-arc]")).toBeTruthy();
  });

  it("段数与切换次数并排", () => {
    render(<MetricRow summary={S} loading={false} />);
    expect(screen.getByText("458 段")).toBeTruthy();
    expect(screen.getByText("切换 12 次")).toBeTruthy();
  });

  it("summary 为 null 时显示加载态而不是崩（Review Focus #1）", () => {
    render(<MetricRow summary={null} loading />);
    expect(screen.getByText(/加载中/)).toBeTruthy();
  });
});
```

（第一段里那行 `container` 需在函数体内 `const { container } = render(...)`，写成：

```tsx
it("四张卡：总时长 / 活跃·空闲 / 圆环 / 段数·切换", () => {
  const { container } = render(<MetricRow summary={S} loading={false} />);
  expect(screen.getByText("监控总时长")).toBeTruthy();
  expect(screen.getByText("活跃 / 空闲")).toBeTruthy();
  expect(screen.getByText("活动段")).toBeTruthy();
  expect(container.querySelector("[data-arc]")).toBeTruthy();
});
```

- [ ] **Step 3: 运行确认失败**

```bash
pnpm vitest run src/components/DonutChart.test.tsx src/components/MetricRow.test.tsx
```

Expected: 失败，`Failed to resolve import`。

- [ ] **Step 4: 写 `DonutChart.tsx`**

```tsx
import type { DonutSlice } from "../types";

interface DonutChartProps {
  donut: DonutSlice[];
  totalMs: number;
}

/** 圆环的 4 档 -> 颜色。**类别色**，不是连续量色阶。 */
const ARC_COLOR: Record<string, string> = {
  work: "var(--color-cat-work)",
  browsing: "var(--color-cat-browsing)",
  idle: "var(--color-cat-idle)",
  unknown: "var(--color-cat-unknown)",
};

const ARC_LABEL: Record<string, string> = {
  work: "工作", browsing: "浏览", idle: "空闲", unknown: "未分类",
};

const R = 28;
const C = 2 * Math.PI * R;

/**
 * 固定四档的圆环。手写 SVG 弧段，不引 d3 —— 四个 arc 用
 * `stroke-dasharray` 就够了（spec §1.2、§6）。
 *
 * 环是完整的 360°：`unknown` 那一档同时兜住「规则没覆盖」和
 * 学习/娱乐/社交/生活（后端已并档）。
 */
export default function DonutChart({ donut, totalMs }: DonutChartProps) {
  const safeTotal = Math.max(totalMs, 0);
  // totalMs 为 0 时全 0 弧段，环空着而不是除以 0
  const fracs = donut.map((d) => (safeTotal > 0 ? d.ms / safeTotal : 0));
  let acc = 0;

  return (
    <div className="flex items-center gap-3">
      <svg viewBox="0 0 64 64" width="64" height="64" className="shrink-0" aria-hidden>
        {fracs.map((frac, i) => {
          const dash = frac * C;
          const el = (
            <circle
              key={donut[i].key}
              data-arc=""
              cx="32" cy="32" r={R} fill="none"
              stroke={ARC_COLOR[donut[i].key]}
              strokeWidth="9"
              strokeDasharray={`${dash} ${C - dash}`}
              strokeDashoffset={-acc * C}
              transform="rotate(-90 32 32)"
            />
          );
          acc += frac;
          return el;
        })}
      </svg>

      <ul className="grid gap-1 text-xs">
        {donut.map((d) => {
          const pct = safeTotal > 0 ? Math.round((d.ms / safeTotal) * 100) : 0;
          return (
            <li key={d.key} className="grid grid-cols-[9px_1fr_auto_auto] items-center gap-2">
              <span aria-hidden className="size-2 rounded-sm"
                    style={{ background: ARC_COLOR[d.key] }} />
              <span className="text-ink">{ARC_LABEL[d.key]}</span>
              <span className="tnum text-ink-faint">{fmt(d.ms)}</span>
              <span className="tnum w-9 text-right text-ink-faint">{pct}%</span>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function fmt(ms: number): string {
  const h = ms / 3_600_000;
  return h >= 1 ? `${Math.round(h * 10) / 10}h` : `${Math.round(ms / 60_000)} 分`;
}
```

- [ ] **Step 5: 写 `MetricRow.tsx`**

```tsx
import DonutChart from "./DonutChart";
import { formatDuration } from "../lib/bucket";
import type { Summary } from "../types";

interface MetricRowProps {
  summary: Summary | null;
  loading: boolean;
}

function Card({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <div className="rounded-lg border border-line bg-surface-1 px-3 py-2">
      <span className="block text-[10.5px] text-ink-faint">{label}</span>
      <b className="tnum block text-[17px] leading-tight font-semibold text-ink">{value}</b>
      {sub && <i className="block text-[10px] text-ink-ghost not-italic">{sub}</i>}
    </div>
  );
}

/** 顶部指标横条。四张卡：总时长 / 活跃·空闲 / 圆环 / 段数·切换。 */
export default function MetricRow({ summary, loading }: MetricRowProps) {
  if (!summary) {
    return <p className="text-sm text-ink-faint">{loading ? "加载中…" : "暂无数据"}</p>;
  }
  const activePct =
    summary.totalMs > 0 ? Math.round((summary.activeMs / summary.totalMs) * 100) : 0;

  return (
    <div className="grid gap-3 lg:grid-cols-4">
      <Card label="监控总时长" value={formatDuration(summary.totalMs)} />
      <Card
        label="活跃 / 空闲"
        value={`${formatDuration(summary.activeMs)} / ${formatDuration(summary.idleMs)}`}
        sub={`${activePct}% 活跃`}
      />
      <div className="rounded-lg border border-line bg-surface-1 px-3 py-2">
        <span className="mb-1 block text-[10.5px] text-ink-faint">类别构成</span>
        <DonutChart donut={summary.donut} totalMs={summary.totalMs} />
      </div>
      <Card
        label="活动段"
        value={`${summary.segmentCount} 段`}
        sub={`切换 ${summary.switchCount} 次`}
      />
    </div>
  );
}
```

- [ ] **Step 6: 运行确认通过**

```bash
pnpm vitest run src/components/DonutChart.test.tsx src/components/MetricRow.test.tsx
```

Expected: 8 passed。

- [ ] **Step 7: 提交**

```bash
git add src/components/DonutChart.tsx src/components/DonutChart.test.tsx src/components/MetricRow.tsx src/components/MetricRow.test.tsx
git commit -m "$(cat <<'EOF'
feat: donut chart and the top metric row

圆环是 4 个 <circle> 加 stroke-dasharray，不引 d3。用类别色不用紫阶 ——
蓝/橙/灰是类别色，混用会让「蓝 = work」这条已建立的语义失效。

totalMs 为 0 时全 0 弧段、环空着，不除以 0。

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 12: `DayPartChart` + `RankedList`

两个简单的展示组件。放一起，因为它们各自的测试都很短。

**Files:**
- Create: `src/components/DayPartChart.tsx`
- Create: `src/components/DayPartChart.test.tsx`
- Create: `src/components/RankedList.tsx`
- Create: `src/components/RankedList.test.tsx`

**Interfaces:**
- Consumes: Task 7 的 `AppSlice` / `MergedTitle`
- Produces:
  ```tsx
  interface DayPartChartProps { hourlyMs: number[] }
  interface RankedItem { label: string; value: string; ratio: number; note?: string }
  interface RankedListProps { title: string; items: RankedItem[]; emptyHint: string }
  ```

- [ ] **Step 1: 写测试**

`src/components/DayPartChart.test.tsx`：

```tsx
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import DayPartChart from "./DayPartChart";

describe("DayPartChart", () => {
  it("画 24 根柱", () => {
    const { container } = render(<DayPartChart hourlyMs={new Array(24).fill(0)} />);
    expect(container.querySelectorAll("[data-bar]")).toHaveLength(24);
  });

  it("按传入值定柱高，最大值满格", () => {
    const h = new Array(24).fill(0);
    h[3] = 9_000_000;
    h[12] = 1_000_000;
    const { container } = render(<DayPartChart hourlyMs={h} />);
    const bars = container.querySelectorAll("[data-bar]");
    const a = Number((bars[3] as HTMLElement).style.height.replace("px", ""));
    const b = Number((bars[12] as HTMLElement).style.height.replace("px", ""));
    expect(a).toBeGreaterThan(b);
  });

  it("全 0 时不产生 NaN（Review Focus #1）", () => {
    const { container } = render(<DayPartChart hourlyMs={new Array(24).fill(0)} />);
    expect(container.innerHTML).not.toContain("NaN");
  });

  it("用中档紫而不是类别色", () => {
    const h = new Array(24).fill(3_600_000);
    const { container } = render(<DayPartChart hourlyMs={h} />);
    expect(container.innerHTML).toContain("--color-scale-3");
    expect(container.innerHTML).not.toContain("--color-cat-");
  });
});
```

`src/components/RankedList.test.tsx`：

```tsx
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import RankedList from "./RankedList";

const ITEMS = [
  { label: "无标题", value: "7086", ratio: 100 },
  { label: "New Tab", value: "3722", ratio: 53 },
];

describe("RankedList", () => {
  it("按给定顺序渲染，不自己重排", () => {
    render(<RankedList title="窗口标题 Top" items={ITEMS} emptyHint="没有数据" />);
    const items = screen.getAllByRole("listitem");
    expect(items[0].textContent).toContain("无标题");
  });

  it("空列表显示提示而不是空白面板", () => {
    // Review Focus #5 的反面：空是因为还没数据，不该是一块什么都没有的面板
    render(<RankedList title="窗口标题 Top" items={[]} emptyHint="没有数据" />);
    expect(screen.getByText("没有数据")).toBeTruthy();
  });

  it("note 字段渲染在标签后面", () => {
    render(
      <RankedList
        title="窗口标题 Top"
        items={[{ label: "客户 42 - [redacted] - Code", value: "120", ratio: 100, note: "已脱敏" }]}
        emptyHint="没有数据"
      />,
    );
    expect(screen.getByText("已脱敏")).toBeTruthy();
  });
});
```

- [ ] **Step 2: 运行确认失败**

```bash
pnpm vitest run src/components/DayPartChart.test.tsx src/components/RankedList.test.tsx
```

Expected: `Failed to resolve import`。

- [ ] **Step 3: 写 `DayPartChart.tsx`**

```tsx
interface DayPartChartProps {
  /** 24 个桶，按本地小时切。只含非 idle 时长。 */
  hourlyMs: number[];
}

const H = 56;   // 绘图区高度
const HOUR_MS = 3_600_000;

/**
 * 24h 活跃时段分布。**24 根细柱，不用平滑面积** ——
 * 平滑曲线会在两个 0 之间鼓出弧，读起来像「凌晨 6 点也在用」，
 * 那是数据里不存在的值（spec §10）。
 *
 * 范围是「全部」时这条几乎是一条直线（5 天里既有熬夜的也有白天的，
 * 一平均就抵消了）。那不是 bug：平是诚实结果，点某天/某周就有波形。
 */
export default function DayPartChart({ hourlyMs }: DayPartChartProps) {
  const max = Math.max(...hourlyMs, 1);
  const h = hourlyMs.length === 24 ? hourlyMs : new Array(24).fill(0);

  return (
    <div>
      <svg viewBox="0 0 720 76" className="w-full" role="img" aria-label="24 小时活跃分布">
        {h.map((ms, hour) => {
          const bh = Math.max((ms / max) * H, ms > 0 ? 1.5 : 0);
          return (
            <rect
              key={hour}
              data-bar=""
              x={hour * 30 + 13.5}
              y={H - bh}
              width="3"
              height={bh}
              rx="1.5"
              fill="var(--color-scale-3)"
            >
              <title>{`${String(hour).padStart(2, "0")}:00 · ${Math.round((ms / HOUR_MS) * 10) / 10}h`}</title>
            </rect>
          );
        })}
        {[0, 6, 12, 18, 24].map((t) => (
          <g key={t}>
            <line x1={(t / 24) * 720} y1={H} x2={(t / 24) * 720} y2={H + 4}
                  stroke="var(--line)" strokeWidth="1" />
            <text x={(t / 24) * 720} y={H + 15} fill="var(--ink-faint)"
                  fontSize="9" textAnchor="middle">{String(t).padStart(2, "0")}</text>
          </g>
        ))}
      </svg>
      <p className="text-[10px] text-ink-faint">24h 时段分布 · 非空闲时长。主打「什么时间在活跃」。</p>
    </div>
  );
}
```

- [ ] **Step 4: 写 `RankedList.tsx`**

```tsx
export interface RankedItem {
  label: string;
  /** 右侧读数，时长或次数，由调用方格式化 */
  value: string;
  /** 0..100，决定条的长度 */
  ratio: number;
  /** 可选后缀，例如「已脱敏」 */
  note?: string;
}

interface RankedListProps {
  title: string;
  items: RankedItem[];
  emptyHint: string;
}

/** 排名列表。应用 Top 与窗口标题 Top 共用 —— 两者是同一形状。 */
export default function RankedList({ title, items, emptyHint }: RankedListProps) {
  return (
    <div className="panel p-3">
      <h2 className="panel-title mb-2">{title}</h2>
      {items.length === 0 ? (
        <p className="text-sm text-ink-faint">{emptyHint}</p>
      ) : (
        <ol className="grid gap-2">
          {items.map((it, i) => (
            <li key={`${it.label}-${i}`} className="grid grid-cols-[minmax(0,1fr)_auto] gap-x-2 gap-y-1">
              <span className="truncate text-xs text-ink" title={it.label}>
                {it.label}
                {it.note && <em className="ml-1.5 not-italic text-[10px] text-state-warning">{it.note}</em>}
              </span>
              <span className="tnum text-xs text-ink-faint">{it.value}</span>
              <span className="col-span-2 block h-[3px] overflow-hidden rounded-full bg-surface-2">
                <i className="block h-full bg-ink-ghost" style={{ width: `${it.ratio}%` }} />
              </span>
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}
```

- [ ] **Step 5: 运行确认通过**

```bash
pnpm vitest run src/components/DayPartChart.test.tsx src/components/RankedList.test.tsx
```

Expected: 7 passed。

- [ ] **Step 6: 提交**

```bash
git add src/components/DayPartChart.tsx src/components/DayPartChart.test.tsx src/components/RankedList.tsx src/components/RankedList.test.tsx
git commit -m "$(cat <<'EOF'
feat: 24h profile bars and a shared ranked list

24 根细柱不用平滑面积：平滑会在两个 0 之间鼓出弧，读起来像「凌晨 6 点
也在用」，那是数据里不存在的值。

Range 选「全部」时这条接近直线 —— 那是诚实结果，不是 bug。

RankedList 抽出来是因为应用 Top 与标题 Top 是同一形状。

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 13: `SummaryPage` — 组装、范围联动、懒加载

整个页面的骨架在这里。**最关键的一条断言**：热力图的数据在范围变化时不重取。

**Files:**
- Modify: `src/views/SummaryPage.tsx`（把 Task 9 的空壳换掉）
- Create: `src/views/SummaryPage.test.tsx`

**Interfaces:**
- Consumes: Task 7 的三个 IPC 封装、Task 8 的 `fillDays` / `rangeFor` / `rangeLabel` / `DateRange`、Task 10/11/12 的组件
- Produces: 完整的 `SummaryPage`

- [ ] **Step 1: 写失败的测试**

`src/views/SummaryPage.test.tsx`：

```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import SummaryPage from "./SummaryPage";
import type { DailyCalendar, Summary } from "../types";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

const CAL: DailyCalendar = {
  first: "2026-10-01",
  last: "2026-10-05",
  days: [
    { date: "2026-10-01", totalMs: 3_600_000 },
    { date: "2026-10-02", totalMs: 13_600_000 },
    { date: "2026-10-03", totalMs: 11_360_000 },
    { date: "2026-10-04", totalMs: 17_320_000 },
    { date: "2026-10-05", totalMs: 2_420_000 },
  ],
};

const SUM: Summary = {
  totalMs: 194_400_000, activeMs: 167_400_000, idleMs: 27_000_000,
  segmentCount: 458, switchCount: 12, hourlyMs: new Array(24).fill(0),
  donut: [
    { key: "work", ms: 5_700_000 }, { key: "browsing", ms: 20_200_000 },
    { key: "idle", ms: 7_500_000 }, { key: "unknown", ms: 20_600_000 },
  ],
  topApps: [{ name: "msedge.exe", ms: 87_200_000 }],
};

function callsTo(cmd: string) {
  return invoke.mock.calls.filter((c) => c[0] === cmd).length;
}

beforeEach(() => {
  invoke.mockReset();
  invoke.mockImplementation((cmd: string) => {
    if (cmd === "get_daily_calendar") return Promise.resolve(CAL);
    if (cmd === "get_summary") return Promise.resolve(SUM);
    if (cmd === "get_top_titles") return Promise.resolve([]);
    return Promise.resolve(null);
  });
});

describe("SummaryPage", () => {
  it("首屏拉日历与指标，但不拉标题排名（懒加载）", async () => {
    render(<SummaryPage />);
    await waitFor(() => expect(callsTo("get_summary")).toBeGreaterThan(0));
    expect(callsTo("get_daily_calendar")).toBe(1);
    expect(callsTo("get_top_titles")).toBe(0);
  });

  it("点格子改范围时热力图不重取", async () => {
    const { container } = render(<SummaryPage />);
    await waitFor(() => expect(callsTo("get_summary")).toBeGreaterThan(0));
    const before = callsTo("get_daily_calendar");

    fireEvent.click(screen.getByTestId("cell-2026-10-03"));
    await waitFor(() => expect(callsTo("get_summary")).toBe(2));

    expect(callsTo("get_daily_calendar")).toBe(before);
    expect(container.querySelector("[data-frame]")).toBeTruthy();
  });

  it("点格子后 get_summary 收到那一天", async () => {
    render(<SummaryPage />);
    await waitFor(() => expect(callsTo("get_summary")).toBeGreaterThan(0));
    fireEvent.click(screen.getByTestId("cell-2026-10-03"));
    await waitFor(() => {
      const args = invoke.mock.calls.filter((c) => c[0] === "get_summary").at(-1)![1];
      expect(args).toMatchObject({ from: "2026-10-03", to: "2026-10-03" });
    });
  });

  it("点已选中的同一格回到全部", async () => {
    render(<SummaryPage />);
    await waitFor(() => expect(callsTo("get_summary")).toBeGreaterThan(0));
    fireEvent.click(screen.getByTestId("cell-2026-10-03"));
    await waitFor(() => expect(callsTo("get_summary")).toBe(2));
    fireEvent.click(screen.getByTestId("cell-2026-10-03"));
    await waitFor(() => {
      const args = invoke.mock.calls.filter((c) => c[0] === "get_summary").at(-1)![1];
      expect(args).toMatchObject({ from: "2026-10-01", to: "2026-10-05" });
    });
  });

  it("空日历显示空状态，不画墙也不崩（Review Focus #1）", async () => {
    invoke.mockImplementation((cmd: string) =>
      Promise.resolve(cmd === "get_daily_calendar" ? null : null));
    const { container } = render(<SummaryPage />);
    await waitFor(() => expect(screen.getByText(/还没有采集数据/)).toBeTruthy());
    expect(container.innerHTML).not.toContain("NaN");
  });

  it("get_summary 失败时显示错误而不是白屏", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "get_daily_calendar") return Promise.resolve(CAL);
      if (cmd === "get_summary") return Promise.reject(new Error("no db"));
      return Promise.resolve([]);
    });
    render(<SummaryPage />);
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
  });

  it("应用 Top 用 get_summary 带来的数据，不需要额外请求", async () => {
    render(<SummaryPage />);
    await waitFor(() => expect(screen.getByText("msedge.exe")).toBeTruthy());
    expect(callsTo("get_top_titles")).toBe(0);
  });

  it("范围 chip 存在但不触发任何请求（本轮是死控件）", async () => {
    render(<SummaryPage />);
    await waitFor(() => expect(callsTo("get_summary")).toBeGreaterThan(0));
    const n = invoke.mock.calls.length;
    fireEvent.click(screen.getByTestId("range-chip"));
    expect(invoke.mock.calls.length).toBe(n);
  });
});
```

- [ ] **Step 2: 运行确认失败**

```bash
pnpm vitest run src/views/SummaryPage.test.tsx
```

Expected: 失败（当前是空壳，没有 `cell-2026-10-03` 这些 testid）。

- [ ] **Step 3: 写实现**

把 `src/views/SummaryPage.tsx` 整个换掉：

```tsx
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import ContributionWall from "../components/ContributionWall";
import DayPartChart from "../components/DayPartChart";
import MetricRow from "../components/MetricRow";
import RankedList, { type RankedItem } from "../components/RankedList";
import { fillDays, rangeFor, rangeLabel, type DateRange, type RangeKind } from "../lib/summary";
import { formatDuration } from "../lib/bucket";
import {
  getDailyCalendar, getSummary, getTopTitles,
  type DayCell, type MergedTitle, type Summary,
} from "../types";

/** 标题排名的请求条数。 */
const TITLE_LIMIT = 10;

/**
 * 汇总页。
 *
 * 范围状态只存在于这里，**不写 URL、不持久化**（spec §2.3）——
 * 这是单机桌面应用，不是可分享的网页。
 *
 * 三段布局（spec §2.2）：指标横条 / 热力图整宽（含 24h 条）/ 底部双栏。
 * 段内不各自滚，滚动交给整页。
 */
export default function SummaryPage() {
  // `all` 是热力图覆盖的整个数据区间，热力图只渲染它，永不变。
  const [all, setAll] = useState<DateRange | null>(null);
  const [kind, setKind] = useState<RangeKind>("all");
  const [picked, setPicked] = useState<string>("");   // 点中的那一天

  const [days, setDays] = useState<DayCell[]>([]);
  const [summary, setSummary] = useState<Summary | null>(null);
  const [summaryError, setSummaryError] = useState(false);
  const [titles, setTitles] = useState<MergedTitle[]>([]);
  const [titleBusy, setTitleBusy] = useState(false);

  // ① 热力图数据：挂载时取一次，与 range 无关。
  useEffect(() => {
    let dead = false;
    getDailyCalendar()
      .then((cal) => {
        if (dead) return;
        if (!cal) {
          setAll(null);
          return;
        }
        setAll({ kind: "all", from: cal.first, to: cal.last, label: "全部" });
        setDays(fillDays(cal.first, cal.last, new Map(cal.days.map((d) => [d.date, d.totalMs]))));
      })
      .catch(() => { if (!dead) setSummaryError(true); });
    return () => { dead = true; };
  }, []);

  const range: DateRange | null = useMemo(
    () => (all ? rangeFor(kind, picked || all.to, all) : null),
    [all, kind, picked],
  );

  // ② 顶部指标：随 range 变。
  useEffect(() => {
    if (!range) return;
    let dead = false;
    setSummaryError(false);
    getSummary(range.from, range.to)
      .then((s) => { if (!dead) setSummary(summaryOrEmpty(s)); })
      .catch(() => { if (!dead) { setSummaryError(true); setSummary(null); } });
    return () => { dead = true; };
  }, [range?.from, range?.to]);

  // ③ 标题排名：懒加载。滚到底部才发这一发（spec §3.3）。
  const bottomRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!range) return;
    const el = bottomRef.current;
    if (!el || typeof IntersectionObserver === "undefined") return;
    let dead = false;
    const io = new IntersectionObserver((entries) => {
      if (!entries.some((e) => e.isIntersecting) || dead) return;
      setTitleBusy(true);
      getTopTitles(range.from, range.to, TITLE_LIMIT)
        .then((t) => { if (!dead) setTitles(t); })
        .catch(() => { if (!dead) setTitles([]); })
        .finally(() => { if (!dead) setTitleBusy(false); });
      io.disconnect();
    }, { rootMargin: "200px" });
    io.observe(el);
    return () => { dead = true; io.disconnect(); };
  }, [range?.from, range?.to]);

  const select = useCallback((k: RangeKind) => (date: string) => {
    setKind((cur) => (cur === k && picked === date ? "all" : k));
    setPicked(date);
  }, [picked]);

  // 标题次数不换算成时长——那是「出现次数」，说成时间会骗人。
  const titleItems: RankedItem[] = useMemo(() => {
    if (titles.length === 0) return [];
    const max = Math.max(...titles.map((t) => t.hits), 1);
    return titles.map((t) => ({
      label: t.title,
      value: `${t.hits} 次`,
      ratio: Math.round((t.hits / max) * 100),
      note: t.redacted ? "已脱敏" : undefined,
    }));
  }, [titles]);
  const titleEmpty = titleBusy
    ? "统计中…"
    : titles.length === 0 ? "向下滚动加载" : "这个范围没有记录";

  const apps: RankedItem[] = useMemo(() => {
    if (!summary || summary.topApps.length === 0) return [];
    const max = Math.max(...summary.topApps.map((a) => a.ms), 1);
    return summary.topApps.map((a) => ({
      label: a.name,
      value: formatDuration(a.ms),
      ratio: Math.round((a.ms / max) * 100),
    }));
  }, [summary]);

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-3">
      <div className="flex shrink-0 flex-wrap items-center gap-2">
        <h1 className="m-0 shrink-0 text-label font-semibold tracking-wide whitespace-nowrap text-ink-muted">
          Time Scope
        </h1>
        {/* 本轮是死控件：范围由热力图上的点击驱动（spec §1.2） */}
        <button
          type="button"
          data-testid="range-chip"
          className="cursor-default rounded-md border border-line bg-surface-1 px-2.5 py-1 text-sm text-ink-faint"
        >
          范围：{range ? rangeLabel(range) : "—"}
        </button>
      </div>

      {summaryError && (
        <div role="alert" className="flex items-start gap-2 rounded-md border border-line bg-surface-1 p-3 text-sm text-ink">
          <span>加载失败。请确认后端已启动（<code className="tnum">pnpm tauri dev</code>）。</span>
        </div>
      )}

      <MetricRow summary={summary} loading={!summary && !summaryError} />

      <section className="panel p-4" aria-label="监控时长热力图">
        <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
          <span className="panel-title tnum">
            监控时长 · {all ? `${all.from} – ${all.to}` : "暂无"}
          </span>
          <span className="text-micro text-ink-faint">少 ░▒▓█ 多</span>
        </div>
        <ContributionWall
          days={days}
          selection={kind === "all" ? null : range}
          onSelectDay={select("day")}
          onSelectWeek={select("week")}
          onSelectMonth={select("month")}
        />
        <div className="mt-3 border-t border-line pt-3">
          <DayPartChart hourlyMs={summary?.hourlyMs ?? []} />
        </div>
      </section>

      <div className="grid flex-1 grid-cols-1 gap-4 lg:grid-cols-2" ref={bottomRef}>
        <RankedList title="窗口标题 Top" items={titleItems} emptyHint={titleEmpty} />
        <RankedList title="应用 Top" items={apps} emptyHint="这个范围没有记录" />
      </div>
    </div>
  );
}

/** 后端可能返回缺字段的形状（老库/半成品），这里补齐而不是让组件崩。 */
function summaryOrEmpty(s: Summary): Summary {
  return { ...s, hourlyMs: s.hourlyMs ?? [], donut: s.donut ?? [], topApps: s.topApps ?? [] };
}
```

- [ ] **Step 4: 运行确认通过**

```bash
pnpm vitest run src/views/SummaryPage.test.tsx
```

Expected: 9 passed。

`点格子改范围时热力图不重取` 是这一页最重要的一条。它挂了说明 `get_daily_calendar`
的 `useEffect` 依赖里混进了 `range`。

- [ ] **Step 5: 跑全量**

```bash
pnpm test
pnpm build
```

Expected: 全绿，tsc 无报错。

- [ ] **Step 6: 提交**

```bash
git add src/views/SummaryPage.tsx src/views/SummaryPage.test.tsx
git commit -m "$(cat <<'EOF'
feat: the summary page — three data layers, range-driven metrics

热力图的数据挂载时取一次，与范围无关；范围只驱动顶部指标和标题排名。
标题排名懒加载到面板进入视口（它是最贵的那一项，默认范围又是「全部」）。

范围 chip 存在但不发请求：真实的范围切换由热力图上的点击驱动。

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 14: 全量验证与视觉预览

不加新功能。这一步是把前面 13 个任务的结果对着 spec §8 的检查表过一遍。

**Files:**
- Create: `src/preview-summary.test.tsx`
- Modify: `docs/superpowers/STATUS.md`

**Interfaces:**
- Consumes: 前 13 个任务的全部产物
- Produces: 无新接口

- [ ] **Step 1: 加视觉预览测试**

照抄 `src/preview.test.tsx` 的结构（依赖 `dist/assets` 的 CSS，没 build 就自动 skip——
这是既有约定，**不是坏测试**，见 STATUS ⛔ #8）。造**最坏情况**的数据：
125 个 1–3 分钟的碎片段、跨 5 个月、含 4 天完全没活动。

```tsx
/**
 * 汇总页的视觉预览（不是断言）。
 *
 * 造**最坏情况**的数据：碎片化、跨 5 个月、含 4 天空白 ——
 * STATUS §4.6 记着「用 mock 数据验收 UI」的教训：干净数据下设计的界面
 * 在真实场景里不可读。
 *
 * 依赖 `dist/assets` 的 CSS，没 `pnpm build` 会自动 skip。
 */
import { describe, it, beforeAll } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { existsSync } from "node:fs";
import SummaryPage from "./views/SummaryPage";
import type { DayCell } from "./types";
```

**注意**：`SummaryPage` 在这个预览里会发 IPC，而 `renderToStaticMarkup` 是同步的。
把预览改成直接渲染四个组件（不带数据获取）：

```tsx
import ContributionWall from "./components/ContributionWall";
import DayPartChart from "./components/DayPartChart";
import MetricRow from "./components/MetricRow";
import RankedList from "./components/RankedList";

const DAYS: DayCell[] = makeWorstCase();
```

`makeWorstCase()` 造 5 个月、每工作日 12–20 个 1–3 分钟碎片、跳过 4 天。
组件按 Task 13 的 `SummaryPage` 里的同一套 props 组合渲染。

- [ ] **Step 2: 跑四条验证**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace
pnpm test
pnpm build
node design-system/time-scope/verify-palette.mjs
```

Expected: 四条全过。`verify-palette.mjs` 必须 exit 0。

Rust 测试数应比实施前多 **32 条**（Task 1 的 8 条 + Task 2 的 5 条 + Task 3 的 6 条 +
Task 4 的 6 条 + Task 5 的 12 条，其中 storage 侧 6 条、app 侧 6 条）。
前端测试数应比实施前多 **50 条** 左右。若对不上，多半是某条测试被合并或漏写。

- [ ] **Step 3: 截图看效果**

```bash
pnpm build
pnpm vitest run src/preview-summary.test.tsx
```

生成的 HTML 用 headless Chrome 打开，对照 spec §8.4 的清单逐条看：

- 三段布局在 700px 高下不溢出
- 热力图 5 个月的数据下周日起始、补齐位不可见、跨月列距正确
- 选中框在日/周/月三档下的尺寸明显不同
- 24h 条的柱高能拉开（**用眼睛定这四档**：0.2/0.4/0.6/0.8 拉得开不开）
- 圆环四档的颜色与图例对得上，未分类那一块是不是灰得刺眼
- 深浅两套主题各看一遍

**色阶不合适就在这里改** `src/lib/summary.ts` 的 `scaleStep` 四个阈值，
改完重跑 `pnpm test` 与 `pnpm build`（spec §5.2 明说这几个数是初值）。

- [ ] **Step 4: 更新 `STATUS.md`**

在「⛔ 别动这里」一节里加一条，把这轮定下来的结构决策钉住（防后人重犯）：

```markdown
| 14 | 热力图只有 5 档、0 档是轨道色，看着像「少了 6 档」 | **0 档是「0 活跃」**，就该看起来是空的。与 MASTER §2.5「没有活动的桶不画，露出轨道色」同一处理 | 别给 0 档也上紫阶 |
| 15 | 选中框的宽窄是随手调的 | **框宽 = 跨的格数**（日 1 / 周 1 列 / 月 N 列），框的宽窄就是档位标识 | 别改成逐格描边 |
```

在「二、代码规模与状态」里更新 Rust / TS 文件数与测试数。

- [ ] **Step 5: 提交**

```bash
git add src/preview-summary.test.tsx docs/superpowers/STATUS.md
git commit -m "$(cat <<'EOF'
docs: visual preview for the summary page, and pin two layout decisions

预览造最坏情况的数据：碎片化、跨 5 个月、含 4 天空白。干净数据下设计的
界面在真实场景里不可读 —— 这是 Phase 1 UI 那轮踩过的坑。

STATUS 加两条：0 档是轨道色不是「少了 6 档」；选中框宽窄是档位标识不是随手调的。

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
)"
```

---

## 附：实施后要回填的文档

- [ ] `README.md` 的「还没做什么」表删掉「设置界面」那一行的**热力图相关部分**
      （设置页本身仍未实现，别整行删）
- [ ] `docs/superpowers/specs/2026-10-05-summary-page-design.md` 加一节
      「实施后修订记录」，记下与 spec 的分歧（按既有 spec §19 的格式）
- [ ] 若色阶阈值在 Task 14 调过，spec §5.2 的 `0.2/0.4/0.6/0.8` 同步改成实际值
