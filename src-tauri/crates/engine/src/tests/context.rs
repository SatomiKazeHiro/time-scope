use crate::{context_of, ContextBuilder};
use activity_core::{Event, EventType, InputHeartbeatPayload, WindowFocusPayload,
                    WindowTitleChangePayload};

fn focus(app: &str, title: &str) -> Event {
    Event::new(
        EventType::WindowFocus(WindowFocusPayload {
            process_name: app.into(),
            window_title: Some(title.into()),
            exe_path: None,
        }),
        1000,
    )
}

fn title(app: &str, title: &str) -> Event {
    Event::new(
        EventType::WindowTitleChange(WindowTitleChangePayload {
            process_name: app.into(),
            window_title: Some(title.into()),
        }),
        2000,
    )
}

fn heartbeat(secs: u8) -> Event {
    Event::new(
        EventType::InputHeartbeat(InputHeartbeatPayload { active_seconds: secs }),
        3000,
    )
}

#[test]
fn window_focus_sets_application_and_title() {
    let e = focus("Code.exe", "main.rs");
    let c = context_of(&e, false, true);
    assert_eq!(c.application.as_deref(), Some("Code.exe"));
    assert_eq!(c.window_title.as_deref(), Some("main.rs"));
    assert!(!c.is_idle);
    assert!(c.input_active);
}

#[test]
fn title_change_updates_application_and_title() {
    let mut c = context_of(&focus("Code.exe", "a.rs"), false, true);
    ContextBuilder::apply(&mut c, &title("chrome.exe", "New Tab"));
    assert_eq!(c.application.as_deref(), Some("chrome.exe"));
    assert_eq!(c.window_title.as_deref(), Some("New Tab"));
}

#[test]
fn null_title_does_not_erase_previous_title() {
    // 切到无标题窗口时不该把上一个窗口的标题带过来
    let mut c = context_of(&focus("Code.exe", "main.rs"), false, true);
    let e = Event::new(
        EventType::WindowTitleChange(WindowTitleChangePayload {
            process_name: "explorer.exe".into(),
            window_title: None,
        }),
        2000,
    );
    ContextBuilder::apply(&mut c, &e);
    assert_eq!(c.application.as_deref(), Some("explorer.exe"));
    assert_eq!(c.window_title, None);
}

#[test]
fn heartbeat_updates_input_active() {
    let mut c = context_of(&focus("Code.exe", "x"), false, false);
    ContextBuilder::apply(&mut c, &heartbeat(0));
    assert!(!c.input_active, "active_seconds=0 表示这窗口没输入");
    ContextBuilder::apply(&mut c, &heartbeat(3));
    assert!(c.input_active);
}

#[test]
fn idle_flag_comes_from_caller_not_from_event() {
    // SystemIdle/Resume 的状态翻转是 reduce 的职责，ContextBuilder 只接收结果
    let e = Event::new(EventType::SystemIdle, 5000);
    assert!(context_of(&e, true, false).is_idle);
    assert!(!context_of(&e, false, false).is_idle);
}

#[test]
fn session_events_leave_context_untouched() {
    // 意图是"这些事件不会改动已有 context"，所以要 apply 到一份已有上下文的副本上，
    // 而不是用 context_of（它总是从空 context 起算）。
    let base = context_of(&focus("Code.exe", "x"), false, true);
    for ev in [
        Event::new(EventType::SessionLock, 9000),
        Event::new(EventType::SessionUnlock, 9000),
    ] {
        let mut c = base.clone();
        ContextBuilder::apply(&mut c, &ev);
        assert_eq!(c, base, "{:?} 不应改动 context", ev.event_type);
    }
}

#[test]
fn builder_is_idempotent_for_the_same_event() {
    let mut a = context_of(&focus("Code.exe", "x"), false, true);
    let mut b = a.clone();
    let e = focus("Code.exe", "x");
    ContextBuilder::apply(&mut a, &e);
    ContextBuilder::apply(&mut b, &e);
    assert_eq!(a, b);
}
