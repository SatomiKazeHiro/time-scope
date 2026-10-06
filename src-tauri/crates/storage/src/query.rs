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

/// 删掉 `timestamp < cutoff_ms` 的**原始事件**，返回删了几条（B3，spec §8.1）。
///
/// **只删 `events`。** `activities` 一行不动 —— 它才是时长账本，
/// 时间线、汇总、热力图全都只读它。连坐会让那些数字凭空少掉一截。
/// 受影响的只有「窗口标题 Top」与段详情里能回溯到的标题范围，
/// 这正是留 365 天（够盖住热力图默认那面 53 周的墙）的理由。
///
/// cutoff 是**排他**的：恰好等于它的那条保留，边界那一天不会差 1 毫秒丢数据。
///
/// 删掉的页会进 freelist 并被后续写入复用，所以**这个函数本身就止住了增长**；
/// 想让已经很大的文件真正变小还需要 `VACUUM`（由 app 层决定何时做 ——
/// 它要锁库，不能放在采集路径上）。
pub fn delete_events_older_than(conn: &Connection, cutoff_ms: i64) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM events WHERE timestamp < ?1", rusqlite::params![cutoff_ms])
}
