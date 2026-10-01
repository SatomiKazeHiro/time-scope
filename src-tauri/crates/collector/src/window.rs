//! Windows 前台窗口采集。
//!
//! 关键约束（spec §5.1）：调用 `SetWinEventHook` 的线程**必须有 Windows 消息循环**，
//! 否则注册成功但回调永不触发。所以这里用 `std::thread::spawn` 起专用线程跑原生
//! `GetMessageW` / `DispatchMessageW` 消息泵，**不要**放进 tokio task。
//!
//! windows-rs 0.58 的几个签名注意点（写错了编译不过，见 §15 风险表）：
//! - `SetWinEventHook` / `UnhookWinEvent` 在 **`Win32::UI::Accessibility`**，不在 WindowsAndMessaging
//! - `SetWinEventHook` 返回裸 `HWINEVENTHOOK`，不是 Result
//! - `WINEVENTPROC` 的回调签名是 7 参数（hook, event, hwnd, idobject, idchild, ideventthread, dwmseventtime）
//! - `EVENT_*` 是裸 `u32` 常量，不是 newtype

#![cfg(windows)]

use crate::signals::RawSignal;
use activity_core::{Event, EventType, WindowFocusPayload, WindowTitleChangePayload};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::OnceLock;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{
    GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, PostThreadMessageW, TranslateMessage, EVENT_OBJECT_NAMECHANGE,
    EVENT_SYSTEM_FOREGROUND, MSG, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_QUIT,
};

/// `QueryFullProcessImageNameW` 的缓冲大小（UTF-16 code unit）。
/// MAX_PATH 是 260，但长路径可远超它；4096 足够且只有 8 KB。
const EXE_PATH_BUF: usize = 4096;

static RUNNING: AtomicBool = AtomicBool::new(false);

/// watcher 线程的 id；`stop_window_watcher` 用它把 `WM_QUIT` 投递到**正确**的线程。
static WATCHER_TID: AtomicU32 = AtomicU32::new(0);

/// 回调里要用的 sender。回调签名是裸 `extern "system" fn`，不能捕获环境，
/// 所以用全局 `OnceLock`（进程内只 spawn 一次，够用）。
static SENDER: OnceLock<Sender<RawSignal>> = OnceLock::new();

/// `EVENT_OBJECT_NAMECHANGE` 不区分对象类型，会对子控件、滚动条等一切对象派发。
/// spec §5.2 要求只保留**当前前台窗口**的标题变化。
///
/// 抽成纯函数以便单测。
pub fn should_capture_title_change(hwnd: u64, current_foreground: u64) -> bool {
    hwnd != 0 && hwnd == current_foreground
}

unsafe extern "system" fn event_hook_callback(
    _hook: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _idobject: i32,
    _idchild: i32,
    _ideventthread: u32,
    _dwmseventtime: u32,
) {
    // 回调内只做最轻操作：塞 HWND。不取标题、不分配字符串、不碰 DB。
    let Some(sender) = SENDER.get() else { return };

    let signal = match event {
        x if x == EVENT_SYSTEM_FOREGROUND => RawSignal::WindowFocus(hwnd),
        x if x == EVENT_OBJECT_NAMECHANGE => {
            // 在**回调里**过滤：此刻的前台窗口就是事件发生时的前台窗口。
            // 若拖到 consumer 侧再过滤，用的是"当前"前台窗口，窗口切换快于
            // consumer 排空队列时会误删合法的标题变化。
            let fg = GetForegroundWindow();
            if !should_capture_title_change(hwnd.0 as u64, fg.0 as u64) {
                return;
            }
            RawSignal::WindowTitleChange(hwnd)
        }
        _ => return,
    };
    let _ = sender.send(signal);
}

/// 启动窗口 watcher 线程。进程内只能调用一次（`SENDER` 是一次性的）。
pub fn spawn_window_watcher(sender: Sender<RawSignal>) {
    RUNNING.store(true, Ordering::SeqCst);
    let _ = SENDER.set(sender);

    std::thread::spawn(|| {
        let tid = unsafe { GetCurrentThreadId() };
        WATCHER_TID.store(tid, Ordering::SeqCst);

        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_OBJECT_NAMECHANGE,
                None,
                Some(event_hook_callback),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if hook.is_invalid() {
            eprintln!(
                "[time-scope] SetWinEventHook failed: {:?}",
                unsafe { GetLastError() }
            );
            return;
        }

        // 原生消息泵：没有它回调永不触发（spec §5.1）
        let mut msg: MSG = unsafe { std::mem::zeroed() };
        loop {
            let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            if r.0 <= 0 {
                break;
            }
            if !RUNNING.load(Ordering::SeqCst) {
                break;
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        unsafe {
            let _ = UnhookWinEvent(hook);
        }
        WATCHER_TID.store(0, Ordering::SeqCst);
    });
}

/// 停止 watcher。必须把 `WM_QUIT` 投给 **watcher 线程**——
/// `GetMessageW` 阻塞时只有收到消息才会返回，单纯置 flag 不会唤醒它，线程会泄漏。
pub fn stop_window_watcher() {
    RUNNING.store(false, Ordering::SeqCst);
    let tid = WATCHER_TID.load(Ordering::SeqCst);
    if tid != 0 {
        let _ = unsafe { PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0)) };
    }
}

/// watcher 线程 id；0 表示未启动或已退出（诊断用）。
pub fn watcher_thread_id() -> u32 {
    WATCHER_TID.load(Ordering::SeqCst)
}

/// 取窗口的 (process_name, window_title, exe_path)。
///
/// 取不到进程名（权限不足、进程已退出、pid 为 0）时返回 None——宁可漏一条 Event，
/// 也不写一条 process_name 为空的事实进不可变的 Event 表（spec §3 核心原则 4）。
pub fn window_info(hwnd: HWND) -> Option<(String, Option<String>, Option<String>)> {
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    if pid == 0 {
        return None;
    }

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;

    // 放堆上而不是栈上：栈只有 2 MB，没必要为一次查询占掉一大块。
    let mut buf = vec![0u16; EXE_PATH_BUF];
    let mut len = buf.len() as u32;
    let queried = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    };
    unsafe {
        let _ = CloseHandle(handle);
    }

    let exe_path = queried
        .ok()
        .map(|_| String::from_utf16_lossy(&buf[..len as usize]));
    let process_name = exe_path
        .as_deref()
        .and_then(|p| p.rsplit(['\\', '/']).next())
        .map(|s| s.to_string())?;

    // 窗口标题：GetWindowTextLengthW <= 0 表示无标题（部分系统窗口 / UWP，Review Focus #4）
    let title = unsafe {
        let n = GetWindowTextLengthW(hwnd);
        if n <= 0 {
            None
        } else {
            let mut t = vec![0u16; (n as usize) + 1];
            let got = GetWindowTextW(hwnd, &mut t);
            Some(String::from_utf16_lossy(&t[..got as usize]))
        }
    };

    Some((process_name, title, exe_path))
}

pub fn make_focus_event(hwnd: HWND, ts: i64) -> Option<Event> {
    let (process_name, window_title, exe_path) = window_info(hwnd)?;
    Some(Event::new(
        EventType::WindowFocus(WindowFocusPayload {
            process_name,
            window_title,
            exe_path,
        }),
        ts,
    ))
}

/// 构造 `WindowTitleChange` 事件。
///
/// **不再做前台过滤**——过滤已移到 hook 回调（见 `event_hook_callback`），
/// 在事件发生的瞬间完成。这里再过滤一次反而会误删：等 consumer 跑到这一行时，
/// 用户可能已经切到别的窗口了。
pub fn make_title_event(hwnd: HWND, ts: i64) -> Option<Event> {
    let (process_name, window_title, _exe) = window_info(hwnd)?;
    Some(Event::new(
        EventType::WindowTitleChange(WindowTitleChangePayload {
            process_name,
            window_title,
        }),
        ts,
    ))
}
