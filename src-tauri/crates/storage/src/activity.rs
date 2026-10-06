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
/// 否则重算后的旧证据会残留。**跨批**覆盖同 id 是重放的正常路径。
///
/// 但**同一批里**出现两个同 id 的段是另一回事：段 id 只由 `start_at` 派生
/// （`seg-{start_at}`），两个段起点相同就会撞上，而 `INSERT OR REPLACE` 会让
/// 后者静默覆盖前者、前者的 evidence 也被一并删掉 —— 一段活动就这么没了，
/// 返回值还是 `Ok`。这里在开事务**之前**就把它拦下来并返回错误（B11）：
/// 宁可整批不写（可事后重放），也不要静默丢数据。
pub fn insert_segments(
    conn: &Connection,
    segments: &[(StoredSegment, Vec<String>)],
) -> rusqlite::Result<()> {
    if segments.is_empty() {
        return Ok(());
    }
    reject_duplicate_ids(segments)?;
    let tx = conn.unchecked_transaction()?;
    write_all(&tx, segments)?;
    tx.commit()
}

/// 同一批里出现重复 id 就报错。**必须在开事务之前**调 —— 报错时库里
/// 不能留下写了一半的数据。
fn reject_duplicate_ids(segments: &[(StoredSegment, Vec<String>)]) -> rusqlite::Result<()> {
    let mut seen = std::collections::HashSet::with_capacity(segments.len());
    for (s, _) in segments {
        if !seen.insert(s.id.as_str()) {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "同一批里出现重复的段 id {:?}（id 只由 start_at 派生，\
                 两个段起点相同就会撞上）。整批已拒绝写入，没有落库。",
                s.id
            )));
        }
    }
    Ok(())
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
    delete_intersecting(&tx, start_ms, end_ms)?;
    tx.commit()
}

/// **单事务**「删掉与 `[start_ms, end_ms)` 相交的段 + 写入新段」。
///
/// 拆成"删一个事务、写另一个事务"会有一个很难看的中间态：删成功、写失败
/// （`SQLITE_FULL`、磁盘满、进程被杀）→ 那天被清空，而且调用方通常已经把
/// 这一天标记成"处理过了"，于是**本进程内再也不会重放**，用户看到空白直到
/// 重启（审计 §3 B5）。合成一个事务后，要么全成、要么全不成。
pub fn replace_day_segments(
    conn: &Connection,
    start_ms: i64,
    end_ms: i64,
    segments: &[(StoredSegment, Vec<String>)],
) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    delete_intersecting(&tx, start_ms, end_ms)?;
    write_all(&tx, segments)?;
    tx.commit()
}

/// 在已有事务里删掉相交的段及其证据。
fn delete_intersecting(tx: &rusqlite::Transaction<'_>, start_ms: i64, end_ms: i64) -> rusqlite::Result<()> {
    // 证据表的外键指向 activities，SQLite 不会级联删除引用方，所以手动清。
    tx.prepare(
        "DELETE FROM activity_evidence
         WHERE activity_id IN (
             SELECT id FROM activities WHERE start_at < ?2 AND end_at > ?1
         )",
    )?
    .execute(rusqlite::params![start_ms, end_ms])?;
    tx.prepare("DELETE FROM activities WHERE start_at < ?2 AND end_at > ?1")?
        .execute(rusqlite::params![start_ms, end_ms])?;
    Ok(())
}

/// 在已有事务里写入 segments 及其 evidence。
fn write_all(
    tx: &rusqlite::Transaction<'_>,
    segments: &[(StoredSegment, Vec<String>)],
) -> rusqlite::Result<()> {
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
    Ok(())
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
