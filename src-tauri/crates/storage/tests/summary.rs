use activity_storage::{
    daily_calendar, insert_segments, open_in_memory, range_totals, StoredSegment,
};

fn seg(id: &str, start_ms: i64, end_ms: i64) -> StoredSegment {
    StoredSegment {
        id: id.into(),
        start_at: start_ms,
        end_at: end_ms,
        category: "work".into(),
        application: Some("Code.exe".into()),
        confidence: 1.0,
        classifier: "rule".into(),
        classifier_version: "1".into(),
        evidence_event_ids: Vec::new(),
    }
}

fn put(conn: &rusqlite::Connection, segs: Vec<StoredSegment>) {
    let refs: Vec<(StoredSegment, Vec<String>)> =
        segs.into_iter().map(|s| (s, Vec::new())).collect();
    insert_segments(conn, &refs).unwrap();
}

/// 某个 Unix 毫秒所在**本地日**的零点。借 SQLite 自己换算，
/// 避免测试里复刻时区 / 夏令时逻辑（spec §3.4 说那个口径是已知的近似）。
fn local_midnight_ms(conn: &rusqlite::Connection, ms: i64) -> i64 {
    conn.query_row(
        "SELECT CAST(strftime('%s', datetime(?/1000, 'unixepoch', 'localtime', 'start of day')) AS INTEGER) * 1000",
        rusqlite::params![ms],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
}

/// 三个连续的本地午夜：d0、d0+1天、d0+2天。
///
/// 注意：这里跨的是**本地日**而不是固定 86,400,000ms 夏令时切换的那天
/// 可能不是 24 小时（spec §3.4 记的已知限制）。测试跑在开发机上，
/// 撞上切换日会失败——那时改成向后再取一天即可。
fn three_days(conn: &rusqlite::Connection) -> (i64, i64, i64) {
    let now_ms: i64 = conn
        .query_row(
            "SELECT CAST(strftime('%s','now') AS INTEGER) * 1000",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let d0 = local_midnight_ms(conn, now_ms);
    (d0, d0 + 86_400_000, d0 + 2 * 86_400_000)
}

#[test]
fn empty_db_yields_none() {
    // Review Focus #1：空库是「刚装完还没跑满一天」的真实状态。
    // 必须返回 None，而不是 first/last 为空串、days 为空的半成品。
    let conn = open_in_memory();
    assert_eq!(daily_calendar(&conn).unwrap(), None, "空库应返回 None");
}

#[test]
fn single_day_is_returned() {
    let conn = open_in_memory();
    let (d0, _, _) = three_days(&conn);
    put(&conn, vec![seg("a", d0 + 3_600_000, d0 + 7_200_000)]);
    let cal = daily_calendar(&conn).unwrap().expect("有段就该有日历");
    assert_eq!(cal.days.len(), 1);
    assert_eq!(cal.days[0].total_ms, 3_600_000);
    assert_eq!(cal.first, cal.last);
    assert_eq!(cal.first, cal.days[0].date);
}

#[test]
fn same_day_segments_are_merged_into_one_row() {
    let conn = open_in_memory();
    let (d0, d1, _) = three_days(&conn);
    put(
        &conn,
        vec![
            seg("a", d0 + 3_600_000, d0 + 7_200_000), // 第 1 天，1 小时
            seg("b", d0 + 7_300_000, d0 + 7_600_000), // 同一天，再 5 分钟
            seg("c", d1 + 1_000, d1 + 61_000),        // 第 2 天，1 分钟
        ],
    );
    let cal = daily_calendar(&conn).unwrap().unwrap();
    assert_eq!(cal.days.len(), 2, "同一天的段必须合并成一行");
    assert_eq!(cal.days[0].total_ms, 3_600_000 + 300_000);
    assert_eq!(cal.days[1].total_ms, 60_000);
    assert!(
        cal.days[0].date < cal.days[1].date,
        "必须按日期升序"
    );
}

#[test]
fn days_with_no_segments_are_absent() {
    // 中间那天没段 -> SQL 不返回它。补齐是前端 fillDays 的活。
    let conn = open_in_memory();
    let (d0, _, d2) = three_days(&conn);
    put(
        &conn,
        vec![
            seg("a", d0 + 3_600_000, d0 + 7_200_000),
            seg("c", d2 + 3_600_000, d2 + 7_200_000),
        ],
    );
    let cal = daily_calendar(&conn).unwrap().unwrap();
    assert_eq!(cal.days.len(), 2, "中间那天没段就不该出现在 days 里");
}

#[test]
fn segments_are_bucketed_by_start_at_not_by_overlap() {
    // 跨零点的段整体算在**开始**那天。这与 get_segments_in_range 的
    // `start_at >= ?1 AND start_at < ?2` 口径一致 —— spec §7.3 要求两者一致，
    // 这条测试把口径钉死，防止有人只把其中一个改成区间相交。
    let conn = open_in_memory();
    let (d0, _, _) = three_days(&conn);
    let start = d0 + 86_700_000; // 当天 23:50
    let end = d0 + 90_000_000; // 次日 00:10
    put(&conn, vec![seg("cross", start, end)]);
    let cal = daily_calendar(&conn).unwrap().unwrap();
    assert_eq!(cal.days.len(), 1, "跨零点段只落在开始那天");
    assert_eq!(
        cal.days[0].total_ms,
        end - start,
        "整段时长算给开始那天"
    );
}

fn segc(
    id: &str,
    start_ms: i64,
    end_ms: i64,
    category: &str,
    app: Option<&str>,
) -> StoredSegment {
    StoredSegment {
        id: id.into(),
        start_at: start_ms,
        end_at: end_ms,
        category: category.into(),
        application: app.map(|a| a.into()),
        confidence: 1.0,
        classifier: "rule".into(),
        classifier_version: "1".into(),
        evidence_event_ids: Vec::new(),
    }
}

#[test]
fn totals_split_active_and_idle() {
    let conn = open_in_memory();
    put(
        &conn,
        vec![
            segc("a", 0, 3_600_000, "work", Some("Code.exe")),
            segc("b", 3_600_000, 5_400_000, "idle", None),
        ],
    );
    let t = range_totals(&conn, 0, 10_000_000).unwrap();
    assert_eq!(t.total_ms, 5_400_000);
    assert_eq!(t.active_ms, 3_600_000);
    assert_eq!(t.idle_ms, 1_800_000);
    assert_eq!(t.segment_count, 2);
}

#[test]
fn donut_always_has_exactly_four_slices_in_fixed_order() {
    // 前端按数组下标取色，顺序不能随数据变。
    let conn = open_in_memory();
    let t = range_totals(&conn, 0, 10_000_000).unwrap();
    let keys: Vec<&str> = t.donut.iter().map(|d| d.key.as_str()).collect();
    assert_eq!(keys, vec!["work", "browsing", "idle", "unknown"]);
    assert!(
        t.donut.iter().all(|d| d.ms == 0),
        "空范围时四档都该是 0"
    );
}

#[test]
fn donut_folds_four_categories_into_unknown() {
    // spec §6：学习/娱乐/社交/生活 并入「未分类」那一档，环才是完整 360°。
    let conn = open_in_memory();
    put(
        &conn,
        vec![
            segc("a", 0, 1_000, "work", Some("Code.exe")),
            segc("b", 2_000, 3_000, "study", Some("Zed.exe")),
            segc("c", 4_000, 5_000, "entertainment", Some("cloudmusic.exe")),
            segc("d", 6_000, 7_000, "communication", Some("WeChat.exe")),
            segc("e", 8_000, 9_000, "life", Some("explorer.exe")),
            segc("f", 10_000, 11_000, "unknown", Some("mstsc.exe")),
        ],
    );
    let t = range_totals(&conn, 0, 20_000_000).unwrap();
    let by = |k: &str| t.donut.iter().find(|d| d.key == k).unwrap().ms;
    assert_eq!(by("work"), 1_000);
    assert_eq!(
        by("unknown"),
        5_000,
        "study+entertainment+communication+life+unknown 都要并进来"
    );
    assert_eq!(
        t.donut.iter().map(|d| d.ms).sum::<i64>(),
        t.total_ms,
        "并档不改变总时长"
    );
}

#[test]
fn top_apps_ranked_desc_and_nulls_dropped() {
    let conn = open_in_memory();
    put(
        &conn,
        vec![
            segc("a", 0, 1_000, "browsing", Some("msedge.exe")),
            segc("b", 2_000, 12_000, "work", Some("Code.exe")),
            segc("c", 14_000, 17_000, "work", Some("Code.exe")),
            segc("d", 20_000, 30_000, "idle", None), // 没有 application
        ],
    );
    let t = range_totals(&conn, 0, 40_000_000).unwrap();
    assert_eq!(t.top_apps.len(), 2, "application IS NULL 的段要丢掉");
    assert_eq!(t.top_apps[0].name, "Code.exe");
    assert_eq!(t.top_apps[0].ms, 13_000);
    assert_eq!(t.top_apps[1].name, "msedge.exe");
}

#[test]
fn empty_range_yields_zeros_not_error() {
    // Review Focus #3：from > to 是公开 IPC 能收到的输入，不许炸。
    let conn = open_in_memory();
    put(
        &conn,
        vec![segc("a", 5_000_000, 6_000_000, "work", Some("Code.exe"))],
    );
    let t = range_totals(&conn, 9_000_000, 1_000_000).unwrap();
    assert_eq!(t.total_ms, 0);
    assert_eq!(t.segment_count, 0);
    assert!(t.top_apps.is_empty());
}

#[test]
fn range_is_half_open() {
    // 终点上的段不计入：与 get_segments_in_range 同一个半开约定。
    let conn = open_in_memory();
    put(
        &conn,
        vec![segc("a", 1_000_000, 2_000_000, "work", Some("Code.exe"))],
    );
    assert_eq!(
        range_totals(&conn, 0, 2_000_000).unwrap().segment_count,
        1,
        "start_at 等于 end 时不计入"
    );
    assert_eq!(
        range_totals(&conn, 2_000_000, 3_000_000).unwrap().segment_count,
        0,
        "start_at 恰好等于 end 时不计入"
    );
}
