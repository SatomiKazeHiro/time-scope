//! 锁屏 / 睡眠监听（spec §3）。
//!
//! 两条 Win32 通路，都必须挂在一个窗口 HWND 上：
//! - `WTSRegisterSessionNotification` —— 锁屏 / 解锁（窗口消息 `WM_WTSSESSION_CHANGE`）
//! - `PowerRegisterSuspendResumeNotification` —— 合盖 / 唤醒（`WM_POWERBROADCAST`）
//!
//! 这与现有的采集线程不同：窗口 hook 和输入轮询都可以纯后台线程起，
//! 但这两个通知只能绑窗口。Tauri 2 不透出原始窗口消息，所以监听自己建一个
//! message-only 窗口并跑自己的消息泵 —— 这也是托盘型应用收这两类通知的
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
    /// 注册失败。只降级锁屏采集，不影响其余功能（spec §7）。
    Failed { reason: &'static str },
}

/// 两条注册通路的独立结果（2026-10-13）。
///
/// 为什么要有这个类型：原来两条注册写在同一个函数里，任何一条失败都直接
/// `return` —— 于是一条失败会连坐另一条，而且函数提前返回意味着消息泵
/// 根本没启动，已成功注册的那个 HWND 也没人收消息。
///
/// 真实事故（2026-10-13）用户终端里打的是
/// `PowerRegisterSuspendResumeNotification 失败：合盖/唤醒将不被记录`
/// 紧接着 `锁屏/睡眠监听 不可用`。而 `WTSRegisterSessionNotification`
/// 其实成功了 —— 只是因为电源失败提前 return，锁屏事件也一条都发不出来。
///
/// 修好之后：两路各自成败，只要有一路可用，消息泵就必须启动。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionRegistration {
    pub wts_ok: bool,
    pub power_ok: bool,
}

impl SessionRegistration {
    /// 锁屏/解锁可用吗
    pub fn lock_unlock_works(&self) -> bool {
        self.wts_ok
    }
    /// 合盖/唤醒可用吗
    pub fn power_works(&self) -> bool {
        self.power_ok
    }
    /// 两路都不可用 —— 只有这时消息泵才可以不启动
    pub fn is_fully_broken(&self) -> bool {
        !self.wts_ok && !self.power_ok
    }
    /// 只要有一路可用就该跑消息泵
    pub fn should_pump(&self) -> bool {
        !self.is_fully_broken()
    }
}

/// 纯函数：把 Win32 的 Result 转成可断言的结论。
pub fn registration_outcome(raw: Result<(), &'static str>) -> RegistrationOutcome {
    match raw {
        Ok(()) => RegistrationOutcome::Ok,
        Err(reason) => RegistrationOutcome::Failed { reason },
    }
}

/// 把一条窗口消息翻译成采集信号。纯函数。
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

/// 在自建的 message-only 窗口上启动锁屏/睡眠监听。
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
            // `run_listener` 在进入消息泵之前就把注册结果发回去。
            // 原来是把返回值等到消息泵退出（= 进程结束）才发，
            // 于是这个 5 秒握手永远超时 —— 监听其实好好的，
            // 界面却报「不可用」。
            let _ = run_listener(sender, ready_tx);
        });
    if spawned.is_err() {
        let reason = "session 监听线程启动失败";
        eprintln!("[time-scope] {reason}：锁屏/解锁与合盖/唤醒将不被记录，其余功能正常");
        return RegistrationOutcome::Failed { reason };
    }
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap_or(RegistrationOutcome::Failed {
            reason: "session 监听线程启动超时",
        })
}

#[cfg(windows)]
fn run_listener(
    sender: Sender<RawSignal>,
    ready: std::sync::mpsc::Sender<RegistrationOutcome>,
) -> RegistrationOutcome {
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
        // 诊断（2026-10-13 保留）：只打消息号与 wparam，不打窗口标题
        // —— 那是用户数据。只在锁屏/合盖/唤醒这三个低频时刻打，
        // 一次刷屏都不会有（实测：一天几十行以内）。
        //
        // 保留的理由：2026-10-13 那次「锁屏没记录」就是靠这几行定位的 ——
        // 注册成功但消息没来，与消息来了没翻译，是完全不同的两个问题。
        // 下次怀疑锁屏采集坏了时，看一眼这几行就知道该往哪查。
        if matches!(msg, WM_WTSSESSION_CHANGE | WM_POWERBROADCAST) {
            eprintln!(
                "[time-scope] session 收到消息 msg={msg} wparam=0x{:x}",
                wparam.0
            );
        }
        if let Some(sig) = translate(msg, wparam.0) {
            eprintln!("[time-scope] session 消息 -> 采集信号 {sig:?}");
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
            let out = RegistrationOutcome::Failed {
                reason: "GetModuleHandleW 失败",
            };
            let _ = ready.send(out);
            return out;
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
        eprintln!("[time-scope] {reason}：该时段不会被记录，其余功能正常");
        let out = RegistrationOutcome::Failed { reason };
        let _ = ready.send(out);
        return out;
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
            eprintln!("[time-scope] {reason}：该时段不会被记录，其余功能正常");
            let out = RegistrationOutcome::Failed { reason };
            let _ = ready.send(out);
            return out;
        }
    };

    // 两条注册各自成败，不要连坐（2026-10-13）。
    // 原来任何一条失败都直接 return，函数提前退出 → 消息泵从未启动 →
    // 已经成功注册的那个 HWND 也没人收消息。真实事故里 WTS 注册成功了，
    // 却因为电源失败而让锁屏事件一条都发不出来。
    let wts_ok = match unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) } {
        Ok(()) => true,
        Err(e) => {
            // 诊断（2026-10-13）：把错误码打出来。原来只判 `is_err()`，
            // 错误码被丢掉了 —— 排查时只能知道「失败了」，不知道「为什么」。
            eprintln!("[time-scope] WTSRegisterSessionNotification 失败：锁屏/解锁将不被记录（{e:?}）");
            false
        }
    };
    if wts_ok {
        // 注册成功 ≠ 消息会来（2026-10-13 实测：注册成功但一条消息都没有）。
        // 这一行把「已注册」明确打出来，和下面的「收到消息」对照着看，
        // 就能区分「没注册上」与「注册上了但消息没来」。
        eprintln!("[time-scope] WTSRegisterSessionNotification 成功（锁屏/解锁通道已注册）");
    }

    let mut power_handle: *mut std::ffi::c_void = std::ptr::null_mut();
    let power = unsafe {
        PowerRegisterSuspendResumeNotification(DEVICE_NOTIFY_WINDOW_HANDLE, hwnd, &mut power_handle)
    };
    let power_ok = power.to_hresult().is_ok();
    if !power_ok {
        // 只说这一路没了，不说「整个监听不可用」 —— 那是之前那句误导人的话，
        // 用户正是被它误导才以为锁屏也没在记
        eprintln!(
            "[time-scope] PowerRegisterSuspendResumeNotification 失败：合盖/唤醒将不被记录（{:?}）",
            power.to_hresult()
        );
    }

    let reg = SessionRegistration { wts_ok, power_ok };
    // 消息泵对两路都必要 —— 它是那个 HWND 上消息被分发的唯一途径。
    // 所以判据是「有一路可用就跑」，不是「两路都成功才跑」。
    if !reg.should_pump() {
        let reason = "锁屏与电源两条注册都失败";
        eprintln!("[time-scope] {reason}：该时段完全不会被记录，其余功能正常");
        let out = RegistrationOutcome::Failed { reason };
        let _ = ready.send(out);
        return out;
    }

    // 进消息泵之前回传结果 —— 握手就靠它（见 `spawn_listener` 的注释）。
    // 只有两路都成功才报 Ok；一路成功时给出具体是哪一路可用。
    // reason 只描述事实，不带「部分可用」这种结论词 ——
    // 上层 `lib.rs` 会加前缀，写两遍就成了
    // 「部分可用（部分可用（…））」（2026-10-13 真机上出现过）。
    let outcome = if reg.wts_ok && reg.power_ok {
        RegistrationOutcome::Ok
    } else if reg.wts_ok {
        RegistrationOutcome::Failed {
            reason: "合盖/唤醒不可用，锁屏/解锁正常",
        }
    } else {
        RegistrationOutcome::Failed {
            reason: "锁屏/解锁不可用，合盖/唤醒正常",
        }
    };
    let _ = ready.send(outcome);

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
    // 泵是进程生命周期内一直在跑的；走到这里说明收到 WM_QUIT 或 GetMessage 出了错。
    // 对外结论早在进泵之前就通过 `ready` 回传过了，这里只是让类型对得上。
    outcome
}

#[cfg(not(windows))]
pub fn spawn_listener(_sender: Sender<RawSignal>) -> RegistrationOutcome {
    RegistrationOutcome::Failed {
        reason: "仅支持 Windows",
    }
}
