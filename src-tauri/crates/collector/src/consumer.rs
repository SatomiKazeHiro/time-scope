use crate::input::{make_heartbeat_event, make_idle_event, make_resume_event};
use crate::signals::RawSignal;
use crate::window::{make_focus_event, make_title_event};
use activity_core::{Event, EventType};
use activity_storage::BatchWriter;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

/// 对窗口标题做脱敏（spec §11）。
///
/// 由 app 层注入，collector 因此既不认识 rules、也不依赖 regex crate——
/// 保持"collector 只做 OS 交互"这条边界（spec §4）。
pub type TitleRedactor = Arc<dyn Fn(&str) -> String + Send + Sync>;

/// 消费循环：收 RawSignal → 生成 Event → 脱敏 → 入写盘队列，并转发给引擎线程。
///
/// 脱敏放在**入队之前**：敏感标题不应该以明文形式在内存队列里存在哪怕一秒。
/// 转发用的是第二条 channel，collector 不需要认识 engine（spec §4 的边界）。
/// `event_tx` 为 `None` 时只是不做转发。
pub fn spawn_consumer(
    rx: Receiver<RawSignal>,
    writer: Arc<BatchWriter>,
    event_tx: Option<Sender<Event>>,
) {
    spawn_consumer_with(rx, writer, event_tx, None)
}

/// 同上，但带一个标题脱敏器。
pub fn spawn_consumer_with(
    rx: Receiver<RawSignal>,
    writer: Arc<BatchWriter>,
    event_tx: Option<Sender<Event>>,
    redactor: Option<TitleRedactor>,
) {
    std::thread::spawn(move || {
        while let Ok(sig) = rx.recv() {
            let now = activity_storage::now_ms();
            let Some(event) = signal_to_event(sig, now) else {
                continue;
            };
            let event = match &redactor {
                Some(r) => redact_event(&event, r.as_ref()),
                None => event,
            };
            if let Some(tx) = &event_tx {
                // 引擎侧失败不该影响采集：断掉的 channel 直接忽略
                let _ = tx.send(event.clone());
            }
            writer.push(event);
        }
    });
}

/// 对一个 Event 的窗口标题做脱敏，其他字段原样保留。
pub fn redact_event(event: &Event, redactor: &dyn Fn(&str) -> String) -> Event {
    let event_type = match &event.event_type {
        EventType::WindowFocus(p) => {
            let mut p = p.clone();
            p.window_title = p.window_title.as_deref().map(redactor);
            EventType::WindowFocus(p)
        }
        EventType::WindowTitleChange(p) => {
            let mut p = p.clone();
            p.window_title = p.window_title.as_deref().map(redactor);
            EventType::WindowTitleChange(p)
        }
        other => other.clone(),
    };
    Event {
        id: event.id.clone(),
        timestamp: event.timestamp,
        event_type,
    }
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
        RawSignal::SessionLock => Some(Event::new(EventType::SessionLock, ts)),
        RawSignal::SessionUnlock => Some(Event::new(EventType::SessionUnlock, ts)),
        #[cfg(not(windows))]
        RawSignal::_Noop => None,
    }
}
