use activity_storage::open_in_memory;

#[test]
fn creates_expected_tables_and_indexes() {
    let conn = open_in_memory();
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type IN ('table','index')")
        .unwrap();
    let names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    for expected in [
        "events",
        "activities",
        "activity_evidence",
        "idx_events_timestamp",
        "idx_events_type_timestamp",
        "idx_activities_range",
    ] {
        assert!(
            names.iter().any(|n| n == expected),
            "missing {expected} in {names:?}"
        );
    }
}

#[test]
fn journal_mode_is_wal() {
    let conn = open_in_memory();
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    // 内存库无法用 WAL，会回退到 memory；此断言只确认 PRAGMA 可执行且未报错。
    assert!(
        mode == "memory" || mode == "wal",
        "unexpected journal_mode {mode}"
    );
}

#[test]
fn file_db_uses_wal() {
    // 内存库退化为 memory，WAL 只能在真文件上验证（spec §8.1）。
    let dir = std::env::temp_dir().join(format!("ts-mig-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.db");
    let _ = std::fs::remove_file(&path);
    let conn = activity_storage::open_file(&path).unwrap();
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    let uv: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(uv, activity_storage::SCHEMA_VERSION);
    drop(conn);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);
}

#[test]
fn migrate_is_idempotent() {
    let dir = std::env::temp_dir().join(format!("ts-idem-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.db");
    let _ = std::fs::remove_file(&path);
    {
        let _c = activity_storage::open_file(&path).unwrap();
    }
    // 第二次打开同一文件：CREATE TABLE IF NOT EXISTS 不应报错
    let _c = activity_storage::open_file(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);
}

#[test]
fn open_file_creates_missing_parent_directory() {
    let dir = std::env::temp_dir().join(format!("ts-nested-{}-a-b", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("deeper").join("t.db");
    let conn = activity_storage::open_file(&path).unwrap();
    assert!(path.exists());
    drop(conn);
    let _ = std::fs::remove_dir_all(&dir);
}

// --- 迁移版本化 ---

#[test]
fn migrate_is_a_noop_when_already_at_current_version() {
    let conn = open_in_memory();
    let before = activity_storage::current_version(&conn).unwrap();
    activity_storage::migrate(&conn).unwrap();
    assert_eq!(activity_storage::current_version(&conn).unwrap(), before);
    assert_eq!(before, activity_storage::SCHEMA_VERSION);
}

#[test]
fn migrate_lifts_a_version_0_database_to_current() {
    // 模拟"还没记录版本的老库"（user_version 默认 0）
    let conn = open_in_memory();
    conn.pragma_update(None, "user_version", 0i64).unwrap();
    // 把表删掉，模拟结构不存在
    conn.execute_batch("DROP TABLE events").unwrap();

    activity_storage::migrate(&conn).unwrap();

    assert_eq!(
        activity_storage::current_version(&conn).unwrap(),
        activity_storage::SCHEMA_VERSION
    );
    // 迁移后表应该被重新建出来
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM sqlite_master WHERE name='events'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1, "迁移应重建缺失的表");
}

#[test]
fn migrate_does_not_reapply_already_applied_versions() {
    // 若 migrate 无条件重跑全部 SQL，第二次会在 ALTER 类迁移上失败。
    // 现在只有 CREATE IF NOT EXISTS，所以用 pragma 计数验证"没有多余副作用"。
    let conn = open_in_memory();
    activity_storage::migrate(&conn).unwrap();
    let count = |c: &rusqlite::Connection, sql: &str| -> i64 {
        c.query_row(sql, [], |r| r.get(0)).unwrap()
    };
    let first_tables = count(&conn, "SELECT COUNT(*) FROM sqlite_master WHERE type='table'");
    let first_idx = count(&conn, "SELECT COUNT(*) FROM sqlite_master WHERE type='index'");
    activity_storage::migrate(&conn).unwrap();
    activity_storage::migrate(&conn).unwrap();
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM sqlite_master WHERE type='table'"), first_tables);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM sqlite_master WHERE type='index'"), first_idx);
}

#[test]
fn current_version_is_reported_for_diagnostics() {
    let conn = open_in_memory();
    assert_eq!(
        activity_storage::current_version(&conn).unwrap(),
        activity_storage::SCHEMA_VERSION
    );
}
