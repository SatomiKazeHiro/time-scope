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
    let (rs, cfg) = (rules(), EngineConfig::default().with_min_segment_duration_s(30));
    let mut st = reduce(&EngineState::initial(), &focus(0, "Code.exe"), &rs, &cfg).state;
    st = reduce(&st, &focus(1_000, "chrome.exe"), &rs, &cfg).state;
    let out = reduce(&st, &heartbeat(100_000, 3), &rs, &cfg);
    assert!(
        !out.closed_segments.is_empty(),
        "超过 min_segment_duration 的 pending 段应被释放"
    );
}
