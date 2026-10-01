//! 事件总线（collector::consumer）的转换逻辑测试。
//!
//! 只测 `signal_to_event` 这个纯映射：RawSignal → Option<Event>。
//! 窗口类信号依赖真实 HWND，由 Task 7 的 smoke 测试覆盖。

use activity_collector::consumer::{redact_event, signal_to_event};
use activity_collector::signals::RawSignal;
use activity_core::EventType;

#[test]
fn idle_signal_becomes_system_idle_event() {
    let e = signal_to_event(RawSignal::IdleStart, 1_700_000_000_000).unwrap();
    assert_eq!(e.timestamp, 1_700_000_000_000);
    assert!(matches!(e.event_type, EventType::SystemIdle));
}

#[test]
fn input_active_signal_becomes_system_resume_event() {
    let e = signal_to_event(RawSignal::InputActive, 1_700_000_000_001).unwrap();
    assert!(matches!(e.event_type, EventType::SystemResume));
}

#[test]
fn heartbeat_signal_preserves_active_seconds() {
    for secs in [0u8, 1, 7, 10] {
        let e = signal_to_event(RawSignal::Heartbeat(secs), 1_700_000_000_000).unwrap();
        match e.event_type {
            EventType::InputHeartbeat(ref p) => {
                assert_eq!(p.active_seconds, secs);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }
}

#[test]
fn every_signal_preserves_the_supplied_timestamp() {
    for sig in [RawSignal::IdleStart, RawSignal::InputActive, RawSignal::Heartbeat(3)] {
        let e = signal_to_event(sig, 1_234_567_890_123).unwrap();
        assert_eq!(e.timestamp, 1_234_567_890_123);
    }
}

#[test]
fn event_ids_are_distinct_for_rapid_signals() {
    let a = signal_to_event(RawSignal::IdleStart, 1_700_000_000_000).unwrap();
    let b = signal_to_event(RawSignal::InputActive, 1_700_000_000_000).unwrap();
    assert_ne!(a.id, b.id, "同毫秒内的两个事件不能撞 id");
}

// --- 脱敏（spec §11）---

// --- 脱敏（spec §11）---

/// 脱敏器由 app 层构造后传进来，collector 不认识 rules、也不依赖 regex crate
/// （spec §4：collector 只做 OS 交互）。这里用闭包代替真实 Redactor。
mod redaction {
    use super::redact_event;
    use activity_core::{
        Event, EventType, WindowFocusPayload, WindowTitleChangePayload,
    };

    fn fake_redactor(t: &str) -> String {
        t.replace("SECRET", "[redacted]")
    }

    #[test]
    fn the_redacted_title_is_what_reaches_storage() {
        // 真正要保证的是"落库的东西已经脱敏"，而不只是"某个函数返回了脱敏结果"
        let conn = activity_storage::open_in_memory_shared();
        let writer = std::sync::Arc::new(activity_storage::BatchWriter::new(
            std::sync::Arc::clone(&conn),
            600_000,
            1000,
        ));
        let (_tx, rx) = std::sync::mpsc::channel::<activity_collector::signals::RawSignal>();
        activity_collector::consumer::spawn_consumer_with(
            rx,
            std::sync::Arc::clone(&writer),
            None,
            Some(std::sync::Arc::new(fake_redactor)),
        );

        let raw = Event::new(
            EventType::WindowFocus(WindowFocusPayload {
                process_name: "slack.exe".into(),
                window_title: Some("SECRET - #general".into()),
                exe_path: None,
            }),
            1_700_000_000_000,
        );
        writer.push(redact_event(&raw, &fake_redactor));
        writer.flush().unwrap();

        let rows =
            activity_storage::get_events_in_range(&conn.lock().unwrap(), 0, i64::MAX).unwrap();
        assert_eq!(rows.len(), 1);
        let stored: EventType =
            serde_json::from_str(&rows[0].payload).expect("payload 应是合法 JSON");
        match stored {
            EventType::WindowFocus(p) => {
                let t = p.window_title.as_deref().unwrap_or("");
                assert!(t.contains("[redacted]"), "落库的标题应已脱敏：{t:?}");
                assert!(!t.contains("SECRET"), "落库的标题不该含明文：{t:?}");
                assert_eq!(p.process_name, "slack.exe", "进程名不该被动");
            }
            other => panic!("variant 错误: {other:?}"),
        }
    }

    #[test]
    fn redacting_an_event_leaves_other_fields_intact() {
        let raw = Event::new(
            EventType::WindowTitleChange(WindowTitleChangePayload {
                process_name: "a.exe".into(),
                window_title: Some("SECRET".into()),
            }),
            1234,
        );
        let out = redact_event(&raw, &fake_redactor);
        assert_eq!(out.id, raw.id);
        assert_eq!(out.timestamp, 1234);
        match out.event_type {
            EventType::WindowTitleChange(p) => {
                assert_eq!(p.process_name, "a.exe");
                assert_eq!(p.window_title.as_deref(), Some("[redacted]"));
            }
            other => panic!("variant 错误: {other:?}"),
        }
    }

    #[test]
    fn a_null_title_survives_redaction() {
        // Review Focus #4：无标题的窗口不能让脱敏炸掉
        let raw = Event::new(
            EventType::WindowFocus(WindowFocusPayload {
                process_name: "a.exe".into(),
                window_title: None,
                exe_path: None,
            }),
            1,
        );
        let out = redact_event(&raw, &fake_redactor);
        match out.event_type {
            EventType::WindowFocus(p) => assert_eq!(p.window_title, None),
            other => panic!("variant 错误: {other:?}"),
        }
    }

    #[test]
    fn events_without_a_title_are_untouched() {
        let raw = Event::new(EventType::SystemIdle, 999);
        assert_eq!(redact_event(&raw, &fake_redactor), raw);
    }

    #[test]
    fn an_event_with_a_non_matching_title_is_unchanged() {
        let raw = Event::new(
            EventType::WindowFocus(WindowFocusPayload {
                process_name: "a.exe".into(),
                window_title: Some("public stuff".into()),
                exe_path: None,
            }),
            7,
        );
        assert_eq!(redact_event(&raw, &fake_redactor), raw);
    }
}
