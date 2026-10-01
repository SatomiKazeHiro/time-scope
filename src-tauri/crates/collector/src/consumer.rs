use crate::input::{make_heartbeat_event, make_idle_event, make_resume_event};
use crate::signals::RawSignal;
use crate::window::{make_focus_event, make_title_event};
use activity_core::Event;
use activity_storage::BatchWriter;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

/// 消费循环：收 RawSignal → 生成 Event → 入写盘队列，**并**转发给引擎线程。
///
/// 转发用的是第二条 channel，collector 因此不需要认识 engine（spec §4 的边界）。
/// `event_tx` 为 `None` 时只是不做转发。
pub fn spawn_consumer(
    rx: Receiver<RawSignal>,
    writer: Arc<BatchWriter>,
    event_tx: Option<Sender<Event>>,
) {
    std::thread::spawn(move || {
        while let Ok(sig) = rx.recv() {
            let now = activity_storage::now_ms();
            let Some(event) = signal_to_event(sig, now) else {
                continue;
            };
            if let Some(tx) = &event_tx {
                // 引擎侧失败不该影响采集：断掉的 channel 直接忽略
                let _ = tx.send(event.clone());
            }
            writer.push(event);
        }
    });
}

/// 把一个信号翻译成 Event。抽成纯函数便于单测。
pub fn signal_to_event(sig: RawSignal, ts: i64) -> Option<Event> {
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
