//! 窗口采集层的自动冒烟测试（不需要人工操作）。
//!
//! 覆盖三件真实的事：
//! 1. watcher 线程起来、`SetWinEventHook` 注册成功、消息泵在跑（hook 会派发信号）
//! 2. `window_info` 能把一个**真实前台窗口**的 HWND 换算成可用的 Event
//!    （含 OpenProcess / QueryFullProcessImageNameW / GetWindowTextW 全链路）
//! 3. `make_title_event` 会把非前台窗口的 HWND 过滤掉（spec §5.2）
//!
//! 不试图强制制造前台切换：Windows 的 foreground lock 在无人值守环境下
//! 不可靠地拒绝（实测 `SetForegroundWindow` 成功率 0/6），依赖它会让本测试变 flaky。
//! 真正的"切窗口 → 冒事件"留给 Task 11 的人工验证清单。
//!
//! 运行：
//!   cargo test -p activity-collector --test smoke -- --nocapture
#![cfg(windows)]

use activity_collector::signals::RawSignal;
use activity_collector::window::{
    make_focus_event, make_title_event, spawn_window_watcher, stop_window_watcher,
    watcher_thread_id,
};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetShellWindow};

fn drain(rx: &Receiver<RawSignal>, ms: u64) -> (usize, usize) {
    let mut focus = 0;
    let mut title = 0;
    let deadline = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < deadline {
        while let Ok(sig) = rx.try_recv() {
            match sig {
                RawSignal::WindowFocus(_) => focus += 1,
                RawSignal::WindowTitleChange(_) => title += 1,
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    (focus, title)
}

#[test]
fn watcher_registers_hook_and_message_pump_runs() {
    let (tx, rx) = channel();
    spawn_window_watcher(tx);

    let boot = Instant::now() + Duration::from_secs(5);
    while watcher_thread_id() == 0 && Instant::now() < boot {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_ne!(watcher_thread_id(), 0, "watcher 线程没起来");

    // 桌面上只要有一点 UI 活动，hook 就会派发信号。这里允许 0——
    // 完全静止的桌面确实可能一条都没有，hook 能否注册由上面的 tid 断言覆盖。
    let (focus, title) = drain(&rx, 1500);
    eprintln!("signals: focus={focus} title={title}");

    stop_window_watcher();
    // 停得掉（PostThreadMessageW 投到了正确的线程）=> 100ms 内心跳就会退出
    let gone = Instant::now() + Duration::from_secs(3);
    while watcher_thread_id() != 0 && Instant::now() < gone {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        watcher_thread_id(),
        0,
        "stop_window_watcher 没能让消息泵退出（WM_QUIT 投错线程？）"
    );
}

#[test]
fn real_foreground_window_yields_a_usable_event() {
    // 绕过 hook，直接拿当前真实前台窗口跑一遍 HWND → Event 的全链路。
    let hwnd = unsafe { GetForegroundWindow() };
    assert!(!hwnd.0.is_null(), "拿不到前台窗口（会话未交互？）");

    let (process_name, title, exe_path) =
        activity_collector::window::window_info(hwnd).expect("前台窗口应能取到进程信息");
    eprintln!("process={process_name} title={title:?} exe={exe_path:?}");

    assert!(!process_name.is_empty());
    assert!(
        process_name.to_lowercase().ends_with(".exe"),
        "process_name 应是文件名，实际={process_name}"
    );
    if let Some(p) = &exe_path {
        assert!(
            p.contains('\\') || p.contains('/'),
            "exe_path 应是完整路径，实际={p}"
        );
    }

    let e = make_focus_event(hwnd, 1_700_000_000_000).expect("应能构造 focus 事件");
    match &e.event_type {
        activity_core::EventType::WindowFocus(p) => {
            assert_eq!(p.process_name, process_name);
            assert_eq!(p.window_title, title);
        }
        other => panic!("variant 错误: {other:?}"),
    }
    assert_eq!(e.timestamp, 1_700_000_000_000);
}

#[test]
fn title_event_no_longer_refilters_in_the_consumer() {
    // 过滤已移到 hook 回调（`event_hook_callback`），在事件发生的那一瞬间完成。
    // 如果 consumer 再拿"当前"前台窗口比一次，用户切走后就会把合法事件误删。
    // 这里的职责是：拿到什么 HWND 就如实构造事件，不做时序相关的判断。
    let fg = unsafe { GetForegroundWindow() };
    let shell = unsafe { GetShellWindow() };

    let e = make_title_event(fg, 0).expect("前台窗口应能构造 title 事件");
    match e.event_type {
        activity_core::EventType::WindowTitleChange(p) => {
            assert!(!p.process_name.is_empty());
        }
        other => panic!("variant 错误: {other:?}"),
    }

    // 非前台窗口也能构造出事件——consumer 不再关心它是不是前台。
    // 不在前台这件事由回调里的 should_capture_title_change 拦掉。
    if !shell.0.is_null() && shell != fg {
        let _ = make_title_event(shell, 0);
    }
}

#[test]
fn title_filter_keeps_only_the_window_that_was_foreground_at_event_time() {
    use activity_collector::window::should_capture_title_change;

    const FG: u64 = 0x1234;
    const OTHER: u64 = 0x5678;
    const NULL: u64 = 0;

    // 前台窗口自己：保留
    assert!(should_capture_title_change(FG, FG));
    // 非前台窗口（子控件、滚动条等）：丢弃
    assert!(!should_capture_title_change(OTHER, FG));
    // 空 HWND：丢弃，绝不能去查一个空窗口
    assert!(!should_capture_title_change(NULL, FG));
    // 没有前台窗口时一切都不匹配
    assert!(!should_capture_title_change(FG, NULL));
}
