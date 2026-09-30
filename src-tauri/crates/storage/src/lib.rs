pub mod query;
pub mod schema;
pub mod writer;

pub use query::{get_events_in_range, insert_events, now_ms, StoredEvent};
pub use schema::{apply_pragmas, migrate, SCHEMA_VERSION};

use rusqlite::Connection;
use std::path::Path;

/// 测试用：内存库，已应用 PRAGMA 与迁移。
pub fn open_in_memory() -> Connection {
    let conn = Connection::open_in_memory().expect("open in-memory db");
    apply_pragmas(&conn).expect("apply pragmas");
    migrate(&conn).expect("migrate");
    conn
}

/// 生产用：文件库。父目录不存在时自动创建（spec §8.1 的 `%APPDATA%/time-scope/`）。
pub fn open_file(path: &Path) -> rusqlite::Result<Connection> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            // io::Error 不在 rusqlite::Error 的 From 列表里，手工映射。
            std::fs::create_dir_all(dir).map_err(|e| {
                rusqlite::Error::ToSqlConversionFailure(Box::new(e))
            })?;
        }
    }
    let conn = Connection::open(path)?;
    apply_pragmas(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}
