//! 事件总线：把采集层的 `RawSignal` 翻译成 `Event` 并交给 `BatchWriter`。
//!
//! 这是 spec §3 架构图里 `Event Bus` 那一层的实现。取窗口标题/进程名这类重活
//! 放在这里（consumer 线程）而不是 hook 回调里，正是为了让回调保持最轻（§5.1）。

use crate::input::{make_heartbeat_event, make_idle_event, make_resume_event};
use crate::signals::RawSignal;
use crate::window::{make_focus_event, make_title_event};
use activity_storage::BatchWriter;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

/// 消费循环：收 RawSignal → 生成 Event → 入写盘队列。
///
/// 拿不到窗口信息的信号（HWND 已失效 / 非前台）会被静默丢弃——宁可少一条 Event，
/// 也不往不可变的事实表里塞一条字段残缺的事实（spec §3 核心原则 4）。
pub fn spawn_consumer(rx: Receiver<RawSignal>, writer: Arc<BatchWriter>) {
    std::thread::spawn(move || {
        while let Ok(sig) = rx.recv() {
            let now = activity_storage::now_ms();
            let event = signal_to_event(sig, now);
            if let Some(e) = event {
                writer.push(e);
            }
        }
    });
}

/// 把一个信号翻译成 Event。抽成纯函数便于单测。
pub fn signal_to_event(sig: RawSignal, ts: i64) -> Option<activity_core::Event> {
    match sig {
        #[cfg(windows)]
        RawSignal::WindowFocus(h) => make_focus_event(h, ts),
        #[cfg(windows)]
        RawSignal::WindowTitleChange(h) => make_title_event(h, ts),
        RawSignal::IdleStart => Some(make_idle_event(ts)),
        RawSignal::InputActive => Some(make_resume_event(ts)),
        RawSignal::Heartbeat(active_seconds) => Some(make_heartbeat_event(active_seconds, ts)),
        #[cfg(not(windows))]
        RawSignal::_Noop => None,
    }
}
