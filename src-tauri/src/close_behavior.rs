//! 关窗行为的纯决策（spec §4.2）。
//!
//! 平台交互（弹窗、隐藏、退出）都在 `tray.rs`；这里只回答"该做什么"，
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

/// 用户在确认框里的选择，用来覆写 config。
/// `Ask` 一旦被回答就变成 `Minimize` —— 默认选项是"最小化"，因为托盘常驻
/// 是这个应用的主要形态，而误退出反而会让人以为数据丢了。
pub fn remember(answered: CloseBehavior) -> CloseBehavior {
    match answered {
        CloseBehavior::Ask => CloseBehavior::Minimize,
        other => other,
    }
}

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

    #[test]
    fn remembering_an_answer_never_leaves_ask() {
        // Review Focus #2：写回失败也不能让"每次都问"
        assert_eq!(remember(CloseBehavior::Ask), CloseBehavior::Minimize);
        assert_eq!(remember(CloseBehavior::Minimize), CloseBehavior::Minimize);
        assert_eq!(remember(CloseBehavior::Quit), CloseBehavior::Quit);
    }
}
