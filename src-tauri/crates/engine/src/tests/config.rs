use crate::EngineConfig;

#[test]
fn defaults_match_spec() {
    let c = EngineConfig::default();
    assert_eq!(c.idle_threshold_s, 300);
    assert_eq!(c.grace_period_s, 60);
    assert_eq!(c.min_segment_duration_s, 30);
}

#[test]
fn builder_overrides_each_field() {
    let c = EngineConfig::default()
        .with_idle_threshold_s(10)
        .with_grace_period_s(0)
        .with_min_segment_duration_s(0);
    assert_eq!(c.idle_threshold_s, 10);
    assert_eq!(c.grace_period_s, 0);
    assert_eq!(c.min_segment_duration_s, 0);
}

#[test]
fn config_is_copy_and_comparable() {
    let a = EngineConfig::default();
    let b = a;
    assert_eq!(a, b);
}
