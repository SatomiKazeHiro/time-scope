//! 引擎线程：持续消费 Event，把关闭的 ActivitySegment 落库。
//!
//! 这是"纯引擎"和"有 IO 的世界"之间唯一的桥。引擎本身不碰数据库，
//! IO 全部在这里做（spec §4）。

use crate::engine_runtime::{to_stored, EngineRuntime};
use activity_core::Event;
use activity_engine::RuleSet;
use activity_storage::SharedConn;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

/// 冷启动：清掉那天的 activities，重放当天已有的 Event，把结果落库。
///
/// 返回可继续驱动的运行时。
pub fn bootstrap_day(
    conn: &SharedConn,
    rules: RuleSet,
    config: activity_engine::EngineConfig,
    day_start_ms: i64,
    day_end_ms: i64,
) -> EngineRuntime {
    // 重放前必须先清，否则每次启动都会累积一份重复的段（spec §7.4）
    {
        let c = conn.lock().unwrap();
        if let Err(e) = activity_storage::delete_segments_for_day(&c, day_start_ms, day_end_ms) {
            eprintln!("[time-scope] 清理当天 activities 失败: {e}");
        }
    }
    let events = {
        let c = conn.lock().unwrap();
        activity_storage::get_events_in_range(&c, day_start_ms, day_end_ms).unwrap_or_default()
    };
    let engine_events: Vec<Event> = events
        .iter()
        .map(|stored| {
            // payload 里存的是 EventType；id / timestamp 来自各自的列
            match serde_json::from_str::<activity_core::EventType>(&stored.payload) {
                Ok(event_type) => Event {
                    id: stored.id.clone(),
                    timestamp: stored.timestamp,
                    event_type,
                },
                Err(e) => {
                    eprintln!("[time-scope] 跳过无法解析的 event {}: {e}", stored.id);
                    Event {
                        id: String::new(),
                        timestamp: 0,
                        event_type: activity_core::EventType::SystemIdle,
                    }
                }
            }
        })
        .filter(|e| !e.id.is_empty())
        .collect();

    let (rt, closed) = EngineRuntime::bootstrap(&engine_events, rules, config);
    persist(conn, &closed);
    rt
}

/// 把关闭的段落库。失败只记日志——采集链路不能因为落库失败而停摆。
pub fn persist(conn: &SharedConn, closed: &[activity_engine::ActivitySegment]) {
    if closed.is_empty() {
        return;
    }
    let Ok(c) = conn.lock() else {
        eprintln!("[time-scope] 连接锁中毒，跳过 {} 段的写入", closed.len());
        return;
    };
    if let Err(e) = activity_storage::insert_segments(&c, &to_stored(closed)) {
        eprintln!("[time-scope] 写 activities 失败（{} 段）: {e}", closed.len());
    }
}

/// 启动引擎线程：收 Event → 推进引擎 → 落库。
pub fn spawn_engine_thread(
    rx: Receiver<Event>,
    conn: SharedConn,
    runtime: Arc<Mutex<EngineRuntime>>,
) {
    std::thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            let closed = match runtime.lock() {
                Ok(mut rt) => rt.ingest(&event),
                Err(_) => continue,
            };
            persist(&conn, &closed);
        }
    });
}
