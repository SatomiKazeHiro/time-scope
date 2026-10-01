//! 锁屏 / 睡眠监听（spec §3）。
//!
//! 两条 Win32 通路，**都必须挂在一个窗口 HWND 上**：
//! - `WTSRegisterSessionNotification` —— 锁屏 / 解锁（窗口消息 `WM_WTSSESSION_CHANGE`）
//! - `PowerRegisterSuspendResumeNotification` —— 合盖 / 唤醒（`WM_POWERBROADCAST`）
//!
//! 这与现有的采集线程不同：窗口 hook 和输入轮询都可以纯后台线程起，
//! 但这两个通知只能绑窗口。Tauri 2 不透出原始窗口消息，所以监听**自己建一个
//! message-only 窗口**并跑自己的消息泵 —— 这也是托盘型应用收这两类通知的
//! 标准做法（spec §3.2）。翻译逻辑 [`translate`] 与注册结论
//! [`registration_outcome`] 都是纯函数，可自动验证。

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

/// 在**自建的 message-only 窗口**上启动锁屏/睡眠监听。
///
/// 为什么自己建窗口（而不是用 Tauri 主窗口）：Tauri 2 的 `WindowEvent` 不透出原始
/// 窗口消息，拿不到 `WM_WTSSESSION_CHANGE` / `WM_POWERBROADCAST`。而 WTS 与电源
/// 通知按设计就必须绑一个 HWND —— 对托盘型应用来说，"建一个 message-only 窗口 +
/// 自己的消息泵"就是标准做法。
///
/// 失败不 panic：返回 `Failed` 让调用方记一行 stderr 并继续
/// （spec §7：常驻增强失败不能拖垮主链路）。锁屏采集降级，其余功能不受影响。
#[cfg(windows)]
pub fn spawn_listener(sender: Sender<RawSignal>) -> RegistrationOutcome {
    use std::time::Duration;

    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<RegistrationOutcome>();
    let spawned = std::thread::Builder::new()
        .name("ts-session".into())
        .spawn(move || {
            let outcome = run_listener(sender);
            // 启动结果回传；失败时线程直接结束，不进消息循环
            let _ = ready_tx.send(outcome);
        });
    if spawned.is_err() {
        let reason = "session 监听线程启动失败";
        eprintln!("[time-scope] {reason}：锁屏/解锁与合盖/唤醒将不被记录，其余功能正常");
        return RegistrationOutcome::Failed { reason };
    }
    // 监听线程会阻塞在消息循环里，所以它一定会在进入循环前先回传结果
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap_or(RegistrationOutcome::Failed {
            reason: "session 监听线程启动超时",
        })
}

#[cfg(windows)]
fn run_listener(sender: Sender<RawSignal>) -> RegistrationOutcome {
    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Power::{
        PowerRegisterSuspendResumeNotification,
    };
    use windows::Win32::System::RemoteDesktop::{
        WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
    };
    use windows::Win32::UI::WindowsAndMessaging::DEVICE_NOTIFY_WINDOW_HANDLE;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, HWND_MESSAGE,
        MSG, WNDCLASSW,
    };
    use windows::Win32::Foundation::HWND;
    use windows::core::w;

    // Sender 没法穿过 WndProc 的 C ABI，用线程局部变量递进去
    thread_local! {
        static SENDER: std::cell::RefCell<Option<Sender<RawSignal>>> =
            const { std::cell::RefCell::new(None) };
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if let Some(sig) = translate(msg, wparam.0 as usize) {
            SENDER.with(|s| {
                if let Some(tx) = s.borrow().as_ref() {
                    // 通道断了（consumer 已退出）不该让窗口过程 panic
                    let _ = tx.send(sig);
                }
            });
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    SENDER.with(|s| *s.borrow_mut() = Some(sender));

    let instance = match unsafe { GetModuleHandleW(None) } {
        Ok(h) => h,
        Err(_) => {
            return RegistrationOutcome::Failed {
                reason: "GetModuleHandleW 失败",
            }
        }
    };
    let hinstance = HINSTANCE(instance.0);

    let class = w!("TimeScopeSessionListener");
    // 重复启动时类已存在（ERROR_CLASS_ALREADY_EXISTS），不当作失败
    if unsafe { RegisterClassW(&WNDCLASSW {
        style: windows::Win32::UI::WindowsAndMessaging::WNDCLASS_STYLES(0),
        lpfnWndProc: Some(wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance,
        hIcon: windows::Win32::UI::WindowsAndMessaging::HICON::default(),
        hCursor: windows::Win32::UI::WindowsAndMessaging::HCURSOR::default(),
        hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH::default(),
        lpszMenuName: windows::core::PCWSTR::null(),
        lpszClassName: class,
    }) } == 0
    {
        let reason = "RegisterClassW 失败";
        eprintln!("[time-scope] {reason}：锁屏/解锁与合盖/唤醒将不被记录，其余功能正常");
        return RegistrationOutcome::Failed { reason };
    }

    let hwnd = match unsafe {
        CreateWindowExW(
            windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
            class,
            windows::core::PCWSTR::null(),
            windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            None,
            instance,
            None,
        )
    } {
        Ok(h) => h,
        Err(_) => {
            let reason = "CreateWindowExW 失败";
            eprintln!("[time-scope] {reason}：锁屏/解锁与合盖/唤醒将不被记录，其余功能正常");
            return RegistrationOutcome::Failed { reason };
        }
    };

    if unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) }.is_err() {
        let reason = "WTSRegisterSessionNotification 失败";
        eprintln!("[time-scope] {reason}：锁屏/解锁将不被记录，其余功能正常");
        return RegistrationOutcome::Failed { reason };
    }

    let mut power_handle: *mut std::ffi::c_void = std::ptr::null_mut();
    let power = unsafe {
        PowerRegisterSuspendResumeNotification(DEVICE_NOTIFY_WINDOW_HANDLE, hwnd, &mut power_handle)
    };
    if power.to_hresult().is_err() {
        let reason = "PowerRegisterSuspendResumeNotification 失败";
        eprintln!("[time-scope] {reason}：合盖/唤醒将不被记录，其余功能正常");
        return RegistrationOutcome::Failed { reason };
    }

    // 消息泵：一直转到进程结束。这里的循环体是空的——处理都在 wnd_proc 里。
    let mut msg = MSG::default();
    loop {
        let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        // <= 0：出错或收到 WM_QUIT，都结束
        if r.0 <= 0 {
            break;
        }
        unsafe {
            let _ = DispatchMessageW(&msg);
        }
    }
    RegistrationOutcome::Ok
}

#[cfg(not(windows))]
pub fn spawn_listener(_sender: Sender<RawSignal>) -> RegistrationOutcome {
    RegistrationOutcome::Failed {
        reason: "仅支持 Windows",
    }
}
