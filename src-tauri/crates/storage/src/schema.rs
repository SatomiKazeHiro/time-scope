//! SQLite schema 与迁移。
//!
//! 迁移是**按 `user_version` 顺序应用**的：`MIGRATIONS` 是一个 `(目标版本, SQL)` 的
//! 有序表，`migrate` 只执行那些目标版本大于当前 `user_version` 的条目。
//!
//! 这样加第 2 版时只需往表里追加一条：
//!
//! ```ignore
//! (2, r#"ALTER TABLE events ADD COLUMN foo TEXT"#),
//! ```
//!
//! 已有数据库会在下次启动时自动升到 2，而新建库会从 0 一路升到最新。
//! **不要**再写"每次启动都重跑全部 CREATE TABLE"的版本——那会让 ALTER TABLE
//! 在第二次启动时因"列已存在"而失败，或者干脆永远不执行。

use rusqlite::Connection;

/// 当前 schema 版本。每加一条迁移就 +1。
pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

/// 有序迁移表。索引 i 对应版本 i+1。
const MIGRATIONS: &[(i64, &str)] = &[
    (
        1,
        r#"
        -- spec §8：Phase 1 三张表。
        CREATE TABLE IF NOT EXISTS events (
            id TEXT PRIMARY KEY,
            timestamp INTEGER NOT NULL,
            type TEXT NOT NULL,
            payload TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
        CREATE INDEX IF NOT EXISTS idx_events_type_timestamp ON events(type, timestamp);

        CREATE TABLE IF NOT EXISTS activities (
            id TEXT PRIMARY KEY,
            start_at INTEGER NOT NULL,
            end_at INTEGER NOT NULL,
            category TEXT NOT NULL,
            application TEXT,
            confidence REAL NOT NULL,
            classifier TEXT NOT NULL,
            version TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_activities_range ON activities(start_at, end_at);

        CREATE TABLE IF NOT EXISTS activity_evidence (
            activity_id TEXT NOT NULL REFERENCES activities(id),
            event_id TEXT NOT NULL REFERENCES events(id),
            PRIMARY KEY (activity_id, event_id)
        );
        "#,
    ),
];

pub fn apply_pragmas(conn: &Connection) -> rusqlite::Result<()> {
    // WAL：读写并发，崩溃时最多丢最后一个未 checkpoint 的事务。
    // 对内存库无效（会回退到 memory），这没关系——内存库只用于测试。
    conn.pragma_update(None, "journal_mode", "WAL")?;
    // NORMAL 而非 FULL：崩溃最多丢最后几秒 Event，spec §8.1 明确接受这个取舍。
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    // activity_evidence 的外键约束默认是关的，不开就没有引用完整性。
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

pub fn current_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
}

/// 按版本顺序把库升到 `SCHEMA_VERSION`。每条迁移各自一个事务，失败不回滚已完成的。
pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let mut version = current_version(conn)?;

    for (target, sql) in MIGRATIONS {
        if *target <= version {
            continue;
        }
        // 单条迁移必须能在自己的事务里跑完：SQLite 的 DDL 是事务性的。
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", *target)?;
        tx.commit()?;
        version = *target;
    }

    Ok(())
}
