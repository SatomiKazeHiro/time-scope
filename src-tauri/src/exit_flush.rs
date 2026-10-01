//! 退出刷盘。
//!
//! spec §8.1 明确"程序正常退出时 flush"。这件事必须显式做：`BatchWriter` 的
//! `Drop` 只是给后台线程置一个停止标志，而进程从 `main` 返回时 `AppState` 往往
//! 根本不会被 drop，后台线程也没有机会跑完它那最后一次冲刷。
//!
//! 不这样做的话，**每一次干净关闭都会丢掉队列里最多 5 秒的 Event**。

use activity_storage::BatchWriter;

/// 退出前同步冲刷队列，返回写入条数。写失败只记日志，不阻塞退出。
pub fn flush_for_exit(writer: &BatchWriter) -> usize {
    match writer.flush() {
        Ok(n) => n,
        Err(e) => {
            eprintln!("[time-scope] exit flush failed: {e}");
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::flush_for_exit;
    use activity_core::{Event, EventType};
    use activity_storage::{get_events_in_range, open_in_memory_shared, BatchWriter};
    use std::sync::Arc;

    fn count(conn: &activity_storage::SharedConn) -> usize {
        get_events_in_range(&conn.lock().unwrap(), 0, i64::MAX).unwrap().len()
    }

    #[test]
    fn writes_events_queued_since_last_auto_flush() {
        let conn = open_in_memory_shared();
        // 间隔很长 => 手动 flush 前不会自动落盘，模拟"刚采到、还没到 5s"
        let writer = BatchWriter::new(Arc::clone(&conn), 600_000, 1_000);
        for i in 0..5 {
            writer.push(Event::new(EventType::SystemIdle, 1000 + i));
        }
        assert_eq!(writer.queued(), 5);
        assert_eq!(count(&conn), 0, "还没到间隔，不应已经落盘");

        assert_eq!(flush_for_exit(&writer), 5);
        assert_eq!(count(&conn), 5, "退出刷盘应把队列里剩下的全部写入");
    }

    #[test]
    fn is_a_noop_when_nothing_is_queued() {
        let conn = open_in_memory_shared();
        let writer = BatchWriter::new(conn, 600_000, 1_000);
        assert_eq!(flush_for_exit(&writer), 0);
    }

    #[test]
    fn draining_is_idempotent() {
        let conn = open_in_memory_shared();
        let writer = BatchWriter::new(Arc::clone(&conn), 600_000, 1_000);
        writer.push(Event::new(EventType::SystemIdle, 1000));
        assert_eq!(flush_for_exit(&writer), 1);
        assert_eq!(flush_for_exit(&writer), 0, "队列已空，第二次不应重复写");
        assert_eq!(count(&conn), 1);
    }
}
