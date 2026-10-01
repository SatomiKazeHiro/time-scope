//! 单实例守卫（spec §12）。
//!
//! 没有它时，第二次启动会开出第二个进程，两个进程同时写同一个 SQLite 文件——
//! 实测表现为 WebView2 报 `HRESULT(0x800700AA) 请求的资源在使用中` 而崩溃。
//!
//! 关键顺序：`single_instance` 插件必须**第一个**注册。它靠抢一个全局锁来判定
//! "是不是第一个实例"，而 Tauri 的窗口/webview 初始化发生在插件之后；顺序反了
//! 的话第二个进程会先把 webview 建起来再发现自己是多余的。
//!
//! 这里只保留可测的纯逻辑，真正的进程间互斥由插件在 `main.rs` 里完成。

/// 第二个实例启动时应采取的动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondInstanceAction {
    /// 把已有实例的窗口叫醒并置前，然后自己退出
    FocusExisting,
}

/// 算出"另一个实例带着这些参数启动了"时该怎么办。
///
/// 永远只做 [`SecondInstanceAction::FocusExisting`]：本应用没有多窗口/多文档模型，
/// 第二个实例带不带参数都一样——把第一个窗口叫醒即可。
/// 参数本身不丢弃是为了不给未来留坑：如果将来支持"用命令行打开某天"，
/// 这里改成转发即可。
pub fn on_second_instance(_argv: &[String]) -> SecondInstanceAction {
    SecondInstanceAction::FocusExisting
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_instance_always_focuses_the_existing_one() {
        assert_eq!(on_second_instance(&[]), SecondInstanceAction::FocusExisting);
    }

    #[test]
    fn arguments_do_not_change_the_decision() {
        // 目前没有命令行语义，带不带参都只是"叫醒第一个"
        let with_args = vec!["--date".to_string(), "2026-10-02".to_string()];
        assert_eq!(on_second_instance(&with_args), SecondInstanceAction::FocusExisting);
    }
}
