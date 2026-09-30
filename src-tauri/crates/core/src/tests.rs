use crate::{Event, EventType, InputHeartbeatPayload, WindowFocusPayload, WindowTitleChangePayload};

#[test]
fn window_focus_event_roundtrips_json() {
    let e = Event::new(
        EventType::WindowFocus(WindowFocusPayload {
            process_name: "Code.exe".into(),
            window_title: Some("main.rs - time-scope".into()),
            exe_path: Some("C:\\dev\\Code.exe".into()),
        }),
        1_700_000_000_000,
    );
    let s = serde_json::to_string(&e).unwrap();
    let back: Event = serde_json::from_str(&s).unwrap();
    assert_eq!(back.id, e.id);
    assert_eq!(back.timestamp, 1_700_000_000_000);
    match back.event_type {
        EventType::WindowFocus(ref p) => {
            assert_eq!(p.process_name, "Code.exe");
            assert_eq!(p.window_title.as_deref(), Some("main.rs - time-scope"));
            assert_eq!(p.exe_path.as_deref(), Some("C:\\dev\\Code.exe"));
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn event_type_serializes_with_snake_case_tag() {
    let e = Event::new(EventType::SystemIdle, 1_700_000_002_000);
    let s = serde_json::to_string(&e).unwrap();
    assert!(s.contains("\"type\":\"system_idle\""), "unexpected json: {s}");
    assert_eq!(e.event_type.type_tag(), "system_idle");
}

#[test]
fn window_title_change_with_null_title_roundtrips() {
    // Review Focus #4: 部分系统窗口/UWP 应用没有标题，null 不得导致反序列化失败
    let e = Event::new(
        EventType::WindowTitleChange(WindowTitleChangePayload {
            process_name: "explorer.exe".into(),
            window_title: None,
        }),
        1_700_000_001_000,
    );
    let s = serde_json::to_string(&e).unwrap();
    let back: Event = serde_json::from_str(&s).unwrap();
    match back.event_type {
        EventType::WindowTitleChange(ref p) => {
            assert!(p.window_title.is_none());
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn unit_events_roundtrip() {
    for et in [
        EventType::SystemIdle,
        EventType::SystemResume,
        EventType::SessionLock,
        EventType::SessionUnlock,
    ] {
        let tag = et.type_tag();
        let e = Event::new(et, 1_700_000_002_000);
        let s = serde_json::to_string(&e).unwrap();
        let back: Event = serde_json::from_str(&s).unwrap();
        assert_eq!(back.timestamp, 1_700_000_002_000);
        assert_eq!(back.event_type.type_tag(), tag);
    }
}

#[test]
fn input_heartbeat_roundtrips() {
    let e = Event::new(
        EventType::InputHeartbeat(InputHeartbeatPayload { active_seconds: 7 }),
        1_700_000_003_000,
    );
    let s = serde_json::to_string(&e).unwrap();
    let back: Event = serde_json::from_str(&s).unwrap();
    match back.event_type {
        EventType::InputHeartbeat(ref p) => assert_eq!(p.active_seconds, 7),
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn ids_are_unique_even_within_same_millisecond() {
    let a = Event::new(EventType::SystemIdle, 1_700_000_004_000);
    let b = Event::new(EventType::SystemIdle, 1_700_000_004_000);
    assert_ne!(a.id, b.id);
    assert_eq!(a.id.len(), 32);
}
