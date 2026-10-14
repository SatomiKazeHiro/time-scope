//! 驱动 engine 的运行时：持有状态、喂 Event、交出可落库的段。
//!
//! **纯逻辑、零 IO**：它不碰数据库、不开文件。IO 全部在调用方（app 层）做。
//! 这样引擎状态机可以脱离 DB 单测，而 `cargo test` 也不需要建库。

use activity_core::Event;
use activity_engine::{reduce, ActivitySegment, EngineConfig, EngineState, RuleSet};
use activity_storage::StoredSegment;

pub struct EngineRuntime {
    /// 公开给同 crate 的测试与诊断使用；改状态请走 `ingest`。
    pub state: EngineState,
    pub rules: RuleSet,
    pub config: EngineConfig,
}

impl EngineRuntime {
    pub fn new(rules: RuleSet, config: EngineConfig) -> Self {
        Self {
            state: EngineState::initial(),
            rules,
            config,
        }
    }

    /// 冷启动：把一串历史 Event 重放一遍，返回 `(运行时, 期间关闭的段)`。
    ///
    /// 调用方负责**先清掉那天的 activities**（spec §7.4 的重放），
    /// 再把这里返回的段落库。不清会累积重复段。
    pub fn bootstrap(
        events: &[Event],
        rules: RuleSet,
        config: EngineConfig,
    ) -> (Self, Vec<ActivitySegment>) {
        let mut rt = Self::new(rules, config);
        let mut closed = Vec::new();
        for e in events {
            closed.extend(rt.ingest(e));
        }
        (rt, closed)
    }

    /// 喂一个 Event，返回因此关闭、可落库的段。
    pub fn ingest(&mut self, event: &Event) -> Vec<ActivitySegment> {
        let out = reduce(&self.state, event, &self.rules, &self.config);
        self.state = out.state;
        out.closed_segments
    }

    /// 某天的全部段 = 已落库的 + 正在生长的当前段（裁定 B）。
    ///
    /// 当前段还**没有**落库，这里只是投影出来让时间线保持实时。
    ///
    /// 归属用**区间相交**而不是“end_at 落在当天”：一个 23:50 开始、次日 01:00
    /// 结束的段，在它落库之前（也就是次日 01:00 之前）两天都查不到它，
    /// 用户会看到“昨天最靠近午夜的一段凭空没了”。相交则两天都能看到它。
    /// 结束于午夜的段（end_at == day_end）只算前一天，不重复。
    pub fn segments_for_day(
        &self,
        stored: Vec<StoredSegment>,
        day_start_ms: i64,
        day_end_ms: i64,
    ) -> Vec<StoredSegment> {
        let mut segs = stored;
        if let Some(open) = self
            .state
            .open_segment_snapshot(&self.rules, &self.config)
        {
            if open.end_at > day_start_ms && open.start_at < day_end_ms {
                segs.push(segment_to_stored(&open));
            }
        }
        segs.sort_by_key(|s| s.start_at);
        segs
    }

}

/// engine 的 `ActivitySegment` -> storage 的 `StoredSegment`。
///
/// engine crate 不依赖 storage，所以这个转换住在同时认识两者的 app 层。
pub fn segment_to_stored(s: &ActivitySegment) -> StoredSegment {
    StoredSegment {
        id: s.id.clone(),
        start_at: s.start_at,
        end_at: s.end_at,
        category: s.category.as_str().to_string(),
        application: s.application.clone(),
        confidence: s.confidence,
        classifier: s.classifier.clone(),
        classifier_version: s.classifier_version.clone(),
        // 引擎算出来的量，专注度的新分母（2026-10-13）
        active_ms: Some(s.active_ms),
        evidence_event_ids: Vec::new(),
    }
}

/// 批量转成 `insert_segments` 需要的 `(段, evidence)` 配对。
pub fn to_stored(segments: &[ActivitySegment]) -> Vec<(StoredSegment, Vec<String>)> {
    segments
        .iter()
        .map(|s| (segment_to_stored(s), s.evidence_event_ids.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{to_stored, EngineRuntime};
    use activity_core::{Event, EventType, WindowFocusPayload};
    use activity_engine::{EngineConfig, RuleSet, DEFAULT_RULES_TOML};
    use activity_storage::{
        delete_segments_for_day, get_events_in_range, get_segments_in_range, insert_events,
        insert_segments, open_in_memory_shared, StoredSegment,
    };

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

    fn rules() -> RuleSet {
        RuleSet::from_toml(DEFAULT_RULES_TOML).unwrap()
    }

    /// min=0 让段立刻可落库，grace=0 排除抖动语义，专测"重放"这件事。
    fn cfg() -> EngineConfig {
        EngineConfig::default()
            .with_min_segment_duration_s(0)
            .with_grace_period_s(0)
    }

    fn rt() -> EngineRuntime {
        EngineRuntime::new(rules(), cfg())
    }

    #[test]
    fn ingest_returns_nothing_until_a_segment_closes() {
        let mut r = rt();
        assert!(
            r.ingest(&focus(0, "Code.exe")).is_empty(),
            "未关闭的段不该被返回"
        );
        assert_eq!(
            r.ingest(&focus(600_000, "chrome.exe")).len(),
            1,
            "切换时前一段关闭"
        );
    }

    #[test]
    fn closed_segments_are_stored_shape_ready() {
        let mut r = rt();
        r.ingest(&focus(0, "Code.exe"));
        let closed = r.ingest(&focus(600_000, "chrome.exe"));
        let stored = to_stored(&closed);
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].0.category, "work");
        assert_eq!(stored[0].0.classifier, "rule");
        assert!(!stored[0].0.classifier_version.is_empty());
        assert!(!stored[0].1.is_empty(), "关闭的段应带 evidence");
    }

    #[test]
    fn bootstrap_replays_events_and_emits_the_closed_ones() {
        let events = vec![
            focus(0, "Code.exe"),
            focus(600_000, "chrome.exe"),
            focus(1_200_000, "Code.exe"),
        ];
        let (r, closed) = EngineRuntime::bootstrap(&events, rules(), cfg());
        assert!(
            closed.len() >= 2,
            "重放 3 次切换应产出多段，实际 {}",
            closed.len()
        );
        let pairs = to_stored(&closed);
        let cats: Vec<&str> = pairs.iter().map(|s| s.0.category.as_str()).collect();
        assert!(cats.contains(&"work"), "{:?}", cats);
        assert!(cats.contains(&"browsing"), "{:?}", cats);
        assert!(
            r.state.current_segment.is_some(),
            "最后一段仍应是打开的"
        );
    }

    #[test]
    fn bootstrap_on_empty_input_is_harmless() {
        let (r, closed) = EngineRuntime::bootstrap(&[], rules(), cfg());
        assert!(closed.is_empty());
        assert!(r.state.current_segment.is_none());
        assert!(r
            .state
            .open_segment_snapshot(&r.rules, &r.config)
            .is_none());
    }

    #[test]
    fn replay_is_idempotent() {
        // 重放两次同样的事件，段数不应翻倍
        let events = vec![focus(0, "Code.exe"), focus(600_000, "chrome.exe")];
        let run = || {
            let (r, closed) = EngineRuntime::bootstrap(&events, rules(), cfg());
            closed.len() + usize::from(r.state.current_segment.is_some())
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn segments_for_day_appends_the_open_segment_and_sorts() {
        let (r, closed) = EngineRuntime::bootstrap(
            &[focus(0, "Code.exe"), focus(600_000, "chrome.exe")],
            rules(),
            cfg(),
        );
        let stored: Vec<StoredSegment> = to_stored(&closed)
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        let with_open = r.segments_for_day(stored, 0, 2_000_000);
        assert!(!with_open.is_empty(), "应附上正在生长的当前段");
        for w in with_open.windows(2) {
            assert!(
                w[0].start_at <= w[1].start_at,
                "未排序: {:?} {:?}",
                w[0],
                w[1]
            );
        }
    }

    #[test]
    fn segments_for_day_clamps_open_segment_to_the_requested_day() {
        // 当前段落在别的日子时不该被算进今天
        let (r, closed) = EngineRuntime::bootstrap(&[focus(5_000_000, "Code.exe")], rules(), cfg());
        let stored: Vec<StoredSegment> = to_stored(&closed)
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        let today = r.segments_for_day(stored, 0, 1_000_000);
        assert!(today.is_empty(), "今天不该看到 5,000,000ms 处的段");
    }

    #[test]
    fn bootstrap_then_persist_round_trips_through_storage() {
        let conn = open_in_memory_shared();
        let events = vec![focus(0, "Code.exe"), focus(600_000, "chrome.exe")];
        let (_r, closed) = EngineRuntime::bootstrap(&events, rules(), cfg());
        insert_segments(&conn.lock().unwrap(), &to_stored(&closed)).unwrap();

        let rows = get_segments_in_range(&conn.lock().unwrap(), 0, 2_000_000).unwrap();
        assert!(!rows.is_empty());
        assert!(rows
            .iter()
            .all(|s| s.category == "work" || s.category == "browsing"));
    }

    #[test]
    fn full_replay_cycle_replaces_previous_results() {
        // spec §7.4：重放 = 先清当天 activities，再重跑。不清会累积重复段。
        let conn = open_in_memory_shared();
        let events = vec![focus(0, "Code.exe"), focus(600_000, "chrome.exe")];

        for _ in 0..2 {
            {
                let c = conn.lock().unwrap();
                delete_segments_for_day(&c, 0, 2_000_000).unwrap();
            }
            let (_r, closed) = EngineRuntime::bootstrap(&events, rules(), cfg());
            insert_segments(&conn.lock().unwrap(), &to_stored(&closed)).unwrap();
        }

        let rows = get_segments_in_range(&conn.lock().unwrap(), 0, 2_000_000).unwrap();
        assert!(
            rows.len() <= 2,
            "重放两遍不该累积，拿到 {} 段",
            rows.len()
        );
    }

    #[test]
    fn replay_does_not_modify_the_events_table() {
        let conn = open_in_memory_shared();
        let events = vec![focus(0, "Code.exe"), focus(600_000, "chrome.exe")];
        insert_events(&conn.lock().unwrap(), &events).unwrap();
        let (_r, _closed) = EngineRuntime::bootstrap(&events, rules(), cfg());
        let rows = get_events_in_range(&conn.lock().unwrap(), 0, 2_000_000).unwrap();
        assert_eq!(rows.len(), 2, "events 表不应被重放修改");
    }
}

#[cfg(test)]
mod day_replay_tests {
    use super::*;
    use activity_core::{Event, EventType, WindowFocusPayload};
    use activity_engine::RuleSet;
    use activity_storage::{
        delete_segments_for_day, get_events_in_range, get_segments_in_range, insert_events,
        open_in_memory_shared, SharedConn,
    };

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

    fn rules() -> RuleSet {
        RuleSet::from_toml(activity_engine::DEFAULT_RULES_TOML).unwrap()
    }

    fn cfg() -> EngineConfig {
        EngineConfig::default()
            .with_min_segment_duration_s(0)
            .with_grace_period_s(0)
    }

    fn persist_segments(conn: &SharedConn, closed: &[activity_engine::ActivitySegment]) {
        let c = conn.lock().unwrap();
        activity_storage::insert_segments(&c, &to_stored(closed)).unwrap();
    }

    fn read_events(conn: &SharedConn, start: i64, end: i64) -> Vec<Event> {
        let rows = get_events_in_range(&conn.lock().unwrap(), start, end).unwrap();
        rows.iter()
            .filter_map(|stored| {
                serde_json::from_str::<EventType>(&stored.payload).ok().map(|event_type| Event {
                    id: stored.id.clone(),
                    timestamp: stored.timestamp,
                    event_type,
                })
            })
            .collect()
    }

    #[test]
    fn replaying_a_day_is_idempotent_per_day() {
        // 同一天重放两次不该累积段
        let conn = open_in_memory_shared();
        let events = vec![focus(0, "Code.exe"), focus(600_000, "chrome.exe")];
        insert_events(&conn.lock().unwrap(), &events).unwrap();

        for _ in 0..2 {
            {
                let c = conn.lock().unwrap();
                delete_segments_for_day(&c, 0, 86_400_000).unwrap();
            }
            let (rt, closed) = EngineRuntime::bootstrap(&read_events(&conn, 0, 86_400_000), rules(), cfg());
            persist_segments(&conn, &closed);
            let _ = rt;
        }
        let rows = get_segments_in_range(&conn.lock().unwrap(), 0, 86_400_000).unwrap();
        assert!(rows.len() <= 2, "重放两遍不该累积，拿到 {}", rows.len());
    }

    #[test]
    fn replaying_today_does_not_disturb_yesterdays_segments() {
        // 关键：按需重放"昨天"不能动"今天"已经算好的段
        let conn = open_in_memory_shared();
        insert_events(&conn.lock().unwrap(), &[focus(0, "Code.exe"), focus(600_000, "QQ.exe")]).unwrap();
        {
            let c = conn.lock().unwrap();
            delete_segments_for_day(&c, 0, 86_400_000).unwrap();
        }
        let (_rt, closed) = EngineRuntime::bootstrap(&read_events(&conn, 0, 86_400_000), rules(), cfg());
        persist_segments(&conn, &closed);
        let today_count = get_segments_in_range(&conn.lock().unwrap(), 0, 86_400_000).unwrap().len();
        assert!(today_count > 0);

        // 重放昨天（该天没有 event）
        {
            let c = conn.lock().unwrap();
            delete_segments_for_day(&c, -86_400_000, 0).unwrap();
        }
        let (_rt2, closed2) = EngineRuntime::bootstrap(&read_events(&conn, -86_400_000, 0), rules(), cfg());
        persist_segments(&conn, &closed2);

        let still_there = get_segments_in_range(&conn.lock().unwrap(), 0, 86_400_000).unwrap().len();
        assert_eq!(still_there, today_count, "重放昨天不该动今天的段");
    }
}


#[cfg(test)]
mod midnight_tests {
    use super::*;
    use activity_core::{Event, EventType, WindowFocusPayload};
    use activity_engine::RuleSet;

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

    const DAY: i64 = 86_400_000;

    #[test]
    fn a_segment_spanning_midnight_shows_up_in_both_days() {
        // 23:50 开始、次日 01:00 还在生长的段。用户翻到昨天或今天，都该看到它——
        // 否则"缺了一块"看起来像数据丢了。
        let rules = RuleSet::from_toml(activity_engine::DEFAULT_RULES_TOML).unwrap();
        let cfg = EngineConfig::default()
            .with_min_segment_duration_s(0)
            .with_grace_period_s(0);
        let mut rt = EngineRuntime::new(rules, cfg);

        // 前一天 23:50 开段
        rt.ingest(&focus(DAY - 600_000, "Code.exe"));
        // 跨过零点后又收到事件，把 open 段撑到次日 01:00
        rt.ingest(&focus(DAY + 3_600_000, "Code.exe"));

        let yesterday: Vec<StoredSegment> = rt.segments_for_day(vec![], DAY - DAY, DAY);
        let today: Vec<StoredSegment> = rt.segments_for_day(vec![], DAY, DAY + DAY);
        assert_eq!(yesterday.len(), 1, "昨天应看到跨零点的段");
        assert_eq!(today.len(), 1, "今天也应看到同一个跨零点的段");
    }

    #[test]
    fn a_segment_entirely_within_one_day_is_not_duplicated() {
        let rules = RuleSet::from_toml(activity_engine::DEFAULT_RULES_TOML).unwrap();
        let cfg = EngineConfig::default()
            .with_min_segment_duration_s(0)
            .with_grace_period_s(0);
        let mut rt = EngineRuntime::new(rules, cfg);
        rt.ingest(&focus(DAY + 3_600_000, "Code.exe"));

        assert!(rt.segments_for_day(vec![], DAY, DAY + DAY).len() == 1);
        assert!(
            rt.segments_for_day(vec![], DAY - DAY, DAY).is_empty(),
            "今天的段不该出现在昨天"
        );
    }

    #[test]
    fn a_segment_ending_exactly_at_midnight_belongs_to_the_earlier_day_only() {
        let rules = RuleSet::from_toml(activity_engine::DEFAULT_RULES_TOML).unwrap();
        let cfg = EngineConfig::default()
            .with_min_segment_duration_s(0)
            .with_grace_period_s(0);
        let mut rt = EngineRuntime::new(rules, cfg);
        // 昨天 23:00 开段，最后一个事件正好在 00:00:00
        rt.ingest(&focus(DAY - 3_600_000, "Code.exe"));
        rt.ingest(&focus(DAY, "Code.exe"));

        assert!(rt.segments_for_day(vec![], DAY - DAY, DAY).len() == 1);
        assert!(
            rt.segments_for_day(vec![], DAY, DAY + DAY).is_empty(),
            "结束于午夜的段不该重复算进今天"
        );
    }

    /// 重放后残留的 open 段必须被钉在**最后一条重放事件**上。
    ///
    /// 2026-10-12 事故的第二条入口：应用重启时 `bootstrap_today` 重放当天事件，
    /// 返回的运行时**带着一条没收尾的 open 段**，直接接进实时流。
    /// 于是重启前的最后一条事件与重启后的第一条事件之间那段空白，
    /// 被当成连续工作记了下来 —— 一次重启凭空造出一段。
    ///
    /// 这里钉住它：不钉的话，`day_replay.rs:123` 那个返回值就在替引擎
    /// 撒一个谎（说「这段一直活到现在」）。
    #[test]
    fn a_replayed_open_segment_is_pinned_to_its_last_event() {
        let rules = RuleSet::from_toml(activity_engine::DEFAULT_RULES_TOML).unwrap();
        let cfg = EngineConfig::default()
            .with_min_segment_duration_s(0)
            .with_grace_period_s(0);
        // 重放到一半停下：只喂了 DAY 09:15 那一条，open 段没收到尾事件
        let (rt, _) = EngineRuntime::bootstrap(&[focus(DAY + 9 * 3_600_000, "explorer.exe")], rules, cfg);

        let segs = rt.segments_for_day(vec![], DAY, DAY + DAY);
        assert_eq!(segs.len(), 1, "重放后的 open 段应当仍然可见（它是当前段）");
        assert_eq!(
            (segs[0].start_at, segs[0].end_at),
            (DAY + 9 * 3_600_000, DAY + 9 * 3_600_000),
            "open 段的结束时间必须等于它最后一条证据，而不是「现在」"
        );
        assert_eq!(
            segs[0].end_at - segs[0].start_at,
            0,
            "一条证据撑不起一段时长 —— 否则重启一次就凭空多出一段时间"
        );
    }

    /// 重启场景端到端：重放 → 实时流恢复。
    ///
    /// 这才是 2026-10-12 的实际经过：09:15 有事件，应用退出，
    /// 20:13 重启后第一条实时事件到达 —— 空白被记成了一整段工作。
    /// 两条路径都要验：bootstrap 留下的 open 段，以及随后 extend 的那条。
    #[test]
    fn a_restart_between_two_events_does_not_invent_a_segment() {
        let hour = 3_600_000i64;
        let rules = RuleSet::from_toml(activity_engine::DEFAULT_RULES_TOML).unwrap();
        let cfg = EngineConfig::default()
            .with_min_segment_duration_s(0)
            .with_grace_period_s(0);
        // 启动时只重放了 09:15 那条
        let (mut rt, _) =
            EngineRuntime::bootstrap(&[focus(DAY + 9 * hour, "explorer.exe")], rules, cfg);

        // 20:13 应用恢复，第一条实时事件到达 —— 与上一条隔了 11 小时
        rt.ingest(&focus(DAY + 20 * hour, "explorer.exe"));

        let segs = rt.segments_for_day(vec![], DAY, DAY + DAY);
        let total: i64 = segs.iter().map(|s| s.end_at - s.start_at).sum();
        assert_eq!(
            total, 0,
            "11 小时的停机不该被记成工作时间（实际记了 {} ms）",
            total
        );
    }
}
