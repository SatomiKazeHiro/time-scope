use crate::{ActivityContext, ActivitySegment, Category, CLASSIFIER_RULE};

fn seg(category: Category, start: i64, end: i64) -> ActivitySegment {
    ActivitySegment {
        id: format!("s{start}"),
        start_at: start,
        end_at: end,
        category,
        application: Some("Code.exe".into()),
        confidence: 0.9,
        classifier: CLASSIFIER_RULE.into(),
        classifier_version: "test-v1".into(),
        evidence_event_ids: vec![],
    }
}

#[test]
fn category_roundtrips_through_str() {
    for c in Category::all() {
        assert_eq!(Category::from_str(c.as_str()), Some(c), "{c:?}");
    }
}

#[test]
fn category_as_str_is_snake_case() {
    assert_eq!(Category::Work.as_str(), "work");
    assert_eq!(Category::Entertainment.as_str(), "entertainment");
    assert_eq!(Category::Unknown.as_str(), "unknown");
    assert_eq!(Category::Idle.as_str(), "idle");
}

#[test]
fn category_from_str_rejects_unknown() {
    assert_eq!(Category::from_str("nope"), None);
    assert_eq!(Category::from_str(""), None);
    // 大小写不敏感：用户手写 rules.toml 时不该因为大小写就失配
    assert_eq!(Category::from_str("WORK"), Some(Category::Work));
    assert_eq!(Category::from_str("Work"), Some(Category::Work));
}

#[test]
fn category_has_all_eight_variants() {
    assert_eq!(Category::all().len(), 8);
}

#[test]
fn context_equality_uses_app_and_idle_only() {
    let a = ActivityContext {
        application: Some("Code.exe".into()),
        window_title: Some("main.rs".into()),
        input_active: true,
        is_idle: false,
    };
    let mut b = a.clone();
    b.window_title = Some("lib.rs".into()); // 标题变了
    assert!(a.same_app_and_category(&b), "标题变化不切段");

    b.application = Some("chrome.exe".into());
    assert!(!a.same_app_and_category(&b), "应用变化应切段");
}

#[test]
fn context_idle_flag_always_diffs() {
    let mut a = ActivityContext {
        application: Some("Code.exe".into()),
        window_title: None,
        input_active: true,
        is_idle: false,
    };
    let b = a.clone();
    a.is_idle = true;
    assert!(!a.same_app_and_category(&b), "idle 状态不同必然切段");
}

#[test]
fn segment_helpers() {
    let s = seg(Category::Work, 1000, 61_000);
    assert_eq!(s.duration_ms(), 60_000);
    assert!(!s.is_empty());
    let zero = seg(Category::Work, 1000, 1000);
    assert!(zero.is_empty());
    assert_eq!(zero.duration_ms(), 0);
}
