use crate::{reduce, EngineConfig, EngineState, RuleSet, DEFAULT_RULES_TOML};
use activity_core::{
    Event, EventType, InputHeartbeatPayload, WindowFocusPayload, WindowTitleChangePayload,
};

fn rules() -> RuleSet {
    RuleSet::from_toml(DEFAULT_RULES_TOML).unwrap()
}

fn focus(ts: i64, app: &str) -> Event {
    Event::new(
        EventType::WindowFocus(WindowFocusPayload {
            process_name: app.into(),
            window_title: None,
            exe_path: None,
        }),
        ts,
    )
}

/// 一个心跳。`window_ms` 是这个窗口有多长（默认 10 秒，与
/// `heartbeat_every_s` 一致）—— 窗口起点由它与 `ts` 推出来。
fn heartbeat(ts: i64, secs: u8) -> Event {
    heartbeat_at(ts, secs, 10_000)
}

fn heartbeat_at(ts: i64, secs: u8, window_ms: i64) -> Event {
    Event::new(
        EventType::InputHeartbeat(InputHeartbeatPayload {
            active_seconds: secs,
            window_start_ms: ts - window_ms,
        }),
        ts,
    )
}

/// 取全部段：已关闭的 + 正在生长的当前段。
fn all_segments(events: &[Event], rs: &RuleSet, cfg: &EngineConfig) -> Vec<crate::ActivitySegment> {
    let mut st = EngineState::initial();
    let mut all = Vec::new();
    for e in events {
        let out = reduce(&st, e, rs, cfg);
        all.extend(out.closed_segments);
        st = out.state;
    }
    if let Some(open) = st.open_segment_snapshot(rs, cfg) {
        all.push(open);
    }
    all
}

fn cats(events: &[Event]) -> Vec<String> {
    all_segments(events, &rules(), &EngineConfig::default().with_min_segment_duration_s(0))
        .iter()
        .map(|s| s.category.as_str().to_string())
        .collect()
}

/// 关掉归一化：段一关闭就立即可落库。
/// 只想验证「分段的形状」而不涉及归一化语义时用它。
fn emit_now() -> EngineConfig {
    EngineConfig::default()
        .with_min_segment_duration_s(0)

}

// --- 基本：同应用延长 ---

#[test]
fn single_event_creates_one_segment() {
    let segs = all_segments(&[focus(1_000, "Code.exe")], &rules(), &emit_now());
    assert_eq!(segs.len(), 1, "只有 1 条 Event 也要出 1 段");
    assert_eq!(segs[0].category.as_str(), "work");
    assert_eq!(segs[0].start_at, 1_000);
    assert_eq!(segs[0].application.as_deref(), Some("Code.exe"));
}

#[test]
fn same_app_extends_one_segment() {
    let segs = all_segments(
        &[
            focus(1_000, "Code.exe"),
            focus(10_000, "Code.exe"),
            focus(20_000, "Code.exe"),
        ],
        &rules(),
        &emit_now(),
    );
    assert_eq!(segs.len(), 1, "同一应用不应切段");
    assert_eq!(segs[0].start_at, 1_000);
    assert_eq!(segs[0].end_at, 20_000);
    assert_eq!(segs[0].evidence_event_ids.len(), 3, "每个 Event 都应成为证据");
}

#[test]
fn segment_end_is_always_at_or_after_start() {
    // 跨零点 / 乱序时间戳也不能产生负长度段
    let segs = all_segments(
        &[focus(5_000, "Code.exe"), focus(1_000, "Code.exe")],
        &rules(),
        &emit_now(),
    );
    for s in &segs {
        assert!(s.end_at >= s.start_at, "负长度段: {s:?}");
        assert!(s.duration_ms() >= 0);
    }
}

// --- 切段 ---

#[test]
fn different_app_splits_segment() {
    let segs = all_segments(
        &[focus(1_000, "Code.exe"), focus(600_000, "chrome.exe")],
        &rules(),
        &emit_now(),
    );
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].application.as_deref(), Some("Code.exe"));
    assert_eq!(segs[1].application.as_deref(), Some("chrome.exe"));
    assert_eq!(segs[0].end_at, 600_000, "旧段应在切换点收尾");
    assert_eq!(segs[1].start_at, 600_000);
}

#[test]
fn unknown_app_gets_unknown_category() {
    let segs = all_segments(&[focus(1_000, "MysteryApp.exe")], &rules(), &emit_now());
    assert_eq!(segs[0].category.as_str(), "unknown");
    assert_eq!(segs[0].confidence, 0.0);
}

#[test]
fn segments_are_ordered_and_non_overlapping() {
    let events = vec![
        focus(0, "Code.exe"),
        focus(600_000, "chrome.exe"),
        focus(1_200_000, "Code.exe"),
        focus(1_800_000, "QQ.exe"),
    ];
    let segs = all_segments(&events, &rules(), &emit_now());
    for w in segs.windows(2) {
        assert!(w[0].end_at <= w[1].start_at, "段不应重叠: {:?} {:?}", w[0], w[1]);
    }
}

// --- 归一化：短段并进邻居（取代原 grace 抖动吸收，spec §7.3）---

#[test]
fn a_quick_switch_back_stays_one_segment() {
    // Code → chrome(2s) → Code：那 2 秒不足以成为一段活动，
    // 并进它打断的 Code 段，切回来再把 tail 重新长回当前段 —— 一段。
    let cfg = EngineConfig::default().with_min_segment_duration_s(5);
    let segs = all_segments(
        &[
            focus(0, "Code.exe"),
            focus(2_000, "chrome.exe"),
            focus(4_000, "Code.exe"),
        ],
        &rules(),
        &cfg,
    );
    assert_eq!(
        segs.len(),
        1,
        "2 秒的往返不该留下多段，实际 {:?}",
        segs.iter().map(|s| s.category.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(segs[0].application.as_deref(), Some("Code.exe"));
    assert_eq!(segs[0].start_at, 0);
    assert_eq!(segs[0].end_at, 4_000);
}

#[test]
fn a_longer_switch_back_stays_separate_segments() {
    // 190 秒的 chrome 是真实活动，不是抖动 —— 它不该被并掉。
    let cfg = EngineConfig::default().with_min_segment_duration_s(5);
    let segs = all_segments(
        &[
            focus(0, "Code.exe"),
            focus(10_000, "chrome.exe"),
            focus(200_000, "Code.exe"),
        ],
        &rules(),
        &cfg,
    );
    assert!(segs.len() >= 2, "超过 N 的往返应保留为多段，实际 {}", segs.len());
    assert!(
        cats(&[
            focus(0, "Code.exe"),
            focus(10_000, "chrome.exe"),
            focus(200_000, "Code.exe"),
        ])
        .contains(&"browsing".to_string()),
        "chrome 那一段应当真的存在"
    );
}

#[test]
fn min_zero_disables_normalization() {
    // 0 = 关掉归一化：每次切换都切段，段一关闭就立即可落库。
    let cfg = EngineConfig::default().with_min_segment_duration_s(0);
    let segs = all_segments(
        &[focus(0, "Code.exe"), focus(1_000, "chrome.exe"), focus(2_000, "Code.exe")],
        &rules(),
        &cfg,
    );
    assert_eq!(segs.len(), 3, "min=0 时每次切换都切段");
}

#[test]
fn the_merged_short_segment_keeps_its_evidence() {
    // 被并掉的那个段，其证据应并进留下的段，而不是凭空消失
    let cfg = EngineConfig::default().with_min_segment_duration_s(5);
    let segs = all_segments(
        &[
            focus(0, "Code.exe"),
            focus(2_000, "chrome.exe"),
            focus(4_000, "Code.exe"),
        ],
        &rules(),
        &cfg,
    );
    assert_eq!(segs[0].evidence_event_ids.len(), 3, "三个 Event 都应留下证据");
}

// --- idle ---

#[test]
fn idle_starts_an_idle_segment() {
    let segs = all_segments(
        &[
            focus(0, "Code.exe"),
            Event::new(EventType::SystemIdle, 400_000),
            Event::new(EventType::SystemResume, 700_000),
        ],
        &rules(),
        &emit_now(),
    );
    let c = segs.iter().map(|s| s.category.as_str()).collect::<Vec<_>>();
    assert!(c.contains(&"idle"), "应有 idle 段，实际={:?}", c);
    let idle = segs.iter().find(|s| s.category.as_str() == "idle").unwrap();
    assert!(idle.start_at >= 400_000);
    assert!(idle.end_at >= idle.start_at);
}

#[test]
fn idle_closes_the_previous_work_segment() {
    let segs = all_segments(
        &[focus(0, "Code.exe"), Event::new(EventType::SystemIdle, 400_000)],
        &rules(),
        &emit_now(),
    );
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].category.as_str(), "work");
    assert_eq!(segs[0].end_at, 400_000, "工作段应在进入 idle 时收尾");
    assert_eq!(segs[1].category.as_str(), "idle");
}

#[test]
fn heartbeats_during_idle_do_not_extend_the_work_segment() {
    // spec §7.3：idle 期间的心跳不计入活跃
    let segs = all_segments(
        &[
            focus(0, "Code.exe"),
            heartbeat(1_000, 0),
            Event::new(EventType::SystemIdle, 400_000),
            heartbeat(410_000, 0),
            heartbeat(420_000, 0),
        ],
        &rules(),
        &emit_now(),
    );
    let work = segs.iter().find(|s| s.category.as_str() == "work").unwrap();
    assert_eq!(work.end_at, 400_000, "工作段不应被 idle 期的心跳撑长");
}

// --- 纯函数性（spec §7.4 可重放）---

#[test]
fn reduce_is_pure_same_input_same_output() {
    let events = vec![focus(0, "Code.exe"), focus(600_000, "chrome.exe")];
    let (rs, cfg) = (rules(), emit_now());
    let run = || {
        let mut st = EngineState::initial();
        for e in &events {
            st = reduce(&st, e, &rs, &cfg).state;
        }
        st
    };
    assert_eq!(run(), run(), "同一串事件必须得到同一状态");
}

#[test]
fn reduce_does_not_mutate_input_state() {
    let rs = rules();
    let cfg = EngineConfig::default();
    let st = EngineState::initial();
    let _ = reduce(&st, &focus(0, "Code.exe"), &rs, &cfg);
    assert!(st.current_segment.is_none());
    assert!(st.current_context.is_none());
}

#[test]
fn replay_after_rules_change_produces_different_categories() {
    let events = vec![focus(0, "Code.exe")];
    let before = all_segments(&events, &rules(), &emit_now());
    let other = RuleSet::from_toml(
        r#"
[[rule]]
id = "everything"
process = ["Code.exe"]
category = "entertainment"
confidence = 0.1
"#,
    )
    .unwrap();
    let after = all_segments(&events, &other, &emit_now());
    assert_eq!(before[0].category.as_str(), "work");
    assert_eq!(after[0].category.as_str(), "entertainment");
    assert_ne!(before[0].classifier_version, after[0].classifier_version);
}

// --- open segment ---

#[test]
fn open_segment_is_not_in_closed_segments() {
    let (rs, cfg) = (rules(), emit_now());
    let out = reduce(&EngineState::initial(), &focus(0, "Code.exe"), &rs, &cfg);
    assert!(out.closed_segments.is_empty(), "未关闭的段不该出现在 closed 里");
    assert!(out.state.current_segment.is_some());
}

#[test]
fn open_segment_snapshot_mirrors_the_growing_segment() {
    let (rs, cfg) = (rules(), EngineConfig::default());
    let mut st = reduce(&EngineState::initial(), &focus(0, "Code.exe"), &rs, &cfg).state;
    st = reduce(&st, &focus(5_000, "Code.exe"), &rs, &cfg).state;
    let snap = st.open_segment_snapshot(&rs, &cfg).expect("应有正在生长的段");
    assert_eq!(snap.start_at, 0);
    assert_eq!(snap.end_at, 5_000);
    assert_eq!(snap.category.as_str(), "work");
    assert_eq!(snap.classifier, crate::CLASSIFIER_RULE);
}

#[test]
fn open_segment_snapshot_is_none_when_idle_and_empty() {
    let (rs, cfg) = (rules(), EngineConfig::default());
    assert!(EngineState::initial().open_segment_snapshot(&rs, &cfg).is_none());
}

// --- 归一化缓冲（原 min_segment_duration 缓冲）---

#[test]
fn the_previous_segment_is_held_until_the_next_one_arrives() {
    // 归一化只需要一段前瞻：前一段压着，等下一个段决定「并」还是「定稿」。
    // 这里用 60 秒的 Code（够长，不会被并进后继），所以它确实「压着等」。
    let (rs, cfg) = (rules(), EngineConfig::default().with_min_segment_duration_s(30));
    let mut st = reduce(&EngineState::initial(), &focus(0, "Code.exe"), &rs, &cfg).state;
    st = reduce(&st, &focus(60_000, "chrome.exe"), &rs, &cfg).state;
    assert!(st.current_segment.is_some(), "应该有正在生长的 chrome 段");
    let snapshot = st.open_segment_snapshot(&rs, &cfg).unwrap();
    assert_eq!(snapshot.application.as_deref(), Some("chrome.exe"));
    let held = st.tail.as_ref().expect("前一段应压在缓冲里等下一个段");
    assert_eq!(held.application.as_deref(), Some("Code.exe"));
}

#[test]
fn the_held_segment_is_released_once_it_is_stale() {
    // N = 30s：够长的段切走之后先压着（等下一个段来决定要不要并），
    // 满 30 秒没有新段就定稿落库 —— 否则数据会永远留在内存里。
    let (rs, cfg) = (rules(), EngineConfig::default().with_min_segment_duration_s(30));
    let mut st = reduce(&EngineState::initial(), &focus(0, "Code.exe"), &rs, &cfg).state;
    let after_switch = reduce(&st, &focus(60_000, "chrome.exe"), &rs, &cfg);
    assert!(
        after_switch.closed_segments.is_empty(),
        "刚切段，还在等下一段来决定并进谁"
    );
    st = after_switch.state;

    let out = reduce(&st, &heartbeat(100_000, 3), &rs, &cfg);
    assert!(
        !out.closed_segments.is_empty(),
        "超过 N 秒没有后继就该定稿"
    );
}

// --- A. 无应用的事件不该开段 ---

#[test]
fn heartbeat_before_any_window_event_does_not_open_a_segment() {
    // 应用刚启动时第一个事件通常是心跳，而心跳不携带 application。
    // 这类事件无法归因，不该凭空开一个 "unknown / 无应用" 的段。
    let cfg = EngineConfig::default()
        .with_min_segment_duration_s(0)
;
    let (rs, cfg) = (rules(), cfg);
    let mut st = EngineState::initial();
    for ts in [1_000, 11_000, 21_000] {
        st = reduce(&st, &heartbeat(ts, 1), &rs, &cfg).state;
    }
    assert!(
        st.current_segment.is_none(),
        "没有已知应用时不该开段，实际 {:?}",
        st.current_segment
    );
    assert!(st.open_segment_snapshot(&rs, &cfg).is_none());
}

#[test]
fn heartbeat_with_no_known_app_still_extends_an_existing_segment() {
    // 已经有段之后，心跳（application 不变）应正常延长它
    let cfg = EngineConfig::default()
        .with_min_segment_duration_s(0)
;
    let (rs, cfg) = (rules(), cfg);
    let mut st = reduce(&EngineState::initial(), &focus(0, "Code.exe"), &rs, &cfg).state;
    st = reduce(&st, &heartbeat(5_000, 1), &rs, &cfg).state;
    let snap = st.open_segment_snapshot(&rs, &cfg).unwrap();
    assert_eq!(snap.application.as_deref(), Some("Code.exe"));
    assert_eq!(snap.end_at, 5_000);
}

// --- B. min_segment_duration 真的要把短段并掉 ---

/// N = 2 秒：小时间线就能看清「短段并进邻居」的行为。
fn merge_cfg() -> EngineConfig {
    EngineConfig::default()
        .with_min_segment_duration_s(2)

}

/// 喂完所有事件，返回 (已释放的段, 最终状态)。
fn run_cfg(events: &[Event], cfg: &EngineConfig) -> (Vec<crate::ActivitySegment>, EngineState) {
    let rs = rules();
    let mut st = EngineState::initial();
    let mut closed = Vec::new();
    for e in events {
        let out = reduce(&st, e, &rs, cfg);
        closed.extend(out.closed_segments);
        st = out.state;
    }
    (closed, st)
}

fn apps_of(segs: &[crate::ActivitySegment]) -> Vec<Option<String>> {
    segs.iter().map(|s| s.application.clone()).collect()
}

#[test]
fn short_segment_merges_into_the_following_long_segment() {
    // Code 只出现 1 秒（1s < 2s），后面是 4 秒的 explorer。
    // Code 没有前驱长段，应并入后继 explorer，起点回延到 0。
    let cfg = merge_cfg();
    let (closed, _) = run_cfg(
        &[
            focus(0, "Code.exe"),
            focus(1_000, "explorer.exe"),
            focus(5_000, "QQ.exe"),
            focus(9_000, "Code.exe"),
        ],
        &cfg,
    );
    let merged = closed
        .iter()
        .find(|s| s.application.as_deref() == Some("explorer.exe"))
        .unwrap_or_else(|| panic!("explorer 段应被释放，实际 {:?}", apps_of(&closed)));
    assert_eq!(merged.start_at, 0, "1 秒的 Code 前驱应并入 explorer");
    assert_eq!(merged.end_at, 5_000);
    assert!(
        !apps_of(&closed).contains(&Some("Code.exe".into())),
        "过短的 Code 段不该单独落库"
    );
}

#[test]
fn short_segment_merges_into_the_preceding_long_segment() {
    // 20 秒的 msedge 之后一个 64ms 的 explorer 抖动，没有后继长段，应并入前驱
    let cfg = merge_cfg();
    let (closed, _) = run_cfg(
        &[
            focus(0, "msedge.exe"),
            focus(20_000, "explorer.exe"),
            focus(20_064, "QQ.exe"),
            focus(40_000, "Code.exe"),
        ],
        &cfg,
    );
    let msedge = closed
        .iter()
        .find(|s| s.application.as_deref() == Some("msedge.exe"))
        .unwrap_or_else(|| panic!("msedge 段应保留，实际 {:?}", apps_of(&closed)));
    assert_eq!(msedge.end_at, 20_064, "64ms 的 explorer 应并入前驱 msedge");
    assert!(
        !apps_of(&closed).contains(&Some("explorer.exe".into())),
        "64ms 的段不该单独落库"
    );
}

#[test]
fn long_segments_are_left_alone() {
    let cfg = merge_cfg();
    let (closed, _) = run_cfg(
        &[
            focus(0, "Code.exe"),
            focus(20_000, "msedge.exe"),
            focus(40_000, "QQ.exe"),
            focus(60_000, "Code.exe"),
            focus(80_000, "msedge.exe"),
            focus(100_000, "QQ.exe"),
        ],
        &cfg,
    );
    let apps = apps_of(&closed);
    assert!(apps.contains(&Some("Code.exe".into())), "{:?}", apps);
    assert!(apps.contains(&Some("msedge.exe".into())), "{:?}", apps);
    assert!(apps.contains(&Some("QQ.exe".into())), "{:?}", apps);
    for s in &closed {
        assert!(s.end_at - s.start_at >= 2_000, "长段不该被并短: {:?}", s);
    }
}

#[test]
fn merging_concatenates_evidence_of_both_segments() {
    let cfg = merge_cfg();
    let (closed, _) = run_cfg(
        &[
            focus(0, "Code.exe"),
            focus(1_000, "msedge.exe"),
            focus(5_000, "QQ.exe"),
            focus(9_000, "Code.exe"),
        ],
        &cfg,
    );
    let merged = closed
        .iter()
        .find(|s| s.application.as_deref() == Some("msedge.exe"))
        .expect("msedge 段");
    assert_eq!(
        merged.evidence_event_ids.len(),
        2,
        "被并掉的 Code 段的证据不该丢失，实际 {:?}",
        merged.evidence_event_ids
    );
}

#[test]
fn a_short_segment_surrounded_by_two_long_ones_chooses_the_preceding() {
    // msedge(长) -> explorer(0.5s 短) -> QQ(长)：explorer 并入前驱 msedge
    let cfg = merge_cfg();
    let (closed, _) = run_cfg(
        &[
            focus(0, "msedge.exe"),
            focus(20_000, "explorer.exe"),
            focus(20_500, "QQ.exe"),
            focus(40_000, "Code.exe"),
        ],
        &cfg,
    );
    assert!(
        !apps_of(&closed).contains(&Some("explorer.exe".into())),
        "explorer 不该单独落库：{:?}",
        apps_of(&closed)
    );
    let msedge = closed
        .iter()
        .find(|s| s.application.as_deref() == Some("msedge.exe"))
        .unwrap();
    assert_eq!(msedge.end_at, 20_500, "explorer 并入前驱 msedge");
}

#[test]
fn a_short_segment_waits_for_its_neighbour_rather_than_becoming_a_sliver() {
    // Code 只闪了 1 秒就切到 explorer：它不是一段活动，等它的邻居出现再决定。
    // 这里 explorer 紧接在后面，于是那 1 秒并进 explorer —— 没有任何段落库，
    // 也没有一根 2px 的刺留在时间线上。
    let cfg = merge_cfg();
    let (closed, st) = run_cfg(
        &[
            focus(0, "Code.exe"),
            focus(1_000, "explorer.exe"),
            focus(5_000, "QQ.exe"),
        ],
        &cfg,
    );
    assert!(
        apps_of(&closed).is_empty(),
        "还没有段定稿：短段与它的邻居都压在缓冲里，实际 {:?}",
        apps_of(&closed)
    );
    let held = st.tail.as_ref().expect("缓冲里应有一段");
    assert_eq!(
        (held.start_at, held.end_at),
        (0, 5_000),
        "1 秒的 Code 并进了后面的 explorer"
    );
}


// --- 锁屏/睡眠（spec §3.1）---

#[test]
fn session_lock_starts_an_idle_segment() {
    let segs = all_segments(
        &[focus(0, "Code.exe"), Event::new(EventType::SessionLock, 400_000)],
        &rules(),
        &emit_now(),
    );
    let cats: Vec<&str> = segs.iter().map(|s| s.category.as_str()).collect();
    assert!(
        cats.contains(&"idle"),
        "锁屏应产出 idle 段，实际={:?}",
        cats
    );
    let idle = segs.iter().find(|s| s.category.as_str() == "idle").unwrap();
    assert!(idle.start_at >= 400_000);
}

#[test]
fn session_unlock_closes_the_idle_segment() {
    let segs = all_segments(
        &[
            focus(0, "Code.exe"),
            Event::new(EventType::SessionLock, 400_000),
            Event::new(EventType::SessionUnlock, 1_000_000),
        ],
        &rules(),
        &emit_now(),
    );
    let idle = segs.iter().find(|s| s.category.as_str() == "idle").unwrap();
    assert!(idle.end_at >= 1_000_000, "解锁应把 idle 段收在解锁时刻");
}

#[test]
fn the_work_segment_ends_when_the_screen_locks() {
    // 锁屏前的工作不该延伸到锁屏之后
    let segs = all_segments(
        &[focus(0, "Code.exe"), Event::new(EventType::SessionLock, 400_000)],
        &rules(),
        &emit_now(),
    );
    let work = segs.iter().find(|s| s.category.as_str() == "work").unwrap();
    assert_eq!(work.end_at, 400_000);
}

#[test]
fn heartbeats_during_a_lock_do_not_produce_active_time() {
    // Review Focus #1：锁屏 8 小时不该产生 active
    let segs = all_segments(
        &[
            focus(0, "Code.exe"),
            Event::new(EventType::SessionLock, 400_000),
            heartbeat(500_000, 0),
            heartbeat(600_000, 9), // 即便有输入心跳，锁屏期间也只该算 idle
        ],
        &rules(),
        &emit_now(),
    );
    // 只看锁屏**之后**开始的段：锁屏前那段 work 是真实存在的
    for s in segs.iter().filter(|s| s.start_at >= 400_000) {
        assert_ne!(
            s.category.as_str(),
            "work",
            "锁屏期间不该有 work 段：{s:?}"
        );
    }
    let idle = segs.iter().find(|s| s.category.as_str() == "idle").unwrap();
    assert!(idle.end_at >= 600_000, "心跳应延长 idle 段");
}

#[test]
fn a_session_lock_is_not_a_repeat_of_the_previous_idle() {
    // 重复收到锁屏事件不应反复开新段（锁屏 + 合盖可能各发一次）
    let segs = all_segments(
        &[
            focus(0, "Code.exe"),
            Event::new(EventType::SessionLock, 400_000),
            Event::new(EventType::SessionLock, 410_000),
        ],
        &rules(),
        &emit_now(),
    );
    assert_eq!(
        segs.iter().filter(|s| s.category.as_str() == "idle").count(),
        1,
        "重复的锁屏事件不该产生第二个 idle 段：{:?}",
        segs.iter().map(|s| s.category.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn a_missing_unlock_does_not_invent_extra_time() {
    // Review Focus #1：解锁事件可能收不到（进程被杀）。那段时间只应是 idle，
    // 绝不能凭空多出 active。
    let segs = all_segments(
        &[focus(0, "Code.exe"), Event::new(EventType::SessionLock, 400_000)],
        &rules(),
        &emit_now(),
    );
    let work = segs.iter().find(|s| s.category.as_str() == "work").unwrap();
    assert_eq!(work.end_at, 400_000, "锁屏后的时间不能算进工作段");
}

// --- 重叠（A1）---

#[test]
fn a_held_back_short_segment_does_not_produce_overlapping_segments() {
    // A1 的现代版。旧实现里 `pending` 是一个**要按时间排序的列表**：
    // 一个够老但等不到邻居的短段被 append 到末尾，顺序一破，
    // `absorb` 用 `pending.last()` 取到十几秒前的老段、把它拉到当前时刻，
    // 于是产出重叠。
    //
    // 现在缓冲里最多只有一段（`EngineState.tail`），没有「排序」可被打乱 ——
    // 这条测试守的不再是那个 bug，而是**同一串真实时序下段仍然不重叠**。
    let events = vec![
        focus(0, "Code.exe"),       // 开 A
        focus(5_000, "chrome.exe"), // 关 A（5s，短段）；开 B
        focus(70_000, "Code.exe"),  // 关 B（65s，长）；开 C。A 无长邻居 → 进 keep
        focus(80_000, "chrome.exe"),// 关 C（10s）；开 D
        focus(85_000, "Code.exe"),  // 判定为抖动 → absorb
        // 拖时间把 pending 里的段冲刷出来，否则看不到它们落库
        heartbeat(200_000, 5),
        heartbeat(300_000, 5),
    ];
    let segs = all_segments(&events, &rules(), &EngineConfig::default());

    for w in segs.windows(2) {
        assert!(
            w[0].end_at <= w[1].start_at,
            "段不应重叠: [{}-{} {}] 与 [{}-{} {}]",
            w[0].start_at,
            w[0].end_at,
            w[0].application.as_deref().unwrap_or("-"),
            w[1].start_at,
            w[1].end_at,
            w[1].application.as_deref().unwrap_or("-"),
        );
    }
}

#[test]
fn overlapping_segments_never_invent_time() {
    // A1 的直接后果：`SUM(end_at - start_at)` 重复计数。
    // 审计里那句「这段 85 s 的序列会算出 160 s」说的就是这个 ——
    // 旧段被拉到当前时刻，而中间真实的段照样落库，同一段时间被数了两遍。
    // 归一化换了一整套实现之后这条不变式照旧要成立（而且更硬：任何合并都只
    // 发生在首尾相接的两段之间）。
    //
    // 与上一条互补：那条查"有没有重叠"，这条查"总量有没有超出墙钟跨度"。
    let events = vec![
        focus(0, "Code.exe"),
        focus(5_000, "chrome.exe"),
        focus(70_000, "Code.exe"),
        focus(80_000, "chrome.exe"),
        focus(85_000, "Code.exe"),
        heartbeat(200_000, 5),
        heartbeat(300_000, 5),
    ];
    let segs = all_segments(&events, &rules(), &EngineConfig::default());

    let span = events.last().unwrap().timestamp - events.first().unwrap().timestamp;
    let total: i64 = segs.iter().map(|s| s.end_at - s.start_at).sum();
    assert!(
        total <= span,
        "段总时长 {}ms 超过墙钟跨度 {}ms —— 重叠被重复计数了：{:?}",
        total,
        span,
        segs.iter()
            .map(|s| (s.start_at, s.end_at, s.application.as_deref().unwrap_or("-")))
            .collect::<Vec<_>>()
    );
}
// --- A1：重复计数 ---

// --- 时钟异常（B10 的现代版）---

#[test]
fn a_backwards_clock_never_merges_into_an_earlier_segment() {
    // 系统时钟被校正 / NTP 跳变时，新段的时间戳可能落在前一段的跨度里。
    // 那时**不许并**：并了就等于用这一小段替换掉一段真实时间，
    // 被替换的部分从账本上整个消失（B10 的教训）。前一段照常定稿。
    let mut tail = Some(mk("a", 0, 50_000, "work"));
    let mut out = Vec::new();
    crate::normalize::settle(&mut tail, &mut out, mk("b", 40_000, 40_000, "browsing"), 5_000);

    assert_eq!(out.len(), 1, "前一段应当定稿");
    assert_eq!(
        (out[0].start_at, out[0].end_at),
        (0, 50_000),
        "A 的跨度不该被回退的时间戳改动"
    );
    assert_eq!(tail.as_ref().map(|s| s.id.clone()), Some("b".into()));
}

#[test]
fn a_segment_is_released_exactly_when_it_has_waited_n_seconds() {
    // 边界钉的是 `>=`：等满 N 秒就算「等不到后继」，该定稿了。
    // 差一毫秒都不放 —— 否则数据在内存里多待一轮。
    let mut tail = Some(mk("a", 0, 10_000, "work"));
    let mut out = Vec::new();
    crate::normalize::release_if_stale(&mut tail, &mut out, 10_000 + 4_999, 5_000);
    assert!(out.is_empty(), "还差 1ms 就不该定稿");
    assert!(tail.is_some());

    crate::normalize::release_if_stale(&mut tail, &mut out, 10_000 + 5_000, 5_000);
    assert_eq!(out.len(), 1, "等满 N 秒应当定稿");
    assert!(tail.is_none());
}

// --- B12：合并短段必须真的紧邻 ---

fn mk(id: &str, start: i64, end: i64, cat: &str) -> crate::ActivitySegment {
    crate::ActivitySegment {
        id: id.into(),
        start_at: start,
        end_at: end,
        category: crate::Category::from_str(cat).expect("合法类别"),
        application: Some("Code.exe".into()),
        confidence: 1.0,
        classifier: "rule".into(),
        classifier_version: "v".into(),
        evidence_event_ids: vec![format!("ev-{id}")],
        active_ms: 0,
    }
}

#[test]
fn a_short_segment_is_never_merged_across_a_gap() {
    // B12。合并只认「首尾相接」：A 在 0–60s，S 在 300–310s，中间有 240 秒空档。
    // 若按 min/max 硬并，那段空档就被记成了 work。
    let mut tail = Some(mk("a", 0, 60_000, "work"));
    let mut out = Vec::new();
    crate::normalize::settle(&mut tail, &mut out, mk("s", 300_000, 310_000, "work"), 30_000);

    assert_eq!(out.len(), 1, "不相接 → 前一段定稿");
    assert_eq!(
        (out[0].start_at, out[0].end_at),
        (0, 60_000),
        "A 被拉长到跨越空档了"
    );
    assert_eq!(
        tail.as_ref().map(|s| s.id.clone()),
        Some("s".to_string()),
        "短段自己进缓冲，等它自己的邻居"
    );
}

#[test]
fn an_adjacent_short_segment_is_still_merged() {
    // 与上一条互补：守住「紧邻的照并」。
    let mut tail = Some(mk("a", 0, 60_000, "work"));
    let mut out = Vec::new();
    crate::normalize::settle(&mut tail, &mut out, mk("s", 60_000, 61_000, "work"), 30_000);

    assert!(out.is_empty(), "还没有段定稿");
    let t = tail.expect("缓冲里应还是那一段");
    assert_eq!((t.start_at, t.end_at), (0, 61_000), "紧邻的短段要并进去");
}

#[test]
fn an_idle_short_segment_is_not_merged_into_work() {
    // B12 的第二半：idle 是「没在工作」，并进 work 等于把空闲算成工作。
    let mut tail = Some(mk("w", 0, 60_000, "work"));
    let mut out = Vec::new();
    crate::normalize::settle(&mut tail, &mut out, mk("i", 60_000, 61_000, "idle"), 30_000);

    assert_eq!(out.len(), 1, "work 段被撑长就意味着把空闲算成了工作");
    assert_eq!((out[0].start_at, out[0].end_at), (0, 60_000));
    assert_eq!(tail.as_ref().map(|s| s.category.as_str()), Some("idle"));
}

// --- 空档守卫：引擎不得凭空造出时间 ---
//
// 下面这组是 2026-10-12 那天的真实事故：两条 explorer.exe 的 unknown 段
// 分别长 7.31h 与 10.98h，而那两段时间里**一条事件都没有**（采集侧如实
// 什么都没记）。段的起止与事件空档的边界**误差 0.0 秒** —— 它不是观测到的，
// 是 `extend` 的 `end_at.max(ts)` 从「上一条事件」一路拉到「下一条事件」造出来的。
//
// 一天 24 小时因此被记成 18.29 小时 unknown，界面随之说谎：
// 24h 时段分布 18/24 根柱子满格，专注度大片 100%。

/// 观测空档超过阈值时，把段收在**最后一条证据**处，空档本身不产生任何段。
#[test]
fn a_long_observation_gap_does_not_get_stretched_into_the_segment() {
    let hour = 3_600_000i64;
    // 09:15 一条事件，20:13 又一条 —— 中间 11 小时什么都没有
    let events = vec![focus(0, "explorer.exe"), focus(11 * hour, "explorer.exe")];
    let segs = all_segments(&events, &rules(), &emit_now());

    // 两条零长度的段，各自在自己的那条证据上，另一条是「正在生长」的那个。
    // 关键不是段数，是**时长**：空档那 11 小时没有分到任何一段上。
    assert_eq!(segs.len(), 2, "空档把同应用的连续使用切成了两段");
    assert_eq!(
        segs[0].end_at, 0,
        "第一段的结束时间应当是它自己的证据时间，而不是下一条事件的时间"
    );
    assert_eq!(
        segs[1].start_at, 11 * hour,
        "第二段从新事件开始 —— 空档没有被算进任何一段"
    );
}

/// 空档超过阈值时，空档应当**真的空着** —— 不是补一段 unknown。
#[test]
fn a_gap_longer_than_the_threshold_leaves_no_time_accounted_for() {
    let hour = 3_600_000i64;
    let events = vec![focus(0, "Code.exe"), focus(11 * hour, "Code.exe")];
    let segs = all_segments(&events, &rules(), &emit_now());
    let total: i64 = segs.iter().map(|s| s.duration_ms()).sum();
    assert_eq!(
        total, 0,
        "11 小时的空档里没有任何证据，总时长应当是 0 而不是 11 小时"
    );
}

/// 阈值以内的正常间隔**不受影响** —— 这是绝大多数真实场景。
#[test]
fn a_short_gap_still_extends_the_segment() {
    let events = vec![focus(0, "Code.exe"), focus(5_000, "Code.exe")];
    let segs = all_segments(&events, &rules(), &emit_now());
    assert_eq!(segs.len(), 1, "5 秒的间隔不该切段");
    assert_eq!(
        (segs[0].start_at, segs[0].end_at),
        (0, 5_000),
        "正常间隔应当照常延长"
    );
}

/// 阈值取自 `idle_threshold_s` 的量级但语义不同 ——
/// 那条是「无输入」，这条是「无观测」，所以复用同一个配置项但要能单独覆盖。
#[test]
fn the_gap_guard_follows_its_own_threshold() {
    let hour = 3_600_000i64;
    let events = vec![focus(0, "Code.exe"), focus(2 * hour, "Code.exe")];
    // 阈值 4 小时 > 2 小时间隔 → 照常延长
    let loose = EngineConfig::default()
        .with_min_segment_duration_s(0)

        .with_max_observation_gap_s(4 * 3600);
    let segs = all_segments(&events, &rules(), &loose);
    assert_eq!((segs[0].start_at, segs[0].end_at), (0, 2 * hour), "宽松阈值下应当延长");

    // 阈值 1 小时 < 2 小时间隔 → 收在最后一条证据
    let tight = EngineConfig::default()
        .with_min_segment_duration_s(0)

        .with_max_observation_gap_s(3600);
    let segs = all_segments(&events, &rules(), &tight);
    assert_eq!((segs[0].start_at, segs[0].end_at), (0, 0), "严格阈值下应当收尾");
}


// --- 重叠：切回来不许越过中间已经收尾的那一段 ---

/// 复现真实事故（2026-10-12）：09:07:14 开的那一段被**复活三次**，而期间
/// msedge 已经另开又关闭了 09:07:46→09:08:29 这一段。最终复活段
/// [09:07:14, 09:09:52] 与它真实相交 43.4 秒。
///
/// 引擎**不该产出重叠段**：时间线上的每一段都代表「这段时间在做什么」，
/// 两段重叠等于同一段时间有两个答案。
#[test]
fn coming_back_does_not_swallow_the_segment_in_between() {
    let base = 1_791_767_234_996i64;
    // A: 09:07:14 起（work）
    // B: 09:07:46 开、09:08:29 关（browsing）—— A 与 C 之间的那一段
    // C: 09:08:29 起（work）—— 回到 A 的应用
    let events = vec![
        focus(base, "WindowsTerminal.exe"),
        focus(base + 32_000, "msedge.exe"),
        focus(base + 75_000, "WindowsTerminal.exe"), // B 收尾
        focus(base + 76_000, "WindowsTerminal.exe"), // C 开 → 抖动 → absorb
    ];

    // 归一化关闭：这里的重点是切段形状本身不重叠，不借合并之力。
    let cfg = EngineConfig::default().with_min_segment_duration_s(0);
    let segs = all_segments(&events, &rules(), &cfg);

    for w in segs.windows(2) {
        assert!(
            w[0].end_at <= w[1].start_at,
            "重叠段：[{}+, {}+] 与 [{}+, {}+]",
            w[0].start_at,
            w[0].end_at,
            w[1].start_at,
            w[1].end_at
        );
    }
}

/// 不管归一化怎么判定，**引擎产出的段两两不得重叠**。
/// 这条是「时间线上一段时间只能有一个答案」的形式化。
#[test]
fn segments_never_overlap_each_other() {
    // A → B → A 的典型抖动，配上各种时间间隔扫一遍
    for gap_ms in [1_000i64, 20_000, 45_000, 70_000] {
        let base = 1_000_000_000_000i64;
        let events = vec![
            focus(base, "Code.exe"),
            focus(base + gap_ms, "chrome.exe"),
            focus(base + 2 * gap_ms, "Code.exe"),
            focus(base + 3 * gap_ms, "chrome.exe"),
            focus(base + 4 * gap_ms, "Code.exe"),
        ];
        let cfg = EngineConfig::default().with_min_segment_duration_s(0);
        let segs = all_segments(&events, &rules(), &cfg);
        for w in segs.windows(2) {
            assert!(
                w[0].end_at <= w[1].start_at,
                "gap={gap_ms}ms 时产出重叠：[{}+, {}+] vs [{}+, {}+]",
                w[0].start_at,
                w[0].end_at,
                w[1].start_at,
                w[1].end_at
            );
        }
    }
}

/// 短段合并**不许跨过已经存在的时间**。
///
/// 归一化允许「相交或首尾相接」就并 —— 这是为了让 3 秒的抖动
/// 能并进邻居。但如果 `extra` 比 target 长、且它跨过了别的时间，
/// `merge_into` 的 `min(start)/max(end)` 会把 target 撑大到覆盖那段，
/// 于是 target 与它中间的真实段**重叠**。
///
/// 生产默认（`min_segment_duration_s = 30`）下真实重放 2026-10-10
/// 复现出 9 对重叠、2026-10-07 复现出 14 对，与库里逐条吻合；
/// `min=0` 时（没有短段、没有合并）重叠为 0。
#[test]
fn merging_a_short_segment_never_widens_the_target_across_other_time() {
    // 用默认参数：min=30 才有短段，才会走合并路径
    let cfg = EngineConfig::default();
    let base = 1_700_000_000_000i64;
    // A 很长(0..600s)，中间插入 B(300..320s) 短段，C(400..410s) 短段
    // B 与 C 都短于 30s? 否 —— 它们各 20s 与 10s，才算短段
    let events = vec![
        focus(base, "Code.exe"),
        focus(base + 300_000, "chrome.exe"),
        focus(base + 320_000, "Code.exe"),
        focus(base + 400_000, "chrome.exe"),
        focus(base + 410_000, "Code.exe"),
        // 再喂足够久，让 pending 全部释放
        focus(base + 600_000, "Code.exe"),
    ];
    let segs = all_segments(&events, &rules(), &cfg);
    for w in segs.windows(2) {
        assert!(
            w[0].end_at <= w[1].start_at,
            "合并后产出重叠：[{}+, {}+] 与 [{}+, {}+]",
            w[0].start_at, w[0].end_at, w[1].start_at, w[1].end_at
        );
    }
}

// ===========================================================================
// 段的时间边界：全局不变式
//
// 2026-10-13：空档守卫、重叠去重、切换次数同名那三轮修下来，根因是同一个 ——
// 段的 start_at / end_at 在 segmenter.rs 里有 6 处独立地写，每处自己决定
// 往哪边取 min/max。空档守卫只加在 close_current 一处，另外几处照样越界。
//
// 这条测试钉住「任意两条段不得重叠」这个全局不变式。它不是为了复现某个
// 具体 bug（那种合成用例会被绕过，见 STATUS §4.3），而是为了让**任何改动**
// 之后它都得是绿的。
// ===========================================================================

/// 一整天的段两两不得重叠，且时长不超过墙钟跨度。
///
/// 「时长不超过墙钟跨度」比「不重叠」更强：它是引擎「不凭空造时间」这条
/// 原则的可观测形式（合并也可能把墙钟拉长，而不重叠未必能抓到）。
#[test]
fn segments_of_a_whole_day_never_overlap() {
    let mut st = EngineState::initial();
    let mut all = Vec::new();
    let mut ts = 1_700_000_000_000i64;

    let apps = ["Code.exe", "msedge.exe", "WindowsTerminal.exe", "explorer.exe"];
    // 密集事件 + 抖动：标题变化、心跳、focus 来回切。
    // 密度是真实重叠的触发条件（10-10 那段 77 分钟 4117 个事件），
    // 这里按 ~100ms 一个事件的密度跑一小时。
    for i in 0..6_000i64 {
        let app = apps[(i as usize) % apps.len()];
        ts += 100;
        let ev = match i % 4 {
            0 => focus(ts, app),
            1 => {
                Event::new(
                    EventType::WindowTitleChange(WindowTitleChangePayload {
                        process_name: app.into(),
                        window_title: Some(format!("t{i}")),
                    }),
                    ts,
                )
            }
            2 => {
                Event::new(
                    EventType::InputHeartbeat(InputHeartbeatPayload { active_seconds: 10, window_start_ms: 0 }),
                    ts,
                )
            }
            _ => focus(ts + 40, apps[(i as usize + 1) % apps.len()]),
        };
        let out = crate::reduce(&st, &ev, &rules(), &EngineConfig::default());
        st = out.state;
        all.extend(out.closed_segments);
    }

    // 末尾还在 pending 的当前段也要算进来，否则「一段都没释放」时会越界
    if let Some(open) = st.open_segment_snapshot(&rules(), &EngineConfig::default()) {
        all.push(open);
    }
    all.sort_by_key(|s| s.start_at);
    if all.is_empty() {
        panic!("一段都没产生 —— 测试数据有问题");
    }
    let span = all.last().unwrap().end_at - all[0].start_at;
    let total: i64 = all.iter().map(|s| s.end_at - s.start_at).sum();

    for w in all.windows(2) {
        assert!(
            w[0].end_at <= w[1].start_at,
            "段两两重叠：[{}+, {}+] 与 [{}+, {}+]（{} vs {}）",
            w[0].start_at,
            w[0].end_at,
            w[1].start_at,
            w[1].end_at,
            w[0].category.as_str(),
            w[1].category.as_str()
        );
    }

    assert!(
        total <= span,
        "段总时长 {}ms 超过墙钟跨度 {}ms —— 引擎凭空造了时间",
        total,
        span
    );
}

/// 把正在生长的段**往前拽**之前，必须确认它与缓冲里那一段真的相接。
///
/// 2026-10-13 的重叠事故：旧 `fold_short_segments` 并进 `open` 的分支没查相邻，
/// 于是 `o.start_at = min(...)` 把正在生长的段拽过中间已落库的段，产出重叠。
/// 现在只有 `normalize::reopen` 会动 open 的起点，而它要求
/// 「同应用同类 ∧ 首尾相接」。
#[test]
fn reopening_the_held_segment_requires_adjacency() {
    let mut tail = Some(mk("prev", 0, 40_000, "browsing"));
    let mut open = crate::OpenSegment {
        start_at: 100_000,
        end_at: 120_000,
        category: crate::Category::Browsing,
        application: Some("msedge.exe".into()),
        confidence: 1.0,
        evidence_event_ids: vec!["ev-open".into()],
        last_evidence_at: 120_000,
        active_ms: 0,
    };

    crate::normalize::absorb_tail_into_open(&mut tail, &mut open, 30_000);

    assert_eq!(
        open.start_at, 100_000,
        "中间隔着 60 秒空档，不许把当前段的起点拽回去"
    );
    assert!(tail.is_some(), "没并成就别把缓冲里的段吞掉");
}

#[test]
fn reopening_a_matching_segment_right_after_it_ends_is_allowed() {
    // 这才是「切回来」：Code → chrome(2s) → Code 应该长成一段，
    // 而不是两根紧挨着的同应用段。
    let mut tail = Some(mk("prev", 0, 40_000, "work"));
    let mut open = crate::OpenSegment {
        start_at: 40_000,
        end_at: 40_000,
        category: crate::Category::Work,
        application: Some("Code.exe".into()),
        confidence: 1.0,
        evidence_event_ids: vec!["ev-open".into()],
        last_evidence_at: 40_000,
        active_ms: 0,
    };

    crate::normalize::absorb_tail_into_open(&mut tail, &mut open, 30_000);

    assert_eq!(open.start_at, 0, "接得上的同应用段应当长成一段");
    assert_eq!(open.evidence_event_ids.len(), 2, "两边的证据都要留下");
    assert!(tail.is_none(), "它已经变成当前段了");
}

// --- 「有输入时长」（2026-10-13，专注度的新分母）---
//
// 用户拍板的口径：`focus = 主导类别时长 / 有输入时长`。
// 关键实测：看视频时心跳是 0 与非 0 **交替**（报 0 比例 14–74%），
// 但**从不连续 300 秒** —— 所以这个量能区分「人不在」与「人在但不专注」。

#[test]
fn a_segment_accumulates_the_time_the_user_actually_had_input() {
    let cfg = emit_now();
    let base = 1_700_000_000_000i64;
    // Code 段：40 秒里两次心跳各报 10 秒有输入
    let events = vec![
        focus(base, "Code.exe"),
        heartbeat_at(base + 10_000, 10, 10_000),
        heartbeat_at(base + 20_000, 10, 10_000),
        heartbeat_at(base + 30_000, 0, 10_000), // 这一窗没动
        heartbeat_at(base + 40_000, 10, 10_000),
        focus(base + 45_000, "msedge.exe"), // 切走，收尾
    ];
    let segs = all_segments(&events, &rules(), &cfg);
    let code = segs
        .iter()
        .find(|s| s.application.as_deref() == Some("Code.exe"))
        .expect("应该有 Code 段");
    assert_eq!(
        code.active_ms, 30_000,
        "40 秒里报了三次 10 秒有输入 = 30 秒；报 0 的那 10 秒不该算进去"
    );
    assert!(
        code.active_ms <= code.duration_ms(),
        "有输入时长不可能超过段的时长（{} > {}）",
        code.active_ms,
        code.duration_ms()
    );
}

#[test]
fn a_fully_idle_period_accumulates_no_input_time() {
    let cfg = emit_now();
    let base = 1_700_000_000_000i64;
    let events = vec![
        focus(base, "Code.exe"),
        heartbeat(base + 10_000, 0),
        heartbeat(base + 20_000, 0),
        heartbeat(base + 30_000, 0),
        focus(base + 40_000, "msedge.exe"),
    ];
    let segs = all_segments(&events, &rules(), &cfg);
    let code = segs.iter().find(|s| s.application.as_deref() == Some("Code.exe")).unwrap();
    assert_eq!(code.active_ms, 0, "全程无输入 → 有输入时长为 0");
}

#[test]
fn active_ms_never_exceeds_the_segments_own_duration() {
    // 心跳窗口可能跨过段的边界：段只开了 5 秒，心跳却报了 10 秒。
    // 那样 active_ms 会大于段时长，专注度分母比分子还大 —— 荒谬。
    let cfg = emit_now();
    let base = 1_700_000_000_000i64;
    let events = vec![
        focus(base, "Code.exe"),
        heartbeat(base + 2_000, 10), // 段才开了 2 秒
        focus(base + 3_000, "msedge.exe"),
    ];
    let segs = all_segments(&events, &rules(), &cfg);
    for s in &segs {
        assert!(
            s.active_ms <= s.duration_ms(),
            "{:?} 的 active_ms={} 超过自身时长 {}",
            s.application,
            s.active_ms,
            s.duration_ms()
        );
    }
}


/// 段外的那几秒不该算到段头上。
///
/// 2026-10-13 定的口径：心跳报的是「过去 10 秒里有几秒在动」，
/// 而段只覆盖其中一部分 —— 段外的那几秒不算到它头上。
/// 这就是心跳必须带窗口起点（`window_start_ms`）的原因。
#[test]
fn input_time_is_counted_only_within_the_segment() {
    let cfg = EngineConfig::default().with_min_segment_duration_s(0);
    let base = 1_700_000_000_000i64;
    // 段 [0, 20s]。三个心跳的窗口分别是 [0,10s] / [5s,15s] / [10s,20s]，
    // **全都落在段内**，各报 8 秒。
    // 合计 24 秒 > 段墙钟 20 秒 —— 所以结果就是 20 秒（被总量夹住）。
    let events = vec![
        focus(base, "Code.exe"),
        heartbeat_at(base + 10_000, 8, 10_000),
        heartbeat_at(base + 15_000, 8, 10_000),
        heartbeat_at(base + 20_000, 8, 10_000),
        focus(base + 20_000, "msedge.exe"),
    ];
    let segs = all_segments(&events, &rules(), &cfg);
    let code = segs.iter().find(|s| s.application.as_deref() == Some("Code.exe")).unwrap();
    assert_eq!(
        code.active_ms, 20_000,
        "三个窗口全在段内，累到段自身的墙钟时长为止（24 秒装不进 20 秒的段）"
    );
}

/// 「有输入时长」不可能超过段的墙钟时长。
///
/// 三个窗口各报 8 秒 = 24 秒，但段只开了 20 秒 —— 装不下。
/// 这是「没有证据就没有时间」那条原则在**总量**上的体现：
/// 分母不能大于它衡量的那个东西。
#[test]
fn input_time_never_exceeds_the_segments_own_duration() {
    let cfg = EngineConfig::default().with_min_segment_duration_s(0);
    let base = 1_700_000_000_000i64;
    let events = vec![
        focus(base, "Code.exe"),
        heartbeat_at(base + 5_000, 10, 10_000), // 窗口 [-5s, 5s]  → 段内 5s
        heartbeat_at(base + 10_000, 10, 10_000), // 窗口 [0, 10s]  → 段内 10s
        heartbeat_at(base + 15_000, 10, 10_000), // 窗口 [5s, 15s] → 段内 10s
        heartbeat_at(base + 20_000, 10, 10_000), // 窗口 [10s, 20s] → 段内 10s
        focus(base + 20_000, "msedge.exe"), // 切走
    ];
    let segs = all_segments(&events, &rules(), &cfg);
    for s in &segs {
        assert!(
            s.active_ms <= s.end_at - s.start_at,
            "{:?} 的 active_ms={} 超过自身墙钟 {}",
            s.application,
            s.active_ms,
            s.end_at - s.start_at
        );
    }
}
