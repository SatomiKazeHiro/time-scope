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
}
