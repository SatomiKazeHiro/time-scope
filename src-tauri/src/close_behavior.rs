//! 关窗行为的纯决策（spec §4.2）。
//!
//! 平台交互（弹窗、隐藏、退出）都在 `residency.rs`；这里只回答"该做什么"，
//! 于是"什么时候问、之后按什么执行"这两条规则可以在 CI 里验证。

use crate::config::CloseBehavior;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    /// 弹确认框问用户
    Ask,
    /// 隐藏到托盘，继续采集
    HideToTray,
    /// 退出
    Quit,
}

/// `ask_supported`：当前平台/上下文是否支持弹确认框。false 时必须能退出，
/// 否则用户会卡在"窗口关不掉"的状态。
pub fn decide(current: CloseBehavior, ask_supported: bool) -> CloseDecision {
    match current {
        CloseBehavior::Ask if ask_supported => CloseDecision::Ask,
        // 问不了就退出：宁可少一个功能，不能把用户困住
        CloseBehavior::Ask => CloseDecision::Quit,
        CloseBehavior::Minimize => CloseDecision::HideToTray,
        CloseBehavior::Quit => CloseDecision::Quit,
    }
}

// 这里曾经有一个 `remember(answered)`：把用户的选择规范化后交回去写 config。
// 2026-10-15 连同它的写回一起删了 —— 设置页刚选的值会被弹框静默改掉。
// 答案怎么执行归 residency.rs 的 `apply_answer`，且**只管这一次**。

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CloseBehavior;

    #[test]
    fn ask_shows_a_prompt() {
        assert_eq!(decide(CloseBehavior::Ask, true), CloseDecision::Ask);
    }

    #[test]
    fn minimize_hides_without_asking() {
        assert_eq!(
            decide(CloseBehavior::Minimize, true),
            CloseDecision::HideToTray
        );
    }

    #[test]
    fn quit_exits_without_asking() {
        assert_eq!(decide(CloseBehavior::Quit, true), CloseDecision::Quit);
    }

    #[test]
    fn when_the_platform_cannot_ask_fall_back_to_quit() {
        // 问不了就必须给用户一条出路，否则窗口关不掉
        assert_eq!(decide(CloseBehavior::Ask, false), CloseDecision::Quit);
    }
}
