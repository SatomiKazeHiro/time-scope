//! collector 内部信号：从 OS 采集线程传给 consumer 线程的最小载荷。
//!
//! 只传 HWND（窗口事件）或一个数字（输入事件），**不在这一层取标题/进程名**——
//! 那是重活，留给 consumer 线程（spec §5.1）。

#[cfg(windows)]
use windows::Win32::Foundation::HWND;

#[derive(Debug, Clone, Copy)]
pub enum RawSignal {
    /// 前台窗口切换
    #[cfg(windows)]
    WindowFocus(HWND),
    /// 同一窗口标题变化（浏览器切 Tab）
    #[cfg(windows)]
    WindowTitleChange(HWND),
    /// 无输入超过 idle_threshold
    IdleStart,
    /// 从 idle 恢复到有输入
    InputActive,
    /// 一个 heartbeat 窗口内的活跃秒数（0..=heartbeat_every_s）
    Heartbeat(u8),
    /// 非 Windows 平台的占位，使枚举在所有平台可编译
    #[cfg(not(windows))]
    _Noop,
}

// `HWND` 是 `*mut c_void`，而裸指针没有 `Send`。但它只是一个不透明的窗口句柄值：
// 采集线程只是把“哪个窗口变了”这个事实传出去，真正调用 Win32 API 的是 consumer
// 线程，而那些 API 本身对 HWND 就是跨线程安全的（前提是窗口仍存在——取不到信息时
// `window_info` 返回 None）。所以这里显式声明 Send。
//
// 注意：这不是在共享可变状态，只是传一个整数级别的句柄。
unsafe impl Send for RawSignal {}
