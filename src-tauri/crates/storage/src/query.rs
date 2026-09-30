use activity_core::Event;
use rusqlite::Connection;

/// 落库后再读出的 Event。`type` 列与 `payload` JSON 并存：前者供索引和筛选，
/// 后者供前端展示细节（spec §8）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredEvent {
    pub id: String,
    /// Unix 毫秒
    pub timestamp: i64,
    /// 与 `EventType::type_tag()` 一致
    #[serde(rename = "type")]
    pub type_: String,
    /// `EventType` 的 JSON
    pub payload: String,
}

/// 单事务批量插入（spec §8.1）。
///
/// `INSERT OR REPLACE`：id 是 `<ts><counter>` 形式的单调 id，正常路径不会撞；
/// 撞上也只会覆盖同一条 Event，不会产生重复。
pub fn insert_events(conn: &Connection, events: &[Event]) -> rusqlite::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR REPLACE INTO events (id, timestamp, type, payload, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        let now = now_ms();
        for e in events {
            let type_name = e.event_type.type_tag();
            let payload = serde_json::to_string(&e.event_type)
                .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
            stmt.execute(rusqlite::params![&e.id, e.timestamp, type_name, payload, now])?;
        }
    }
    tx.commit()
}

/// 查询 `[start_ms, end_ms)` 半开区间内的事件，按 timestamp 升序。
///
/// 半开而非闭区间，是因为 Task 9 的"某一天"是 `[00:00, 次日00:00)`——
/// 闭区间会让零点整的 Event 被相邻两天各算一次（Review Focus #2）。
pub fn get_events_in_range(
    conn: &Connection,
    start_ms: i64,
    end_ms: i64,
) -> rusqlite::Result<Vec<StoredEvent>> {
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, type, payload FROM events
         WHERE timestamp >= ?1 AND timestamp < ?2
         ORDER BY timestamp ASC",
    )?;
    let rows = stmt.query_map(rusqlite::params![start_ms, end_ms], |row| {
        Ok(StoredEvent {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            type_: row.get(2)?,
            payload: row.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
