//! 引擎线程：持续消费 Event，把关闭的 ActivitySegment 落库。
//!
//! 这是"纯引擎"和"有 IO 的世界"之间唯一的桥。引擎本身不碰数据库，
//! IO 全部在这里做（spec §4）。

use crate::engine_runtime::{to_stored, EngineRuntime};
use activity_core::Event;
use activity_storage::SharedConn;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

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
