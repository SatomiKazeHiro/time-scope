use crate::query::insert_events;
use crate::SharedConn;
use activity_core::Event;
use rusqlite::Connection;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// 后台轮询周期。取 100ms 是在"刷盘延迟"和"空转唤醒"之间的折中：
/// 100ms 醒一次、什么都不做，对常驻应用的功耗预算（spec §12：CPU ≈ 0）可忽略。
const TICK: Duration = Duration::from_millis(100);

/// 事件批量写入器（spec §8.1）。
///
/// 采集线程只 `push`，不直接碰 DB；后台线程按「满 `batch_size` 条」或
/// 「距上次刷盘满 `flush_interval_ms`」触发单事务批量插入。
pub struct BatchWriter {
    conn: SharedConn,
    queue: Arc<Mutex<Vec<Event>>>,
    stop: Arc<AtomicBool>,
    /// 已成功写入条数，仅供诊断。
    written: Arc<AtomicUsize>,
}

impl BatchWriter {
    /// * `flush_interval_ms` — 定时刷盘间隔（spec 默认 5000）
    /// * `batch_size` — 队满触发阈值（spec 默认 100）
    pub fn new(conn: SharedConn, flush_interval_ms: u64, batch_size: usize) -> Self {
        let queue: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let written = Arc::new(AtomicUsize::new(0));

        let q = Arc::clone(&queue);
        let s = Arc::clone(&stop);
        let w = Arc::clone(&written);
        let c = Arc::clone(&conn);

        std::thread::spawn(move || {
            let mut last_flush = Instant::now();
            loop {
                std::thread::sleep(TICK);

                if s.load(Ordering::SeqCst) {
                    // 退出前冲一次，尽量不丢队列里的 Event
                    let batch = drain(&q);
                    if !batch.is_empty() {
                        let n = batch.len();
                        match with_conn(&c, |conn| insert_events(conn, &batch)) {
                            Ok(()) => {
                                w.fetch_add(n, Ordering::SeqCst);
                            }
                            Err(e) => {
                                eprintln!("[time-scope] final flush failed, dropped {n} events: {e}");
                            }
                        }
                    }
                    break;
                }

                let interval_elapsed = last_flush.elapsed().as_millis() as u64 >= flush_interval_ms;
                let full = q.lock().map(|g| g.len() >= batch_size).unwrap_or(false);
                if !interval_elapsed && !full {
                    continue;
                }

                let batch = drain(&q);
                if batch.is_empty() {
                    // 队列空：重置计时，避免"空转到点"后连锁刷盘
                    last_flush = Instant::now();
                    continue;
                }
                let n = batch.len();
                match with_conn(&c, |conn| insert_events(conn, &batch)) {
                    Ok(()) => {
                        w.fetch_add(n, Ordering::SeqCst);
                    }
                    Err(e) => {
                        eprintln!("[time-scope] batch flush failed, dropped {n} events: {e}");
                    }
                }
                last_flush = Instant::now();
            }
        });

        Self {
            conn,
            queue,
            stop,
            written,
        }
    }

    /// 采集线程调用。只锁一次队列。
    pub fn push(&self, e: Event) {
        if let Ok(mut g) = self.queue.lock() {
            g.push(e);
        }
    }

    /// 立刻冲刷队列，返回写入条数（0 表示无待写数据）。正常退出时调用。
    pub fn flush(&self) -> rusqlite::Result<usize> {
        let batch = drain(&self.queue);
        if batch.is_empty() {
            return Ok(0);
        }
        let n = batch.len();
        with_conn(&self.conn, |conn| insert_events(conn, &batch))?;
        self.written.fetch_add(n, Ordering::SeqCst);
        Ok(n)
    }

    /// 借出连接锁。拿到的 guard 可直接 `&` 传给需要 `&Connection` 的函数
    /// （Deref 转换），例如 `get_events_in_range(&writer.conn(), start, end)`。
    pub fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn queued(&self) -> usize {
        self.queue.lock().map(|g| g.len()).unwrap_or(0)
    }

    pub fn written(&self) -> usize {
        self.written.load(Ordering::SeqCst)
    }
}

/// 锁中毒时取回内部值：一次写盘失败不应让整个采集线程 panic。
fn with_conn<R>(conn: &SharedConn, f: impl FnOnce(&Connection) -> R) -> R {
    let guard = conn
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&guard)
}

fn drain(queue: &Mutex<Vec<Event>>) -> Vec<Event> {
    match queue.lock() {
        Ok(mut g) => std::mem::take(&mut *g),
        Err(_) => Vec::new(),
    }
}

impl Drop for BatchWriter {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // 只是唤醒信号：后台线程最多 TICK 后醒来冲一次再退出。
        // 队列由 Arc 持有，Event 不会因为 self 被 drop 而消失。
    }
}

/// 正常退出时显式收尾：同步冲一次。返回写入条数。
pub fn flush_and_stop(writer: &BatchWriter) -> rusqlite::Result<usize> {
    writer.flush()
}
