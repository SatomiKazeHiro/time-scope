use rusqlite::Connection;

/// 落库后再读出的 ActivitySegment。
/// `#[serde(rename_all = "camelCase")]` 让前端 TS 侧直接同名对接。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSegment {
    pub id: String,
    pub start_at: i64,
    pub end_at: i64,
    pub category: String,
    pub application: Option<String>,
    pub confidence: f32,
    pub classifier: String,
    pub classifier_version: String,
    #[serde(default)]
    pub evidence_event_ids: Vec<String>,
}

/// 单事务写入 segments 及其 evidence。`(段, 该段的 evidence event id)` 配对。
///
/// 覆盖写（`INSERT OR REPLACE`）时先删掉该段已有的 evidence 再插新的，
/// 否则重算后的旧证据会残留。
pub fn insert_segments(
    conn: &Connection,
    segments: &[(StoredSegment, Vec<String>)],
) -> rusqlite::Result<()> {
    if segments.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    {
        let mut ins = tx.prepare(
            "INSERT OR REPLACE INTO activities
                (id, start_at, end_at, category, application, confidence, classifier, version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        let mut del = tx.prepare("DELETE FROM activity_evidence WHERE activity_id = ?1")?;
        let mut ev = tx.prepare(
            "INSERT OR IGNORE INTO activity_evidence (activity_id, event_id) VALUES (?1, ?2)",
        )?;
        for (s, evidence) in segments {
            ins.execute(rusqlite::params![
                &s.id,
                s.start_at,
                s.end_at,
                &s.category,
                &s.application,
                s.confidence,
                &s.classifier,
                &s.classifier_version,
            ])?;
            del.execute(rusqlite::params![&s.id])?;
            for e in evidence {
                ev.execute(rusqlite::params![&s.id, e])?;
            }
        }
    }
    tx.commit()
}

/// 清掉与 `[start_ms, end_ms)` **相交**的段及其证据。**重放某一天前必须调用**（spec §7.4）。
///
/// 按相交而不是按 `start_at` 落在区间内：跨零点的段 `start_at` 在前一天，
/// 旧口径删不掉它，于是「重放 10-06」写进去的新段会和它重叠。
/// 段是整行存整行的，所以判定按相交、删除也整行删。
pub fn delete_segments_for_day(
    conn: &Connection,
    start_ms: i64,
    end_ms: i64,
) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        // 证据表的外键指向 activities，SQLite 不会级联删除引用方，所以手动清。
        let mut del_ev = tx.prepare(
            "DELETE FROM activity_evidence
             WHERE activity_id IN (
                 SELECT id FROM activities WHERE start_at < ?2 AND end_at > ?1
             )",
        )?;
        del_ev.execute(rusqlite::params![start_ms, end_ms])?;
        let mut del =
            tx.prepare("DELETE FROM activities WHERE start_at < ?2 AND end_at > ?1")?;
        del.execute(rusqlite::params![start_ms, end_ms])?;
    }
    tx.commit()
}

/// 「与 `[?1, ?2)` 相交、并把起止裁剪到该区间」的投影与判定。
///
/// 时间线（`get_segments_in_range`）与节奏（`get_segment_spans_in_range`）
/// 共用这一份 SQL 片段：**两条查询的口径必须同源**（A2），复制粘贴会漂。
/// `pacing_spans_match_the_timeline_spans` 钉住这一点。
const CLIPPED_SPANS_SELECT: &str = "SELECT MAX(start_at, ?1) AS start_at, \
                                    MIN(end_at, ?2) AS end_at, category";
const CLIPPED_SPANS_FROM: &str = "FROM activities \
                                  WHERE start_at < ?2 AND end_at > ?1 \
                                  ORDER BY start_at ASC";

/// 与 `[start_ms, end_ms)` **相交**的段，且起止**裁剪到该区间**。
///
/// 为什么是相交而不是 `start_at` 落在区间内（A2）：跨零点的段 `start_at` 在前一天，
/// 按 `start_at` 查会让它在第二天整个消失——时间线缺一块、汇总是 0。
///
/// 为什么裁剪：时间线画的是「当天 24 小时」，不裁的话第二天的色块会从 x<0 开始画出去；
/// 汇总口径要的是「落在区间内的那部分时长」。裁剪让两者算出同一个数。
/// 已在 `EngineRuntime::segments_for_day` 上确立过同一条规矩（正在生长的当前段就裁）。
///
/// **注意这是重的那条路**：除段本身外，它还对**每个段**再跑一次 evidence 查询
/// （`SELECT event_id FROM activity_evidence WHERE activity_id = ?`），
/// 所以是 N+1。只在真的要展示证据时用（时间线、段详情）；
/// 只需要区间与类别的调用方请走 `get_segment_spans_in_range`（B1）。
pub fn get_segments_in_range(
    conn: &Connection,
    start_ms: i64,
    end_ms: i64,
) -> rusqlite::Result<Vec<StoredSegment>> {
    // 分两趟：先取段，再补证据。避免在外层 stmt 还没 drop 时又发起查询。
    let mut segments: Vec<StoredSegment> = {
        let sql = format!(
            "{}, id, application, confidence, classifier, version {}",
            CLIPPED_SPANS_SELECT, CLIPPED_SPANS_FROM
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params![start_ms, end_ms], |row| {
            Ok(StoredSegment {
                start_at: row.get(0)?,
                end_at: row.get(1)?,
                category: row.get(2)?,
                id: row.get(3)?,
                application: row.get(4)?,
                confidence: row.get(5)?,
                classifier: row.get(6)?,
                classifier_version: row.get(7)?,
                evidence_event_ids: Vec::new(),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };

    let mut ev_stmt =
        conn.prepare("SELECT event_id FROM activity_evidence WHERE activity_id = ?1 ORDER BY event_id")?;
    for s in segments.iter_mut() {
        let ids = ev_stmt.query_map(rusqlite::params![&s.id], |r| r.get::<_, String>(0))?;
        s.evidence_event_ids = ids.collect::<rusqlite::Result<Vec<_>>>()?;
    }

    Ok(segments)
}

/// 段在某个区间内的**跨度**（已裁剪）与类别。
///
/// `range_pacing`（切换次数 + 24h 分布）只需要这三个字段，原来却借道
/// `get_segments_in_range`，于是每个段都白跑一次 evidence 查询（B1）。
/// 实测 654 段 → 654 次查询、66,800 条证据进内存后一个字节都没用。
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentSpan {
    pub start_at: i64,
    pub end_at: i64,
    pub category: String,
}

/// 同 `get_segments_in_range` 的口径（相交 + 裁剪），但不碰证据表。
pub fn get_segment_spans_in_range(
    conn: &Connection,
    start_ms: i64,
    end_ms: i64,
) -> rusqlite::Result<Vec<SegmentSpan>> {
    let sql = format!("{} {}", CLIPPED_SPANS_SELECT, CLIPPED_SPANS_FROM);
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params![start_ms, end_ms], |row| {
        Ok(SegmentSpan {
            start_at: row.get(0)?,
            end_at: row.get(1)?,
            category: row.get(2)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
}
