use activity_core::{Event, EventType, WindowFocusPayload};
use activity_storage::{delete_events_older_than, get_events_in_range, insert_events, open_in_memory};

fn ev(ts: i64, proc: &str) -> Event {
    Event::new(
        EventType::WindowFocus(WindowFocusPayload {
            process_name: proc.into(),
            window_title: None,
            exe_path: None,
        }),
        ts,
    )
}

#[test]
fn insert_and_query_roundtrip() {
    let conn = open_in_memory();
    let a = ev(1000, "a.exe");
    let b = ev(2000, "b.exe");
    insert_events(&conn, &[a.clone(), b.clone()]).unwrap();
    let rows = get_events_in_range(&conn, 0, 3000).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, a.id);
    assert_eq!(rows[1].id, b.id);
    assert_eq!(rows[0].type_, "window_focus");
}

#[test]
fn range_is_half_open_and_sorted() {
    let conn = open_in_memory();
    let events: Vec<Event> = (0..5).map(|i| ev(1000 + i * 1000, "x.exe")).collect();
    insert_events(&conn, &events).unwrap();
    // 半开区间 [1500, 3500) => ts 2000, 3000
    let rows = get_events_in_range(&conn, 1500, 3500).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].timestamp, 2000);
    assert_eq!(rows[1].timestamp, 3000);
    let all = get_events_in_range(&conn, 0, i64::MAX).unwrap();
    assert!(all.windows(2).all(|w| w[0].timestamp <= w[1].timestamp));
}

#[test]
fn empty_range_returns_empty() {
    let conn = open_in_memory();
    insert_events(&conn, &[ev(1000, "a.exe")]).unwrap();
    let rows = get_events_in_range(&conn, 5000, 6000).unwrap();
    assert!(rows.is_empty());
}

#[test]
fn end_boundary_is_exclusive_and_start_boundary_inclusive() {
    let conn = open_in_memory();
    insert_events(&conn, &[ev(1000, "x.exe"), ev(2000, "x.exe")]).unwrap();
    // ts 恰好等于 start 应收进来
    assert_eq!(get_events_in_range(&conn, 1000, 2000).unwrap().len(), 1);
    // ts 恰好等于 end 不应收进来
    assert_eq!(get_events_in_range(&conn, 0, 1000).unwrap().len(), 0);
    assert_eq!(get_events_in_range(&conn, 0, 2001).unwrap().len(), 2);
}

#[test]
fn payload_json_roundtrips_and_keeps_process_name_verbatim() {
    // Review Focus #5: 进程名原样存取，不做大写归一——大小写语义属于 engine 的规则匹配。
    // payload 只存 EventType 变体（id/timestamp/type 各有独立列），不重复存整条 Event。
    let conn = open_in_memory();
    let e = Event::new(
        EventType::WindowFocus(WindowFocusPayload {
            process_name: "CoDe.ExE".into(),
            window_title: Some("main.rs - time-scope".into()),
            exe_path: None,
        }),
        1234,
    );
    insert_events(&conn, std::slice::from_ref(&e)).unwrap();
    let rows = get_events_in_range(&conn, 0, 2000).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, e.id);
    assert_eq!(rows[0].timestamp, 1234);
    let back: EventType = serde_json::from_str(&rows[0].payload).unwrap();
    assert_eq!(back.type_tag(), "window_focus");
    match back {
        EventType::WindowFocus(ref p) => {
            assert_eq!(p.process_name, "CoDe.ExE");
            assert_eq!(p.window_title.as_deref(), Some("main.rs - time-scope"));
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn insert_empty_slice_is_a_noop() {
    let conn = open_in_memory();
    insert_events(&conn, &[]).unwrap();
    assert!(get_events_in_range(&conn, 0, i64::MAX).unwrap().is_empty());
}

#[test]
fn reinserting_same_id_replaces_rather_than_duplicates() {
    let conn = open_in_memory();
    let e = ev(1000, "a.exe");
    insert_events(&conn, &[e.clone(), e.clone()]).unwrap();
    let rows = get_events_in_range(&conn, 0, 2000).unwrap();
    assert_eq!(rows.len(), 1, "id is PRIMARY KEY, re-insert must be idempotent");
}

#[test]
fn distinct_event_types_all_get_distinct_type_column() {
    let conn = open_in_memory();
    use activity_core::{InputHeartbeatPayload, WindowTitleChangePayload};
    let events = vec![
        Event::new(EventType::SystemIdle, 1000),
        Event::new(EventType::SystemResume, 2000),
        Event::new(EventType::SessionLock, 3000),
        Event::new(EventType::SessionUnlock, 4000),
        Event::new(
            EventType::InputHeartbeat(InputHeartbeatPayload { active_seconds: 3 }),
            5000,
        ),
        Event::new(
            EventType::WindowTitleChange(WindowTitleChangePayload {
                process_name: "a.exe".into(),
                window_title: None,
            }),
            6000,
        ),
        ev(7000, "a.exe"),
    ];
    insert_events(&conn, &events).unwrap();
    let rows = get_events_in_range(&conn, 0, 8000).unwrap();
    let tags: Vec<&str> = rows.iter().map(|r| r.type_.as_str()).collect();
    for expected in [
        "system_idle",
        "system_resume",
        "session_lock",
        "session_unlock",
        "input_heartbeat",
        "window_title_change",
        "window_focus",
    ] {
        assert!(tags.contains(&expected), "missing {expected} in {tags:?}");
    }
}

// --- B3：events 保留期 ---

#[test]
fn pruning_removes_only_events_older_than_the_cutoff() {
    // B3。events 表实测 ≈5 MB/天 ≈1.8 GB/年，没有保留期就一直涨。
    // 删的是**原始事件**，`activities` 一行不动 —— 它才是时长账本。
    let conn = open_in_memory();
    insert_events(
        &conn,
        &[
            ev(1_000, "a.exe"),
            ev(2_000, "b.exe"),
            ev(5_000_000, "c.exe"),
            ev(6_000_000, "d.exe"),
        ],
    )
    .unwrap();

    let deleted = delete_events_older_than(&conn, 3_000_000).unwrap();
    assert_eq!(deleted, 2, "只该删掉早于 cutoff 的两条");

    // 活下来的必须全是 cutoff 之后的（Event::new 自造 id，所以按时间戳断言）
    let left: Vec<i64> = get_events_in_range(&conn, 0, i64::MAX)
        .unwrap()
        .into_iter()
        .map(|e| e.timestamp)
        .collect();
    assert_eq!(left.len(), 2);
    assert!(left.iter().all(|t| *t >= 3_000_000), "实际留下 {:?}", left);
}

#[test]
fn the_cutoff_is_exclusive_so_the_boundary_day_survives() {
    // 半开区间 `[cutoff, ∞)` 保留：恰好等于 cutoff 的那条不删。
    // 差 1 毫秒就少一天数据这种事没人受得了。
    let conn = open_in_memory();
    insert_events(&conn, &[ev(3_000_000, "a.exe"), ev(3_000_001, "b.exe")]).unwrap();
    assert_eq!(delete_events_older_than(&conn, 3_000_000).unwrap(), 0);
    assert_eq!(get_events_in_range(&conn, 0, i64::MAX).unwrap().len(), 2);
}

#[test]
fn pruning_leaves_activities_completely_alone() {
    // 关键性质：删 events 不许波及 activities。时间线、汇总、热力图
    // 全都只读 activities —— 一旦连坐，那些数字就凭空少了半年。
    let conn = open_in_memory();
    insert_events(&conn, &[ev(1_000, "a.exe")]).unwrap();
    let seg = activity_storage::StoredSegment {
        id: "seg-1".into(),
        start_at: 1_000,
        end_at: 60_000,
        category: "work".into(),
        application: Some("Code.exe".into()),
        confidence: 1.0,
        classifier: "rule".into(),
        classifier_version: "v".into(),
        evidence_event_ids: vec![],
    };
    activity_storage::insert_segments(&conn, &[(seg, vec![])]).unwrap();

    delete_events_older_than(&conn, 1_000_000).unwrap();

    let segs =
        activity_storage::get_segments_in_range(&conn, 0, i64::MAX).unwrap();
    assert_eq!(segs.len(), 1, "activities 一行都不该少");
    assert_eq!(segs[0].id, "seg-1");
}

#[test]
fn pruning_an_empty_or_all_fresh_database_is_a_harmless_noop() {
    let conn = open_in_memory();
    assert_eq!(delete_events_older_than(&conn, 1_000_000).unwrap(), 0);
    insert_events(&conn, &[ev(9_000_000, "a.exe")]).unwrap();
    assert_eq!(delete_events_older_than(&conn, 1_000_000).unwrap(), 0);
    assert_eq!(get_events_in_range(&conn, 0, i64::MAX).unwrap().len(), 1);
}
