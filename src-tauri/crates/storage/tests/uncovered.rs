use activity_storage::{
    insert_segments, open_in_memory, uncovered_programs, StoredSegment,
};

/// 造一个「最近 N 天里落进 unknown 的程序，按耗时排序」的测试库。
///
/// 覆盖三件事：
/// 1. 只取 `unknown` 的段 —— 已分类的段不该出现在「没规则覆盖」里
/// 2. 同一个程序的多段要合并时长，而不是出现两行
/// 3. 按耗时降序，让「最该先补规则的那个」排在最上面
fn seg(id: &str, start_ms: i64, end_ms: i64, category: &str, app: Option<&str>) -> StoredSegment {
    StoredSegment {
        id: id.into(),
        start_at: start_ms,
        end_at: end_ms,
        category: category.into(),
        application: app.map(|a| a.into()),
        confidence: 1.0,
        classifier: "rule".into(),
        classifier_version: "1".into(),
        active_ms: None,
        evidence_event_ids: Vec::new(),
    }
}

fn put(conn: &rusqlite::Connection, segs: Vec<StoredSegment>) {
    let refs: Vec<(StoredSegment, Vec<String>)> =
        segs.into_iter().map(|s| (s, Vec::new())).collect();
    insert_segments(conn, &refs).unwrap();
}

const H: i64 = 3_600_000;

#[test]
fn orders_by_duration_desc_and_only_unknown() {
    let conn = open_in_memory();
    let now: i64 = 20 * H; // 相对零点 20 小时，够放下下面所有段
    put(
        &conn,
        vec![
            // Zed 用得少（1h）
            seg("a", H, 2 * H, "unknown", Some("Zed.exe")),
            // DBeaver 用得多（3h），但被拆成两段 —— 必须合并成一行
            seg("b", 2 * H, 4 * H, "unknown", Some("DBeaver.exe")),
            seg("c", 5 * H, 6 * H, "unknown", Some("DBeaver.exe")),
            // VS Code 也在 unknown 里（没有被任何规则命中）
            seg("d", 6 * H, 8 * H, "unknown", Some("Code.exe")),
            // 已被规则命中的段不该出现
            seg("e", 8 * H, 20 * H, "work", Some("Code.exe")),
        ],
    );

    let out = uncovered_programs(&conn, now, 30).unwrap();

    let names: Vec<&str> = out.iter().map(|u| u.application.as_str()).collect();
    assert_eq!(names, vec!["DBeaver.exe", "Code.exe", "Zed.exe"]);

    assert_eq!(out[0].total_ms, 3 * H); // DBeaver 两段合并
    assert_eq!(out[1].total_ms, 2 * H);
    assert_eq!(out[2].total_ms, H);
}

#[test]
fn share_is_against_uncovered_total_not_the_whole_day() {
    let conn = open_in_memory();
    let now: i64 = 20 * H;
    put(
        &conn,
        vec![
            seg("a", H, 3 * H, "unknown", Some("Zed.exe")), // 2h 未覆盖
            seg("b", 3 * H, 15 * H, "work", Some("Code.exe")), // 12h 已覆盖
        ],
    );

    let out = uncovered_programs(&conn, now, 30).unwrap();

    // 分母是 unknown 的 2h，不是全天的 14h —— 用户要回答的是
    //「我漏配的时间里有多少给了它」，不是「我一天有多少给了它」
    assert_eq!(out[0].total_ms, 2 * H);
    assert_eq!(out[0].share_of_uncovered, 1.0);
}

#[test]
fn segments_outside_the_window_are_excluded() {
    let conn = open_in_memory();
    let now: i64 = 20 * H;
    // days=1 是一天的窗口：[now-24h, now] = [-4h, 20h]
    put(
        &conn,
        vec![
            seg("recent", now - H, now, "unknown", Some("New.exe")),
            // 在窗口左边界之外（-4h 之前）
            seg("old", -30 * H, -29 * H, "unknown", Some("Ancient.exe")),
        ],
    );

    let out = uncovered_programs(&conn, now, 1).unwrap();
    let names: Vec<&str> = out.iter().map(|u| u.application.as_str()).collect();
    assert_eq!(names, vec!["New.exe"]);
}

#[test]
fn no_uncovered_segments_yields_empty_not_err() {
    let conn = open_in_memory();
    let now: i64 = 20 * H;
    put(&conn, vec![seg("a", H, 2 * H, "work", Some("Code.exe"))]);

    // 「全都覆盖了」是一个好状态，调用方据此显示空状态 ——
    // 返回 Err 会让界面把它当成出错来提示
    assert!(uncovered_programs(&conn, now, 30).unwrap().is_empty());
}

#[test]
fn segments_without_an_application_are_skipped() {
    let conn = open_in_memory();
    let now: i64 = 20 * H;
    put(
        &conn,
        vec![
            seg("a", H, 2 * H, "unknown", None),
            seg("b", 2 * H, 3 * H, "unknown", Some("Code.exe")),
        ],
    );

    let out = uncovered_programs(&conn, now, 30).unwrap();
    // 没有应用名的段没法告诉用户「去加规则」，留着只会显示一个空行
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].application, "Code.exe");
}