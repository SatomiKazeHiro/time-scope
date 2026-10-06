//! 原始事件的保留期（审计 §3 B3）。
//!
//! `events` 表实测 ≈5 MB/天、≈1.8 GB/年，而全仓没有任何保留期或归档，
//! 于是「越用越卡」是一条曲线而不是天花板。删掉它不影响时长统计 ——
//! `activities`（时间线、汇总、热力图唯一的数据源）一行不删。
//!
//! 两处调用，时机与代价都不同：
//!
//! | 时机 | 做什么 | 为什么 |
//! |---|---|---|
//! | 启动时（采集线程起来**之前**） | 删 + `VACUUM` | VACUUM 会锁住整个库。放在启动时它挡住的是"还没开始采集"，放在运行期它挡住的是**采集本身** |
//! | 运行期，每 24h 至多一次 | 只删，不 VACUUM | 删掉的页进 freelist 会被后续写入复用，**这就止住了增长**；文件要缩回去得靠下次启动的 VACUUM |
//!
//! 保留天数来自 `config.toml` 的 `events_retention_days`（默认 365，0 = 永不删）。

use activity_storage::SharedConn;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Instant;

const DAY_MS: i64 = 86_400_000;
/// 运行期两次清理之间的最小间隔。
const RUNTIME_INTERVAL_MS: i64 = DAY_MS;

/// 上次清理的时刻（Unix 毫秒）。`0` = 从没清过，于是首次调用一定会跑。
static LAST_PRUNE_MS: AtomicI64 = AtomicI64::new(0);

/// 删掉 `retention_days` 天以前的事件，返回删了几条。
///
/// `retention_days == 0` 是"永不删"，直接返回 0 —— 注意它**不是**
/// "删掉 0 天前"，那样会把整张表清空。
pub fn prune_events(conn: &SharedConn, retention_days: u32, now_ms: i64) -> usize {
    if retention_days == 0 {
        return 0;
    }
    let cutoff = now_ms - i64::from(retention_days) * DAY_MS;
    let c = match conn.lock() {
        Ok(c) => c,
        Err(_) => return 0,
    };
    match activity_storage::delete_events_older_than(&c, cutoff) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("[time-scope] 清理过期事件失败（不影响采集）: {e}");
            0
        }
    }
}

/// 启动时清理 + VACUUM。**必须在采集线程起来之前调用。**
pub fn prune_at_startup(conn: &SharedConn, retention_days: u32) {
    if retention_days == 0 {
        return;
    }
    let t0 = Instant::now();
    let deleted = prune_events(conn, retention_days, activity_storage::now_ms());
    if deleted == 0 {
        return;
    }
    // 只有真删掉了东西才 VACUUM：它很贵，而空跑一次能把启动拖慢好几秒。
    let c = match conn.lock() {
        Ok(c) => c,
        Err(_) => return,
    };
    if let Err(e) = c.execute_batch("VACUUM") {
        eprintln!("[time-scope] VACUUM 失败（数据仍然正确，只是文件没变小）: {e}");
        return;
    }
    eprintln!(
        "[time-scope] 已清理 {} 天前的原始事件 {deleted} 条并回收空间，用时 {:.1}s",
        retention_days,
        t0.elapsed().as_secs_f64()
    );
    drop(c);
    let _ = LAST_PRUNE_MS.compare_exchange(0, activity_storage::now_ms(), Ordering::Relaxed, Ordering::Relaxed);
}

/// 运行期清理：**每 24h 至多一次，且不 VACUUM**。
///
/// 挂在 `get_segments` 这条稳定的高频 IPC 上，而不是为此再开一个定时线程 ——
/// 代价只是每次多一次原子读。
pub fn prune_if_due(conn: &SharedConn, retention_days: u32, now_ms: i64) {
    if retention_days == 0 {
        return;
    }
    let last = LAST_PRUNE_MS.load(Ordering::Relaxed);
    if now_ms.saturating_sub(last) < RUNTIME_INTERVAL_MS {
        return;
    }
    // compare_exchange 保证并发下只有一个调用者真的去删。
    // 上次清过 = 上一条 compare_exchange 已经把时间戳写进去了。
    if LAST_PRUNE_MS
        .compare_exchange(last, now_ms, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    let deleted = prune_events(conn, retention_days, now_ms);
    if deleted > 0 {
        eprintln!("[time-scope] 已清理 {deleted} 条过期原始事件");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use activity_core::{Event, EventType, InputHeartbeatPayload};
    use activity_storage::{get_events_in_range, insert_events, open_in_memory_shared};

    fn ev(ts: i64) -> Event {
        Event::new(
            EventType::InputHeartbeat(InputHeartbeatPayload { active_seconds: 1 }),
            ts,
        )
    }

    fn seeded() -> (SharedConn, i64) {
        let conn = open_in_memory_shared();
        insert_events(
            &conn.lock().unwrap(),
            &[ev(0), ev(DAY_MS), ev(2 * DAY_MS)],
        )
        .unwrap();
        (conn, 3 * DAY_MS)
    }

    fn count(conn: &SharedConn) -> i64 {
        get_events_in_range(&conn.lock().unwrap(), 0, i64::MAX)
            .unwrap()
            .len() as i64
    }

    #[test]
    fn zero_days_means_keep_everything() {
        // 关键：0 是"永不删"，不是"删掉 0 天前"（那会清空整张表）。
        let (conn, now) = seeded();
        assert_eq!(prune_events(&conn, 0, now), 0);
        assert_eq!(count(&conn), 3, "一行都不该少");
    }

    #[test]
    fn retention_deletes_only_what_is_older_than_the_window() {
        let (conn, now) = seeded();
        // 保留 1 天：now=3d，cutoff=2d，只有 ts=0 与 ts=1d 该删
        assert_eq!(prune_events(&conn, 1, now), 2);
        assert_eq!(count(&conn), 1);
    }

    #[test]
    fn a_window_longer_than_the_data_deletes_nothing() {
        let (conn, now) = seeded();
        assert_eq!(prune_events(&conn, 3650, now), 0);
        assert_eq!(count(&conn), 3);
    }

    #[test]
    fn the_runtime_prune_runs_once_then_waits_a_day() {
        // 连着调 10 次只该删一次 —— 否则每次翻页/轮询都去扫一遍整张 events。
        LAST_PRUNE_MS.store(0, Ordering::Relaxed);
        let (conn, now) = seeded();
        for _ in 0..10 {
            prune_if_due(&conn, 1, now);
        }
        // 第一次把 0 和 1d 删了，剩下 2d 那条
        assert_eq!(count(&conn), 1);

        // 把剩下那条也变成过期的，但只过了 1 小时 → 不到一天，不该再删
        prune_if_due(&conn, 0, now + 3 * 3_600_000);
        assert_eq!(count(&conn), 1, "不到 24h 不该重复清理");
        LAST_PRUNE_MS.store(0, Ordering::Relaxed);
    }

    #[test]
    fn retention_zero_disables_the_runtime_prune_entirely() {
        LAST_PRUNE_MS.store(0, Ordering::Relaxed);
        let (conn, now) = seeded();
        prune_if_due(&conn, 0, now);
        assert_eq!(count(&conn), 3);
        assert_eq!(
            LAST_PRUNE_MS.load(Ordering::Relaxed),
            0,
            "设为 0 时连时间戳都不该更新"
        );
    }
}