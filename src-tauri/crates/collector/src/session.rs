//! 锁屏 / 睡眠监听（spec §3）。
//!
//! 两条 Win32 通路，**都必须挂在一个窗口 HWND 上**：
//! - `WTSRegisterSessionNotification` —— 锁屏 / 解锁（窗口消息 `WM_WTSSESSION_CHANGE`）
//! - `PowerRegisterSuspendResumeNotification` —— 合盖 / 唤醒（`WM_POWERBROADCAST`）
//!
//! 这与现有的采集线程不同：窗口 hook 和输入轮询都可以纯后台线程起，
//! 但这两个通知只能绑窗口。所以 `register` 由 app 层在拿到主窗口 hwnd 后调用，
//! 消息的处理则在 app 层的窗口回调里调 [`translate`]。

use crate::signals::RawSignal;
use std::sync::mpsc::Sender;

/// 窗口消息号（winuser.h）。用自己写的常量而不是从 windows crate 取，
/// 是为了让本模块的纯逻辑部分不必依赖 windows crate；`tests/session.rs`
// 里有一条测试把它们钉在 Win32 的真值上。
pub const WM_WTSSESSION_CHANGE: u32 = 0x02B1;
pub const WM_POWERBROADCAST: u32 = 0x0218;

// WTS 事件码（winuser.h 的 WTS_SESSION_*）
const WTS_SESSION_LOCK: usize = 0x7;
const WTS_SESSION_UNLOCK: usize = 0x8;

// PBT 事件码（winuser.h 的 PBT_APM*）
const PBT_APMSUSPEND: usize = 0x0004;
const PBT_APMRESUME: usize = 0x0007;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationOutcome {
    Ok,
    /// 注册失败。**只降级锁屏采集，不影响其余功能**（spec §7）。
    Failed { reason: &'static str },
}

/// 纯函数：把 Win32 的 Result 转成可断言的结论。
pub fn registration_outcome(raw: Result<(), &'static str>) -> RegistrationOutcome {
    match raw {
        Ok(()) => RegistrationOutcome::Ok,
        Err(reason) => RegistrationOutcome::Failed { reason },
    }
}

/// 把一条窗口消息翻译成采集信号。**纯函数**。
///
/// `wparam` 对两种消息都带着事件码（WTS_* 或 PBT_APM*），无关消息返回 `None`。
pub fn translate(msg: u32, wparam: usize) -> Option<RawSignal> {
    match msg {
        WM_WTSSESSION_CHANGE => match wparam {
            WTS_SESSION_LOCK => Some(RawSignal::SessionLock),
            WTS_SESSION_UNLOCK => Some(RawSignal::SessionUnlock),
            _ => None,
        },
        WM_POWERBROADCAST => match wparam {
            // 合盖记成锁屏：唤醒事件会晚于实际开盖，但中间那段仍是一个正确的
            // idle 段，比"合盖 30 分钟什么都不产生"要好（spec §3.3）
            PBT_APMSUSPEND => Some(RawSignal::SessionLock),
            PBT_APMRESUME => Some(RawSignal::SessionUnlock),
            _ => None,
        },
        _ => None,
    }
}

/// 在主窗口上注册锁屏/睡眠监听。
///
/// 失败不 panic：返回 `Failed` 让调用方记一行 stderr 并继续
/// （spec §7：常驻增强失败不能拖垮主链路）。
///
/// `sender` 供 app 层在窗口消息回调里投递信号用；注册本身不需要它，
/// 但把它收在这里可以让调用方只拿一个句柄。
#[cfg(windows)]
pub fn register(hwnd_raw: isize, _sender: Sender<RawSignal>) -> RegistrationOutcome {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Power::PowerRegisterSuspendResumeNotification;
    use windows::Win32::System::RemoteDesktop::{
        WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
    };
    use windows::Win32::UI::WindowsAndMessaging::DEVICE_NOTIFY_WINDOW_HANDLE;

    let hwnd = HWND(hwnd_raw as *mut std::ffi::c_void);

    if unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) }.is_err() {
        let reason = "WTSRegisterSessionNotification 失败";
        eprintln!("[time-scope] {reason}：锁屏/解锁将不被记录，其余功能正常");
        return RegistrationOutcome::Failed { reason };
    }

    let mut handle: *mut std::ffi::c_void = std::ptr::null_mut();
    let power = unsafe {
        PowerRegisterSuspendResumeNotification(DEVICE_NOTIFY_WINDOW_HANDLE, hwnd, &mut handle)
    };
    // windows 0.58 里这个函数返回 WIN32_ERROR（0 = 成功），不是 HANDLE
    if power.to_hresult().is_err() {
        let reason = "PowerRegisterSuspendResumeNotification 失败";
        eprintln!("[time-scope] {reason}：合盖/唤醒将不被记录，其余功能正常");
        return RegistrationOutcome::Failed { reason };
    }

    RegistrationOutcome::Ok
}

#[cfg(not(windows))]
pub fn register(_hwnd_raw: isize, _sender: Sender<RawSignal>) -> RegistrationOutcome {
    RegistrationOutcome::Failed {
        reason: "仅支持 Windows",
    }
}
