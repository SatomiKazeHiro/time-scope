//! 事件总线（collector::consumer）的转换逻辑测试。
//!
//! 只测 `signal_to_event` 这个纯映射：RawSignal → Option<Event>。
//! 窗口类信号依赖真实 HWND，由 Task 7 的 smoke 测试覆盖。

use activity_collector::consumer::signal_to_event;
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
