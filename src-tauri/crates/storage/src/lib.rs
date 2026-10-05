pub mod activity;
pub mod query;
pub mod schema;
pub mod summary;
pub mod writer;

pub use activity::{
    delete_segments_for_day, get_segments_in_range, insert_segments, StoredSegment,
};
pub use query::{get_events_in_range, insert_events, now_ms, StoredEvent};
pub use schema::{apply_pragmas, current_version, migrate, SCHEMA_VERSION};
pub use summary::{
    daily_calendar, range_totals, AppSlice, DailyCalendar, DayCell, DonutSlice, RangeTotals,
};
pub use writer::BatchWriter;

use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// 跨线程共享的连接。
///
/// `rusqlite::Connection` 是 `Send` 但**不是** `Sync`，所以不能直接 `Arc<Connection>`
/// 丢给后台写盘线程。Mutex 是单文件、单写入者场景下最省事且正确的包法。
pub type SharedConn = Arc<Mutex<Connection>>;

/// 测试用：内存库，已应用 PRAGMA 与迁移。
pub fn open_in_memory() -> Connection {
    let conn = Connection::open_in_memory().expect("open in-memory db");
    apply_pragmas(&conn).expect("apply pragmas");
    migrate(&conn).expect("migrate");
    conn
}

/// 同 `open_in_memory`，但包成可跨线程共享的形式（`BatchWriter` 需要）。
pub fn open_in_memory_shared() -> SharedConn {
    Arc::new(Mutex::new(open_in_memory()))
}

/// 同 `open_file`，但包成可跨线程共享的形式（`BatchWriter` 需要）。
pub fn open_file_shared(path: &Path) -> rusqlite::Result<SharedConn> {
    Ok(Arc::new(Mutex::new(open_file(path)?)))
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
