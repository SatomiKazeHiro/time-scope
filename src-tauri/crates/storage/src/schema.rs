use rusqlite::Connection;

/// Phase 1 只有一个版本；后续加迁移时按 `user_version` 递增。
pub const SCHEMA_VERSION: i64 = 1;

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

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    // spec §8：Phase 1 三张表。IF NOT EXISTS 让重开同一文件是幂等的。
    conn.execute_batch(
        r#"
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
    )?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}
