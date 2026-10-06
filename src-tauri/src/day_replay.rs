use crate::date_range::day_range_ms;
use crate::engine_runtime::to_stored;
use crate::redact::Redactor;
use crate::rules::{load_rules, rules_path};
use activity_core::{Event, EventType};
use activity_engine::{EngineConfig, RuleSet};
use activity_storage::{get_events_in_range, SharedConn};
use std::collections::HashSet;
use std::sync::Mutex;
use time::UtcOffset;

/// 已经重放过的日期。同一��期只重放一次——`get_segments` 每次调用都跑一遍重放
/// 既浪费又会反复删/写当天的 activities。
#[derive(Debug, Default)]
pub struct ReplayedDays(Mutex<HashSet<String>>);

impl ReplayedDays {
    /// 该天是否已经重放**成功**过。
    pub fn already_played(&self, date: &str) -> bool {
        let set = match self.0.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        set.contains(date)
    }

    /// 标记该天已重放。**只在重放成功后调用**（B5）——
    /// 提前 claim 会让一次失败的重放再也重试不了。
    pub fn claim(&self, date: &str) -> bool {
        let mut set = match self.0.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        set.insert(date.to_string())
    }
}

/// 把某一天已有的 Event 重放成 ActivitySegment 并落库。
///
/// **为什么需要**：启动时只重放了"今天"（spec §7.4）。但上次崩溃/被杀时会留下
/// 几秒到几分钟的**孤儿事件**——它们在 events 表里，却从没被引擎处理过。
/// 用户翻到那一天时会看到空白，以为那天没记录到自己。
///
/// 返回 true 表示实际做了重放；false 表示失败（此时保留旧数据，不清空）。
pub fn replay_day_once(
    conn: &SharedConn,
    replayed: &ReplayedDays,
    rules: &RuleSet,
    config: &EngineConfig,
    date: &str,
    offset: UtcOffset,
    redactor: &Redactor,
) -> bool {
    replay_day(conn, replayed, rules, config, date, offset, redactor).is_some()
}

/// 同上，但把重放用的运行时交出来。启动时用它继续跑实时流——否则实时引擎从空状态
/// 开始，会看不见"当前正在生长的那一段"（重放产生的 open 段会被丢掉）。
fn replay_day(
    conn: &SharedConn,
    replayed: &ReplayedDays,
    rules: &RuleSet,
    config: &EngineConfig,
    date: &str,
    offset: UtcOffset,
    redactor: &Redactor,
) -> Option<crate::engine_runtime::EngineRuntime> {
    // 先看一眼有没有重放过，但**先不 claim**：claim 一旦消耗掉就再也回不来，
    // 而下面任何一步失败都必须能重试（B5）。真正的 claim 在写库成功之后。
    if replayed.already_played(date) {
        return None;
    }
    let Ok((start_ms, end_ms)) = day_range_ms(date, offset) else {
        return None;
    };

    let events: Vec<Event> = {
        let Ok(c) = conn.lock() else { return None };
        let Ok(rows) = get_events_in_range(&c, start_ms, end_ms) else {
            return None;
        };
        rows.iter()
            .filter_map(|stored| {
                // **对历史数据再脱敏一次**：脱敏功能上线前落库的标题不会被回溯改写，
                // 重放时经过这里能让**引擎看到的** context 不含敏感片段。
                // events 表里的原始行保持不变（不做数据改写）。
                //
                // 注意这只覆盖了引擎这一侧。**界面**读的是 events.payload 的原文，
                // 由 `titles_for` / `merge_top_titles` 各自再脱敏一次（A4）。
                // 这里曾经是唯一的脱敏出口，注释也就跟着写了「界面上就不会再露出
                // 旧数据里的敏感片段」——那句话当时是错的。
                let payload = redactor
                    .redact_payload(&stored.payload)
                    .unwrap_or_else(|| stored.payload.clone());
                serde_json::from_str::<EventType>(&payload).ok().map(|event_type| Event {
                    id: stored.id.clone(),
                    timestamp: stored.timestamp,
                    event_type,
                })
            })
            .collect()
    };

    // 「删当天 + 写当天」必须在**同一个事务**里（B5）。拆开的话会出现
    // 「删成功、写失败」的中间态：那天被清空，而 claim 已经消耗掉了，
    // 本进程内再也不会重放 —— 用户看到空白直到重启。
    let stored_segments = if events.is_empty() {
        // 那天本来就没有事件，但可能残留了上一版的段，清一下
        Vec::new()
    } else {
        let (rt, closed) =
            crate::engine_runtime::EngineRuntime::bootstrap(&events, rules.clone(), *config);
        let Ok(c) = conn.lock() else { return None };
        if let Err(e) = activity_storage::replace_day_segments(&c, start_ms, end_ms, &to_stored(&closed))
        {
            eprintln!("[time-scope] 重放 {date} 落库失败（未改动旧数据）: {e}");
            return None;
        }
        if !closed.is_empty() {
            eprintln!("[time-scope] 已重放 {date}：{} 段", closed.len());
        }
        replayed.claim(date);
        return Some(rt);
    };

    // 空事件那条路：只删不写，同样是单事务
    {
        let Ok(c) = conn.lock() else { return None };
        if let Err(e) =
            activity_storage::replace_day_segments(&c, start_ms, end_ms, &stored_segments)
        {
            eprintln!("[time-scope] 清理 {date} 的残留段失败: {e}");
            return None;
        }
    }
    replayed.claim(date);
    Some(crate::engine_runtime::EngineRuntime::new(rules.clone(), *config))
}

/// 启动时调用：准备规则集、重放当天，并返回**可继续驱动的运行时**。
pub fn bootstrap_today(
    conn: &SharedConn,
    replayed: &ReplayedDays,
    app_config: &crate::config::AppConfig,
) -> (RuleSet, EngineConfig, crate::engine_runtime::EngineRuntime) {
    let (rules, source) = load_rules(&rules_path());
    match source {
        crate::rules::RulesSource::Created => {
            eprintln!("[time-scope] 已生成默认规则 {}", rules_path().display())
        }
        crate::rules::RulesSource::Loaded => {
            eprintln!("[time-scope] 已加载规则 {}", rules_path().display())
        }
        crate::rules::RulesSource::FellBackToDefault => {
            eprintln!("[time-scope] 规则文件不可用，本次使用内置默认")
        }
    }
    // 引擎的分段参数来自 config.toml（spec §5.2）
    let config = app_config.to_engine_config();
    let redactor = Redactor::new(&rules.redact);
    let offset = time::UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let today = today_string();
    // 拿运行时而不是丢掉：实时流要从"重放后的状态"继续，
    // 否则当前正在生长的那一段会在实时引擎里丢失。
    let rt = replay_day(conn, replayed, &rules, &config, &today, offset, &redactor)
        .unwrap_or_else(|| crate::engine_runtime::EngineRuntime::new(rules.clone(), config));
    (rules, config, rt)
}

pub fn today_string() -> String {
    let d = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
}

#[cfg(test)]
mod tests {
    use super::*;
    use activity_storage::{get_segments_in_range, insert_events, open_in_memory_shared};


    pub(super) fn focus(ts: i64, app: &str) -> Event {
        Event::new(
            EventType::WindowFocus(activity_core::WindowFocusPayload {
                process_name: app.into(),
                window_title: None,
                exe_path: None,
            }),
            ts,
        )
    }

    pub(super) fn rules() -> RuleSet {
        RuleSet::from_toml(activity_engine::DEFAULT_RULES_TOML).unwrap()
    }

    pub(super) fn cfg() -> EngineConfig {
        EngineConfig::default()
            .with_min_segment_duration_s(0)
            .with_grace_period_s(0)
    }

    /// day_index 决定落在哪一天（相对某个基准日）
    pub(super) fn day_ts(day_index: i64, hour: i64) -> i64 {
        1_700_000_000_000 / 86_400_000 * 86_400_000 + day_index * 86_400_000 + hour * 3_600_000
    }

    pub(super) fn day_str(day_index: i64) -> String {
        let d = time::OffsetDateTime::from_unix_timestamp(day_ts(day_index, 12) / 1000)
            .unwrap()
            .date();
        format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
    }

    #[test]
    fn a_day_with_orphan_events_gets_segmented_on_first_view() {
        // 场景：上次崩溃，昨天的 events 留在表里但从没被引擎处理过
        let conn = open_in_memory_shared();
        let replayed = ReplayedDays::default();
        insert_events(
            &conn.lock().unwrap(),
            &[
                focus(day_ts(-1, 9), "Code.exe"),
                focus(day_ts(-1, 15), "chrome.exe"),
            ],
        )
        .unwrap();

        let done = replay_day_once(
            &conn,
            &replayed,
            &rules(),
            &cfg(),
            &day_str(-1),
            UtcOffset::UTC,
            &Redactor::default(),
        );
        assert!(done, "昨天应该被重放");

        let rows = get_segments_in_range(&conn.lock().unwrap(), day_ts(-1, 0), day_ts(0, 0)).unwrap();
        assert!(!rows.is_empty(), "昨天的孤儿事件应变成段");
    }

    #[test]
    fn a_day_is_replayed_only_once() {
        let conn = open_in_memory_shared();
        let replayed = ReplayedDays::default();
        insert_events(&conn.lock().unwrap(), &[focus(day_ts(-1, 9), "Code.exe")]).unwrap();

        assert!(replay_day_once(&conn, &replayed, &rules(), &cfg(), &day_str(-1), UtcOffset::UTC, &Redactor::default()));
        assert!(
            !replay_day_once(&conn, &replayed, &rules(), &cfg(), &day_str(-1), UtcOffset::UTC, &Redactor::default()),
            "同一天不该重放第二次"
        );
        let rows = get_segments_in_range(&conn.lock().unwrap(), day_ts(-1, 0), day_ts(0, 0)).unwrap();
        assert!(rows.len() <= 2, "不该累积，拿到 {} 段", rows.len());
    }

    #[test]
    fn an_empty_day_is_claimed_and_leaves_no_segments() {
        let conn = open_in_memory_shared();
        let replayed = ReplayedDays::default();
        assert!(replay_day_once(&conn, &replayed, &rules(), &cfg(), &day_str(-3), UtcOffset::UTC, &Redactor::default()));
        assert!(get_segments_in_range(&conn.lock().unwrap(), day_ts(-3, 0), day_ts(-2, 0))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn replaying_an_invalid_date_is_a_noop() {
        let conn = open_in_memory_shared();
        let replayed = ReplayedDays::default();
        assert!(!replay_day_once(&conn, &replayed, &rules(), &cfg(), "not-a-date", UtcOffset::UTC, &Redactor::default()));
    }
}


#[cfg(test)]
mod replay_redaction_tests {
    use super::*;
    use activity_storage::{insert_events, open_in_memory_shared};

    /// 时间戳必须落在"今天"，否则按天重放查不到它
    fn today_millis() -> i64 {
        activity_storage::now_ms()
    }

    fn focus(app: &str, title: &str) -> Event {
        Event::new(
            EventType::WindowFocus(activity_core::WindowFocusPayload {
                process_name: app.into(),
                window_title: Some(title.into()),
                exe_path: None,
            }),
            today_millis(),
        )
    }

    fn rule_set(patterns: &[&str]) -> RuleSet {
        let mut body = String::from(
            "\n[[rule]]\nid = \"slack\"\nprocess = [\"slack.exe\"]\ncategory = \"communication\"\nconfidence = 0.8\n",
        );
        for p in patterns {
            body.push_str(&format!("\n[[redact]]\npattern = '{}'\n", p));
        }
        RuleSet::from_toml(&body).unwrap()
    }

    fn local_offset() -> UtcOffset {
        UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC)
    }

    fn config() -> EngineConfig {
        EngineConfig::default()
            .with_min_segment_duration_s(0)
            .with_grace_period_s(0)
    }

    /// 唯一能看到窗口标题的地方是引擎的 `current_context`——落库的 segment 不存标题，
    /// 所以想验证重放确实做了脱敏，只能从这里看。
    fn replayed_title(rt: &crate::engine_runtime::EngineRuntime) -> Option<String> {
        rt.state
            .current_context
            .as_ref()
            .and_then(|c| c.window_title.clone())
    }

    #[test]
    fn replay_applies_redaction_to_legacy_rows() {
        // 场景：脱敏功能上线**之前**落库的明文标题。开启脱敏后翻看那一天，
        // 引擎看到的不该是明文。
        let conn = open_in_memory_shared();
        insert_events(
            &conn.lock().unwrap(),
            &[focus("slack.exe", "客户Alice 的订单 998877")],
        )
        .unwrap();

        let rt = replay_day(
            &conn,
            &ReplayedDays::default(),
            &rule_set(&[]),
            &config(),
            &today_string(),
            // 必须用本地偏移：today_string() 是本地日期，用 UTC 解析会在跨本地
            // 午夜时把区间算错，导致刚插入的事件落在范围外
            local_offset(),
            &Redactor::new(&["Alice".into(), r"订单 \d+".into()]),
        )
        .expect("应完成重放");

        let title = replayed_title(&rt).expect("引擎应记住当前标题");
        assert!(!title.contains("Alice"), "引擎不该看到明文：{title:?}");
        assert!(!title.contains("998877"), "引擎不该看到订单号：{title:?}");
        assert!(title.contains("[redacted]"), "应被替换为占位符：{title:?}");
    }

    #[test]
    fn without_a_redactor_the_legacy_title_passes_through_unchanged() {
        let conn = open_in_memory_shared();
        insert_events(&conn.lock().unwrap(), &[focus("slack.exe", "客户Alice")]).unwrap();

        let rt = replay_day(
            &conn,
            &ReplayedDays::default(),
            &rule_set(&[]),
            &config(),
            &today_string(),
            local_offset(),
            &Redactor::default(),
        )
        .expect("应完成重放");

        assert_eq!(replayed_title(&rt).as_deref(), Some("客户Alice"));
    }

    #[test]
    fn the_events_table_itself_is_never_rewritten() {
        // 脱敏只作用在"读出来往引擎走"的路径上；原始数据保持不变，
        // 这样用户随时能回去核对，也能自己决定要不要清库。
        let conn = open_in_memory_shared();
        insert_events(&conn.lock().unwrap(), &[focus("slack.exe", "客户Alice")]).unwrap();

        replay_day(
            &conn,
            &ReplayedDays::default(),
            &rule_set(&[]),
            &config(),
            &today_string(),
            local_offset(),
            &Redactor::new(&["Alice".into()]),
        );

        let rows =
            activity_storage::get_events_in_range(&conn.lock().unwrap(), 0, i64::MAX).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(
            rows[0].payload.contains("Alice"),
            "events 表的原始 payload 不该被改写：{}",
            rows[0].payload
        );
    }
}

#[cfg(test)]
mod atomicity_tests {
    use super::tests::{cfg, day_str, day_ts, focus, rules};
    use activity_storage::{get_segments_in_range, insert_events, open_in_memory_shared};
    use super::*;

    /// 让 `activities` 的 INSERT 必定失败。用来模拟「删成功、写失败」。
    fn block_inserts(conn: &SharedConn) {
        conn.lock()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER block_insert BEFORE INSERT ON activities
                 BEGIN SELECT RAISE(ABORT, 'simulated SQLITE_FULL'); END;",
            )
            .unwrap();
    }

    fn unblock_inserts(conn: &SharedConn) {
        conn.lock()
            .unwrap()
            .execute_batch("DROP TRIGGER block_insert")
            .unwrap();
    }

    fn seed_a_previous_version_of_the_day(conn: &SharedConn) {
        activity_storage::insert_segments(
            &conn.lock().unwrap(),
            &[(
                activity_storage::StoredSegment {
                    id: "stale".into(),
                    start_at: day_ts(-1, 1),
                    end_at: day_ts(-1, 2),
                    category: "study".into(),
                    application: Some("Old.exe".into()),
                    confidence: 1.0,
                    classifier: "rule".into(),
                    classifier_version: "old".into(),
                    evidence_event_ids: vec![],
                },
                vec![],
            )],
        )
        .unwrap();
    }

    fn try_replay(conn: &SharedConn, replayed: &ReplayedDays, date: &str) -> bool {
        replay_day_once(
            conn,
            replayed,
            &rules(),
            &cfg(),
            date,
            UtcOffset::UTC,
            &Redactor::default(),
        )
    }

    fn segments_of_that_day(conn: &SharedConn) -> Vec<activity_storage::StoredSegment> {
        get_segments_in_range(&conn.lock().unwrap(), day_ts(-1, 0), day_ts(0, 0)).unwrap()
    }

    /// 两个不同应用的焦点事件 → 引擎会**关闭**一段。
    ///
    /// 关键：只有一个事件时引擎产出的 `closed` 是空的，`replace_day_segments`
    /// 只删不写，压根不会碰到那个 INSERT 触发器，测不出任何东西。
    fn seed_two_switches(conn: &SharedConn) {
        insert_events(
            &conn.lock().unwrap(),
            &[
                focus(day_ts(-1, 9), "Code.exe"),
                focus(day_ts(-1, 15), "chrome.exe"),
            ],
        )
        .unwrap();
    }

    #[test]
    fn a_failed_write_does_not_destroy_the_previous_segments() {
        // B5。旧流程是「先 `claim` → 删（一个事务）→ 写（另一个事务）」。
        // 删成功、写失败（SQLITE_FULL / 磁盘满）→ 那天被清空，而
        // `claim` 已经消耗掉了 → 本进程内**永远不会重放**，用户看到空白直到重启。
        let conn = open_in_memory_shared();
        let replayed = ReplayedDays::default();
        seed_two_switches(&conn);
        seed_a_previous_version_of_the_day(&conn);
        block_inserts(&conn);

        assert!(!try_replay(&conn, &replayed, &day_str(-1)), "写入被挡住，重放应当失败");

        let left = segments_of_that_day(&conn);
        assert!(
            left.iter().any(|s| s.id == "stale"),
            "写失败不该把旧段删掉——用户至少还有东西看。实际剩 {:?}",
            left.iter().map(|s| &s.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_failed_replay_can_be_retried_afterwards() {
        // B5 的另一半：`claim` 必须移到**成功之后**。
        // 否则这次失败把那天标记成"重放过了"，再也重试不了。
        let conn = open_in_memory_shared();
        let replayed = ReplayedDays::default();
        seed_two_switches(&conn);
        block_inserts(&conn);
        assert!(!try_replay(&conn, &replayed, &day_str(-1)), "第一次应当失败");

        unblock_inserts(&conn);
        assert!(
            try_replay(&conn, &replayed, &day_str(-1)),
            "写失败不该消耗掉这一天的重放资格"
        );
        assert!(
            !segments_of_that_day(&conn).is_empty(),
            "重试成功后当天应当有段"
        );
    }

    #[test]
    fn a_successful_replay_still_claims_the_day() {
        // 守住另一半：把 `claim` 挪到成功之后，不等于「不 claim 了」。
        // 同一天仍然只重放一次。
        let conn = open_in_memory_shared();
        let replayed = ReplayedDays::default();
        insert_events(&conn.lock().unwrap(), &[focus(day_ts(-1, 9), "Code.exe")]).unwrap();
        assert!(try_replay(&conn, &replayed, &day_str(-1)));
        assert!(!try_replay(&conn, &replayed, &day_str(-1)), "同一天不该重放第二次");
    }
}
