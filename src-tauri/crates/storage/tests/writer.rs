use activity_core::{Event, EventType};
use activity_storage::{get_events_in_range, open_in_memory_shared, BatchWriter};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn ev(ts: i64) -> Event {
    Event::new(EventType::SystemIdle, ts)
}

#[test]
fn push_then_flush_writes_all() {
    let conn = open_in_memory_shared();
    let w = BatchWriter::new(Arc::clone(&conn), 60_000, 100);
    for i in 0..10 {
        w.push(ev(1000 + i));
    }
    assert_eq!(w.flush().unwrap(), 10);
    let rows = get_events_in_range(&conn.lock().unwrap(), 0, i64::MAX).unwrap();
    assert_eq!(rows.len(), 10);
}

#[test]
fn flush_on_empty_is_noop() {
    let conn = open_in_memory_shared();
    let w = BatchWriter::new(Arc::clone(&conn), 60_000, 100);
    assert_eq!(w.flush().unwrap(), 0);
}

#[test]
fn repeated_flush_does_not_duplicate_rows() {
    let conn = open_in_memory_shared();
    let w = BatchWriter::new(Arc::clone(&conn), 60_000, 100);
    w.push(ev(1000));
    assert_eq!(w.flush().unwrap(), 1);
    assert_eq!(w.flush().unwrap(), 0);
    assert_eq!(get_events_in_range(&conn.lock().unwrap(), 0, i64::MAX).unwrap().len(), 1);
}

#[test]
fn background_thread_auto_flushes_without_manual_call() {
    let conn = open_in_memory_shared();
    // 60s 间隔 / batch_size 100：任何自动触发路径都不该在 500ms 内发生，
    // 所以这里能落盘只能靠 flush_interval=0 的定时路径……用 0 才会自动刷。
    let w = BatchWriter::new(Arc::clone(&conn), 0, 100);
    w.push(ev(1000));
    w.push(ev(2000));
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        get_events_in_range(&conn.lock().unwrap(), 0, i64::MAX).unwrap().len(),
        2,
        "background thread should flush on its own"
    );
}

#[test]
fn long_interval_does_not_flush_on_its_own() {
    let conn = open_in_memory_shared();
    let w = BatchWriter::new(Arc::clone(&conn), 60_000, 100);
    w.push(ev(1000));
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        get_events_in_range(&conn.lock().unwrap(), 0, i64::MAX).unwrap().len(),
        0,
        "60s interval + 1 queued row must not hit disk yet"
    );
}

#[test]
fn batch_size_threshold_triggers_flush_below_interval() {
    let conn = open_in_memory_shared();
    // 间隔 60s（定时路径不会触发），batch_size 2：只有"队满"路径能落盘。
    let w = BatchWriter::new(Arc::clone(&conn), 60_000, 2);
    w.push(ev(1000));
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(get_events_in_range(&conn.lock().unwrap(), 0, i64::MAX).unwrap().len(), 0);
    w.push(ev(2000)); // 到达 batch_size
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline
        && get_events_in_range(&conn.lock().unwrap(), 0, i64::MAX).unwrap().len() < 2
    {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        get_events_in_range(&conn.lock().unwrap(), 0, i64::MAX).unwrap().len(),
        2,
        "queue reaching batch_size must trigger a flush without waiting for the interval"
    );
}

#[test]
fn drop_flushes_remaining_queue() {
    let conn = open_in_memory_shared();
    {
        let w = BatchWriter::new(Arc::clone(&conn), 60_000, 100);
        w.push(ev(1000));
        w.push(ev(2000));
        w.push(ev(3000));
        // 退出作用域时 drop 应尽力冲刷队列
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline
        && get_events_in_range(&conn.lock().unwrap(), 0, i64::MAX).unwrap().len() < 3
    {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        get_events_in_range(&conn.lock().unwrap(), 0, i64::MAX).unwrap().len(),
        3,
        "queued events must survive writer drop"
    );
}

#[test]
fn concurrent_pushes_all_land_exactly_once() {
    let conn = open_in_memory_shared();
    let w = Arc::new(BatchWriter::new(Arc::clone(&conn), 60_000, 1000));
    let mut handles = Vec::new();
    for t in 0..4 {
        let w = Arc::clone(&w);
        handles.push(std::thread::spawn(move || {
            for i in 0..25 {
                w.push(ev(1000 + t * 1000 + i));
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(w.flush().unwrap(), 100);
    assert_eq!(get_events_in_range(&conn.lock().unwrap(), 0, i64::MAX).unwrap().len(), 100);
}
