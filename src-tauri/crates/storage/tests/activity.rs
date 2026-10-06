use activity_storage::{
    delete_segments_for_day, get_segments_in_range, insert_segments, open_in_memory,
    StoredSegment,
};

fn seg(id: &str, start: i64, end: i64, cat: &str) -> StoredSegment {
    StoredSegment {
        id: id.into(),
        start_at: start,
        end_at: end,
        category: cat.into(),
        application: Some("Code.exe".into()),
        confidence: 0.9,
        classifier: "rule".into(),
        classifier_version: "rules:3".into(),
        evidence_event_ids: vec![],
    }
}

#[test]
fn insert_and_read_back_a_segment() {
    let conn = open_in_memory();
    let s = seg("s1", 0, 60_000, "work");
    insert_segments(&conn, &[(s, vec!["e1".into(), "e2".into()])]).unwrap();
    let rows = get_segments_in_range(&conn, 0, 100_000).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "s1");
    assert_eq!(rows[0].category, "work");
    assert_eq!(rows[0].application.as_deref(), Some("Code.exe"));
    assert_eq!(rows[0].evidence_event_ids.len(), 2);
}

#[test]
fn range_is_half_open_and_ordered() {
    let conn = open_in_memory();
    insert_segments(
        &conn,
        &[
            (seg("b", 60_000, 120_000, "work"), vec![]),
            (seg("a", 0, 60_000, "work"), vec![]),
        ],
    )
    .unwrap();
    let rows = get_segments_in_range(&conn, 0, 120_000).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, "a", "应按 start_at 升序");
    assert_eq!(rows[1].id, "b");
    // 恰好等于 end 的不收
    assert_eq!(get_segments_in_range(&conn, 0, 60_000).unwrap().len(), 1);
}

#[test]
fn evidence_is_deduplicated() {
    let conn = open_in_memory();
    insert_segments(
        &conn,
        &[(seg("s1", 0, 1000, "work"), vec!["e1".into(), "e1".into()])],
    )
    .unwrap();
    let rows = get_segments_in_range(&conn, 0, 2000).unwrap();
    assert_eq!(rows[0].evidence_event_ids.len(), 1, "PRIMARY KEY 应去重");
}

#[test]
fn reinserting_same_segment_id_replaces_and_drops_stale_evidence() {
    let conn = open_in_memory();
    insert_segments(&conn, &[(seg("s1", 0, 1000, "work"), vec!["e1".into()])]).unwrap();
    insert_segments(
        &conn,
        &[(seg("s1", 0, 2000, "browsing"), vec!["e9".into()])],
    )
    .unwrap();
    let rows = get_segments_in_range(&conn, 0, 5000).unwrap();
    assert_eq!(rows.len(), 1, "同 id 应覆盖而非重复");
    assert_eq!(rows[0].category, "browsing");
    assert_eq!(rows[0].end_at, 2000);
    assert_eq!(
        rows[0].evidence_event_ids,
        vec!["e9".to_string()],
        "覆盖写后旧证据不该残留"
    );
}

#[test]
fn insert_empty_slice_is_noop() {
    let conn = open_in_memory();
    insert_segments(&conn, &[]).unwrap();
    assert!(get_segments_in_range(&conn, 0, i64::MAX).unwrap().is_empty());
}

#[test]
fn delete_removes_segments_and_their_evidence_only() {
    let conn = open_in_memory();
    insert_segments(
        &conn,
        &[
            (seg("old", 0, 1000, "work"), vec!["e1".into()]),
            (seg("new", 5_000_000, 5_001_000, "work"), vec!["e2".into()]),
        ],
    )
    .unwrap();
    delete_segments_for_day(&conn, 0, 1_000_000).unwrap();
    let rows = get_segments_in_range(&conn, 0, i64::MAX).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "new");
    let orphans: i64 = conn
        .query_row("SELECT COUNT(*) FROM activity_evidence", [], |r| r.get(0))
        .unwrap();
    assert_eq!(orphans, 1, "只应剩下 new 的证据");
}

#[test]
fn delete_is_idempotent() {
    let conn = open_in_memory();
    insert_segments(&conn, &[(seg("s1", 0, 1000, "work"), vec![])]).unwrap();
    delete_segments_for_day(&conn, 0, 1_000_000).unwrap();
    delete_segments_for_day(&conn, 0, 1_000_000).unwrap();
    assert!(get_segments_in_range(&conn, 0, i64::MAX).unwrap().is_empty());
}

#[test]
fn segments_with_null_application_roundtrip() {
    let conn = open_in_memory();
    let mut s = seg("s1", 0, 1000, "idle");
    s.application = None;
    insert_segments(&conn, &[(s, vec![])]).unwrap();
    assert!(get_segments_in_range(&conn, 0, 2000).unwrap()[0]
        .application
        .is_none());
}

#[test]
fn empty_range_returns_empty() {
    let conn = open_in_memory();
    insert_segments(&conn, &[(seg("s1", 0, 1000, "work"), vec![])]).unwrap();
    assert!(get_segments_in_range(&conn, 90_000, 100_000).unwrap().is_empty());
}

#[test]
fn evidence_may_reference_events_not_yet_persisted() {
    // 采集链路的写入顺序决定了：段会先于它引用的 Event 落库（Event 还在内存队列里）。
    // activity_evidence.event_id 因此是软引用，不该有外键约束。
    let conn = open_in_memory();
    let n_events: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n_events, 0, "events 表此刻是空的");
    insert_segments(
        &conn,
        &[(seg("s1", 0, 1000, "work"), vec!["not-inserted-yet".into()])],
    )
    .expect("引用尚未落库的 Event 不该报外键错误");
    let rows = get_segments_in_range(&conn, 0, 2000).unwrap();
    assert_eq!(rows[0].evidence_event_ids, vec!["not-inserted-yet".to_string()]);
}

// --- A2：段归属用区间相交，而不是只看 start_at ---

#[test]
fn a_segment_crossing_midnight_is_returned_for_the_second_day() {
    // A2：只看 `start_at` 落在哪一天，跨零点的段在第二天**整个消失**。
    // 真实库里 `seg-1791215866412`（10-05 23:57:46 → 10-06 00:00:22）就是这样：
    // 10-06 的时间线 00:00–00:00:22 缺一块，10-06 的汇总是 0。
    let conn = open_in_memory();
    // 第 1 天 = [0, 86_400_000)
    insert_segments(&conn, &[(seg("cross", 86_300_000, 86_422_000, "work"), vec![])]).unwrap();

    let day2 = get_segments_in_range(&conn, 86_400_000, 2 * 86_400_000).unwrap();
    assert_eq!(day2.len(), 1, "跨零点的段第二天也必须查得到");
    assert_eq!(day2[0].id, "cross");
}

#[test]
fn a_segment_crossing_midnight_is_clipped_to_the_requested_day() {
    // 时间线画的是「当天 24 小时」。不裁剪的话第二天的色块会从 x<0 开始画出去。
    // 已在 `segments_for_day` 上确立过同一条规矩（正在生长的当前段就裁）。
    let conn = open_in_memory();
    insert_segments(&conn, &[(seg("cross", 86_300_000, 86_422_000, "work"), vec![])]).unwrap();

    let day2 = get_segments_in_range(&conn, 86_400_000, 2 * 86_400_000).unwrap();
    assert_eq!(day2[0].start_at, 86_400_000, "起点应贴到当天零点");
    assert_eq!(day2[0].end_at, 86_422_000, "终点不动");
}

#[test]
fn clipping_never_invents_or_loses_duration_across_the_two_days() {
    // 两天的裁剪结果拼起来必须正好等于原段，否则有一天多算或漏算。
    let conn = open_in_memory();
    let (s, e) = (86_300_000, 90_000_000);
    insert_segments(&conn, &[(seg("cross", s, e, "work"), vec![])]).unwrap();

    let d1 = get_segments_in_range(&conn, 0, 86_400_000).unwrap();
    let d2 = get_segments_in_range(&conn, 86_400_000, 2 * 86_400_000).unwrap();
    let total = d1.iter().chain(d2.iter()).map(|x| x.end_at - x.start_at).sum::<i64>();
    assert_eq!(total, e - s, "两天加起来必须等于原段时长");
}

#[test]
fn a_segment_ending_exactly_at_midnight_belongs_only_to_the_first_day() {
    // 半开区间 `[start, end)`：恰好结束于午夜的段不重复算进第二天。
    let conn = open_in_memory();
    insert_segments(&conn, &[(seg("e", 86_300_000, 86_400_000, "work"), vec![])]).unwrap();
    assert_eq!(get_segments_in_range(&conn, 86_400_000, 2 * 86_400_000).unwrap().len(), 0);
}

#[test]
fn deleting_a_day_also_drops_segments_that_started_before_it() {
    // A2 的另一半。旧口径只删 start_at 落在当天的段，于是「重放 10-06」删不掉
    // 10-05 23:57 起的那个跨日段 —— 重放写进去的新段会和它重叠。
    // 段是整行存整行的，删除必须按相交判、整行删，不能只删掉落在当天的那一半。
    let conn = open_in_memory();
    insert_segments(
        &conn,
        &[
            (seg("cross", 86_300_000, 86_422_000, "work"), vec!["e1".into()]),
            (seg("keep", 200_000_000, 200_001_000, "work"), vec!["e2".into()]),
        ],
    )
    .unwrap();

    delete_segments_for_day(&conn, 86_400_000, 2 * 86_400_000).unwrap();

    let left = get_segments_in_range(&conn, 0, i64::MAX).unwrap();
    assert_eq!(left.len(), 1, "跨日段应被整行删掉");
    assert_eq!(left[0].id, "keep");
    let orphans: i64 = conn
        .query_row("SELECT COUNT(*) FROM activity_evidence", [], |r| r.get(0))
        .unwrap();
    assert_eq!(orphans, 1, "被删段的证据也要清掉");
}
