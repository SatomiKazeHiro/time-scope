//! 托盘图标、菜单与关窗拦截（spec §4）。
//!
//! 菜单结构与菜单事件的解释都是**纯数据/纯函数**，可在 CI 里验证；
//! 真正建图标、弹窗、隐藏窗口是平台交互，放在 `residency.rs`，交给人工验证清单。

/// 托盘菜单项。抽成数据是为了能单测"菜单里有什么"，
/// 而不是只能靠肉眼看托盘图标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayMenuItem {
    /// 打开时间线（显示主窗口并置前）
    Open,
    /// 开机自启，带当前勾选态
    ToggleAutostart { enabled: bool },
    /// 显式退出（走 flush）
    Quit,
}

pub fn build_menu(autostart_on: bool) -> Vec<TrayMenuItem> {
    vec![
        TrayMenuItem::Open,
        TrayMenuItem::ToggleAutostart { enabled: autostart_on },
        TrayMenuItem::Quit,
    ]
}

pub fn menu_item_id(item: &TrayMenuItem) -> &'static str {
    match item {
        TrayMenuItem::Open => "open",
        TrayMenuItem::ToggleAutostart { .. } => "autostart",
        TrayMenuItem::Quit => "quit",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuOutcome {
    /// 显示主窗口并置前
    FocusWindow,
    /// 切自启（值是切换后的目标状态）
    SetAutostart(bool),
    /// 退出
    Quit,
    /// 认不出来的 id：忽略，绝不 panic
    Ignored,
}

/// 解释一次菜单点击。自启项是**切换**语义而不是"设为某值"，
/// 免得菜单与实际状态不同步时越点越乱。
pub fn on_menu_event(autostart_now: bool, id: &str) -> MenuOutcome {
    match id {
        "open" => MenuOutcome::FocusWindow,
        "autostart" => MenuOutcome::SetAutostart(!autostart_now),
        "quit" => MenuOutcome::Quit,
        _ => MenuOutcome::Ignored,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_has_exactly_three_items() {
        let m = build_menu(false);
        assert_eq!(m.len(), 3);
        assert!(matches!(m[0], TrayMenuItem::Open));
        assert!(matches!(m[1], TrayMenuItem::ToggleAutostart { enabled: false }));
        assert!(matches!(m[2], TrayMenuItem::Quit));
    }

    #[test]
    fn the_autostart_item_reflects_the_current_state() {
        assert!(build_menu(true)
            .iter()
            .any(|i| matches!(i, TrayMenuItem::ToggleAutostart { enabled: true })));
    }

    #[test]
    fn every_item_has_a_distinct_id() {
        let ids: Vec<&str> = build_menu(false).iter().map(menu_item_id).collect();
        let mut uniq = ids.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(ids.len(), uniq.len());
    }

    #[test]
    fn opening_maps_to_focus() {
        assert_eq!(on_menu_event(false, "open"), MenuOutcome::FocusWindow);
    }

    #[test]
    fn the_autostart_item_toggles_rather_than_sets() {
        assert_eq!(on_menu_event(false, "autostart"), MenuOutcome::SetAutostart(true));
        assert_eq!(on_menu_event(true, "autostart"), MenuOutcome::SetAutostart(false));
    }

    #[test]
    fn quit_maps_to_quit() {
        assert_eq!(on_menu_event(false, "quit"), MenuOutcome::Quit);
    }

    #[test]
    fn an_unknown_id_is_ignored_rather_than_crashing() {
        assert_eq!(on_menu_event(false, "???"), MenuOutcome::Ignored);
        assert_eq!(on_menu_event(false, ""), MenuOutcome::Ignored);
    }
}
