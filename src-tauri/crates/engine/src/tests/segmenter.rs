use crate::{reduce, EngineConfig, EngineState, RuleSet, DEFAULT_RULES_TOML};
use activity_core::{Event, EventType, InputHeartbeatPayload, WindowFocusPayload};

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

fn heartbeat(ts: i64, secs: u8) -> Event {
    Event::new(
        EventType::InputHeartbeat(InputHeartbeatPayload { active_seconds: secs }),
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

/// 立即落库：min 和 grace 都设 0，落库门槛 max(0,0)=0。
/// 只想验证"分段的形状"而不涉及抖动语义时用它。
/// 涉及 grace 的测试必须保留默认 grace（见下面那几个）。
fn emit_now() -> EngineConfig {
    EngineConfig::default()
        .with_min_segment_duration_s(0)
        .with_grace_period_s(0)
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

// --- grace period（spec §7.3）---

#[test]
fn grace_period_absorbs_a_quick_switch_back() {
    // Code -> chrome(2s) -> Code，三段应合并成一个 Code 段。
    // 关键是"复活"前一个 Code 段，而不是把 chrome 段留下来。
    let cfg = EngineConfig::default()
        .with_min_segment_duration_s(0)
        .with_grace_period_s(60);
    let segs = all_segments(
        &[
            focus(0, "Code.exe"),
            focus(2_000, "chrome.exe"),
            focus(4_000, "Code.exe"),
        ],
        &rules(),
        &cfg,
    );
    assert_eq!(segs.len(), 1, "grace 内的往返切换不该留下多段，实际 {:?}", cats(&[
        focus(0, "Code.exe"), focus(2_000, "chrome.exe"), focus(4_000, "Code.exe")
    ]));
    assert_eq!(segs[0].application.as_deref(), Some("Code.exe"));
    assert_eq!(segs[0].start_at, 0);
    assert_eq!(segs[0].end_at, 4_000);
}

#[test]
fn grace_period_does_not_absorb_a_slow_switch_back() {
    let cfg = EngineConfig::default()
        .with_min_segment_duration_s(0)
        .with_grace_period_s(60);
    let segs = all_segments(
        &[
            focus(0, "Code.exe"),
            focus(10_000, "chrome.exe"),
            focus(200_000, "Code.exe"),
        ],
        &rules(),
        &cfg,
    );
    assert!(segs.len() >= 2, "超出 grace 的往返应保留为多段，实际 {}", segs.len());
}

#[test]
fn zero_grace_disables_absorption() {
    let cfg = EngineConfig::default()
        .with_min_segment_duration_s(0)
        .with_grace_period_s(0);
    let segs = all_segments(
        &[focus(0, "Code.exe"), focus(1_000, "chrome.exe"), focus(2_000, "Code.exe")],
        &rules(),
        &cfg,
    );
    assert_eq!(segs.len(), 3, "grace=0 时每次切换都切段");
}

#[test]
fn grace_absorbs_the_evidence_of_the_dropped_segment() {
    // 被判为抖动的那个段，其证据应并回复活段，而不是凭空消失
    let cfg = EngineConfig::default()
        .with_min_segment_duration_s(0)
        .with_grace_period_s(60);
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

// --- min_segment_duration 缓冲 ---

#[test]
fn short_segments_stay_pending_until_min_duration_elapses() {
    let (rs, cfg) = (rules(), EngineConfig::default().with_min_segment_duration_s(30));
    let mut st = reduce(&EngineState::initial(), &focus(0, "Code.exe"), &rs, &cfg).state;
    st = reduce(&st, &focus(1_000, "chrome.exe"), &rs, &cfg).state;
    assert!(st.current_segment.is_some(), "应该有正在生长的 chrome 段");
    let snapshot = st.open_segment_snapshot(&rs, &cfg).unwrap();
    assert_eq!(snapshot.application.as_deref(), Some("chrome.exe"));
    assert_eq!(st.pending.len(), 1);
    assert_eq!(st.pending[0].application.as_deref(), Some("Code.exe"));
}

#[test]
fn pending_segments_are_released_once_old_enough() {
    // 落库门槛 = max(min, grace) = max(0, 5s) = 5s。
    // min=0 表示不会被"太短"卡住，纯粹验证门槛本身。
    let (rs, cfg) = (
        rules(),
        EngineConfig::default()
            .with_min_segment_duration_s(0)
            .with_grace_period_s(5),
    );
    let mut st = reduce(&EngineState::initial(), &focus(0, "Code.exe"), &rs, &cfg).state;
    let after_switch = reduce(&st, &focus(1_000, "chrome.exe"), &rs, &cfg);
    assert!(
        after_switch.closed_segments.is_empty(),
        "刚切段还没到 5s 门槛，不该释放"
    );
    st = after_switch.state;

    let out = reduce(&st, &heartbeat(100_000, 3), &rs, &cfg);
    assert!(
        !out.closed_segments.is_empty(),
        "超过 5s 落库门槛后应被释放"
    );
}

// --- A. 无应用的事件不该开段 ---

#[test]
fn heartbeat_before_any_window_event_does_not_open_a_segment() {
    // 应用刚启动时第一个事件通常是心跳，而心跳不携带 application。
    // 这类事件无法归因，不该凭空开一个 "unknown / 无应用" 的段。
    let cfg = EngineConfig::default()
        .with_min_segment_duration_s(0)
        .with_grace_period_s(0);
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
        .with_grace_period_s(0);
    let (rs, cfg) = (rules(), cfg);
    let mut st = reduce(&EngineState::initial(), &focus(0, "Code.exe"), &rs, &cfg).state;
    st = reduce(&st, &heartbeat(5_000, 1), &rs, &cfg).state;
    let snap = st.open_segment_snapshot(&rs, &cfg).unwrap();
    assert_eq!(snap.application.as_deref(), Some("Code.exe"));
    assert_eq!(snap.end_at, 5_000);
}

// --- B. min_segment_duration 真的要把短段并掉 ---

/// 落库门槛 = max(min, grace)。这里 min=2s、grace=0 => 门槛 2s，
/// 小时间线就能让段真正"就绪"，专心验证合并行为。
fn merge_cfg() -> EngineConfig {
    EngineConfig::default()
        .with_min_segment_duration_s(2)
        .with_grace_period_s(0)
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
fn a_lone_short_segment_is_held_rather_than_emitted() {
    // 两侧都没有长邻居时，宁可先压着，也不要硬并进不相邻的段。
    // 这正是 min_segment_duration 在起作用：宁可不落库，也不要留一段噪声。
    let cfg = merge_cfg();
    let (closed, st) = run_cfg(
        &[
            focus(0, "Code.exe"),
            focus(1_000, "msedge.exe"),
            focus(5_000, "QQ.exe"),
        ],
        &cfg,
    );
    assert!(
        apps_of(&closed).is_empty(),
        "孤立的 1s 短段不该被发出去，实际 {:?}",
        apps_of(&closed)
    );
    assert!(
        st.pending.iter().any(|s| s.application.as_deref() == Some("Code.exe")),
        "它应留在 pending 里等一个长邻居出现，实际 {:?}",
        apps_of(&st.pending)
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

// --- pending 顺序（A1）---

#[test]
fn pending_order_survives_short_segments_being_held_back() {
    // A1：`keep` 段（够老但无长邻居、压着等下一轮的短段）被 append 到 pending 末尾，
    // 于是 pending 不再按时间排序，`absorb` 里的 `pending.last()` 取到的是
    // **十几秒前的老段**而不是刚离开的那一段 —— 把它拉到当前时刻就产出重叠段。
    //
    // 默认参数即可触发（min=30 / grace=60 → hold=max(30,60)=60s、give_up=180s），
    // 所以这里用默认配置。带 min_segment_duration_s(0) 的测试里 keep 恒为空、
    // 这条路径结构上不可达 —— 这正是漏网的原因。
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
