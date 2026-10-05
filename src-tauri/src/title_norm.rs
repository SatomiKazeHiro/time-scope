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

/// 剥掉**前导**状态符。锚在 `^` 上——标题中间的 `⏳` 不动，
/// 因为放宽锚点会连带吃掉 `main.rs - Visual Studio Code` 的开头。
static LEADING_SYMBOL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[\s◐◑◒◓●○◉⭐*+!\-]+").expect("编译期常量正则")
});

/// 剥掉 Edge 标题尾部的 profile + 浏览器名：` - 个人 - Microsoft Edge`
/// （折叠标签组是 ` - 个人`，普通标签页后面还跟着浏览器名，两种都要吃掉）
static EDGE_PROFILE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\s*-\s*(个人|工作|私人)(?:\s*-\s*Microsoft\s*Edge\s*)?$")
        .expect("编译期常量正则")
});

/// 连续空白压成一个
static WHITESPACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+").expect("编译期常量正则"));

/// 归一化一条窗口标题。返回 `None` 表示「剥完没有内容」，调用方应跳过它——
/// 空串会占掉 Top N 的一个坑。
///
/// 规则按顺序应用（spec §4.1），**顺序不能换**：先剥计数器再剥前导符，
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
    // hits 降序；相同时按标题字典序，让顺序在多次刷新间稳定
    out.sort_by(|a, b| b.hits.cmp(&a.hits).then_with(|| a.title.cmp(&b.title)));
    out.truncate(limit);
    out
}

#[cfg(test)]
mod tests {
    use super::{merge_top_titles, normalize_title};

    fn n(s: &str) -> String {
        normalize_title(s).expect("应归一化出非空结果")
    }

    #[test]
    fn edge_tab_group_counter_is_stripped() {
        assert_eq!(n("无标题 和另外 31 个页面 - 个人 - Microsoft Edge"), "无标题");
    }

    #[test]
    fn edge_new_tab_counter_is_stripped() {
        assert_eq!(
            n("New Tab 和另外 33 个页面 - 个人 - Microsoft Edge"),
            "New Tab"
        );
    }

    #[test]
    fn leading_status_symbol_is_stripped() {
        assert_eq!(n("◐ 项目与 uv Python 管理"), "项目与 uv Python 管理");
        assert_eq!(n("◑ 项目与 uv Python 管理"), "项目与 uv Python 管理");
    }

    #[test]
    fn real_code_title_passes_through_unchanged() {
        // 最关键的一条：规则只吃前导状态符和尾部配置名，中间的 " - " 不许碰。
        assert_eq!(
            n("main.rs - Visual Studio Code"),
            "main.rs - Visual Studio Code"
        );
        assert_eq!(
            n("registry.ts - time-scope - Visual Studio Code"),
            "registry.ts - time-scope - Visual Studio Code"
        );
    }

    #[test]
    fn middle_status_symbol_is_kept() {
        // 规则 2 只吃前导。中间的吃不到是有意的（spec §4.2），
        // 这条测试守着「不要为了多剥一点而放宽锚点」。
        assert_eq!(
            n("⏳ 待处理 · 了解该项目概况"),
            "⏳ 待处理 · 了解该项目概况"
        );
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
        // 前 10 条原始行全都塌成同一个 "A"，第 11 条起才是互不相同的 B/C/D…，
        // 而且 hits 更高（所以它们本该排进 Top 10）。
        //  - 先合并再截断：11 个桶，取前 10 -> 10 条，含 B..K
        //  - 先截断再合并：前 10 条塌成 1 桶 -> 1 条，什么名次都丢了
        let collapse_hits: i64 = (0..10).map(|i| 1000 - i).sum();
        let mut raw: Vec<(String, i64)> = (0..10)
            .map(|i| (format!("无标题 和另外 {i} 个页面"), 1000 - i))
            .collect();
        for (i, name) in ["B", "C", "D", "E", "F", "G", "H", "I", "J", "K"]
            .iter()
            .enumerate()
        {
            raw.push((name.to_string(), 5000 + i as i64));
        }

        let top = merge_top_titles(raw, 10);
        assert_eq!(top.len(), 10, "先合并才有 10 个桶可取");
        // 10 条塌成的一桶累加了全部 hits，合计远大于任何单条 B..K，
        // 所以它排第一本身就是「合并真的发生了」的证据。
        assert_eq!(top[0].title, "无标题");
        assert_eq!(top[0].hits, collapse_hits);
        assert!(
            top.iter().any(|t| t.title == "K"),
            "截断若发生在合并之前，第 11~20 条原始行永远进不来"
        );
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
        assert_eq!(top[0].hits, 3);
    }

    #[test]
    fn merge_marks_redacted_titles() {
        // 归一化不碰占位符，所以这里能直接判
        let raw = vec![("客户 42 - [redacted] - Code".to_string(), 7)];
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
        assert_eq!(top[1].title, "New Tab");
    }

    #[test]
    fn merge_respects_limit() {
        let raw: Vec<(String, i64)> = (0..50)
            .map(|i| (format!("t{i}"), 1000 - i))
            .collect();
        assert_eq!(merge_top_titles(raw, 10).len(), 10);
    }

    #[test]
    fn merge_ties_break_on_title_for_stable_order() {
        // 同一 hits 时按标题字典序排，否则每次刷新顺序会跳
        let raw = vec![
            ("b".to_string(), 5),
            ("a".to_string(), 5),
            ("c".to_string(), 9),
        ];
        let top = merge_top_titles(raw, 10);
        assert_eq!(top.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(),
                   vec!["c", "a", "b"]);
    }
}
