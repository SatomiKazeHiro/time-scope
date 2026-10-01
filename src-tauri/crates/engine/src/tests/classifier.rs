use crate::{Classification, RuleSet};

const SAMPLE: &str = r#"
[[rule]]
id = "vscode"
process = ["Code.exe"]
category = "work"
confidence = 0.9

[[rule]]
id = "browser"
process = ["msedge.exe", "chrome.exe"]
category = "browsing"
confidence = 0.5

[[rule]]
id = "wechat"
process = ["WeChat.exe"]
category = "communication"
confidence = 0.85
"#;

#[test]
fn matches_first_rule_in_declaration_order() {
    let rs = RuleSet::from_toml(SAMPLE).unwrap();
    assert_eq!(rs.classify(Some("Code.exe")).rule_id.as_deref(), Some("vscode"));
    assert_eq!(rs.classify(Some("chrome.exe")).rule_id.as_deref(), Some("browser"));
    assert_eq!(rs.classify(Some("WeChat.exe")).rule_id.as_deref(), Some("wechat"));
}

#[test]
fn returns_rule_category_and_confidence() {
    let rs = RuleSet::from_toml(SAMPLE).unwrap();
    let c = rs.classify(Some("Code.exe"));
    assert_eq!(c.category.as_str(), "work");
    assert!((c.confidence - 0.9).abs() < 1e-6);
}

#[test]
fn process_match_is_case_insensitive() {
    // Review Focus #3：用户手写规则不该因为大小写失配
    let rs = RuleSet::from_toml(SAMPLE).unwrap();
    for name in ["code.exe", "CODE.EXE", "CoDe.ExE"] {
        assert_eq!(
            rs.classify(Some(name)).rule_id.as_deref(),
            Some("vscode"),
            "{name} 应匹配 vscode"
        );
    }
}

#[test]
fn matches_on_file_name_not_full_path() {
    let rs = RuleSet::from_toml(SAMPLE).unwrap();
    let c = rs.classify(Some("C:\\Program Files\\Microsoft VS Code\\Code.exe"));
    assert_eq!(c.rule_id.as_deref(), Some("vscode"));
}

#[test]
fn unknown_process_falls_back_to_unknown_with_zero_confidence() {
    let rs = RuleSet::from_toml(SAMPLE).unwrap();
    let c = rs.classify(Some("SomeRandomGame.exe"));
    assert_eq!(c.category.as_str(), "unknown");
    assert_eq!(c.confidence, 0.0);
    assert!(c.rule_id.is_none());
}

#[test]
fn missing_process_name_is_unknown() {
    let rs = RuleSet::from_toml(SAMPLE).unwrap();
    assert_eq!(rs.classify(None).category.as_str(), "unknown");
    assert_eq!(rs.classify(Some("")).category.as_str(), "unknown");
}

#[test]
fn empty_rule_set_classifies_everything_as_unknown() {
    let rs = RuleSet::from_toml("").unwrap();
    assert_eq!(rs.classify(Some("Code.exe")).category.as_str(), "unknown");
}

#[test]
fn category_is_case_insensitive_and_trimmed() {
    let rs = RuleSet::from_toml(
        r#"
[[rule]]
id = "x"
process = ["a.exe"]
category = "  Work  "
confidence = 0.5
"#,
    )
    .unwrap();
    assert_eq!(rs.classify(Some("a.exe")).category.as_str(), "work");
}

#[test]
fn invalid_category_string_is_an_error_not_a_silent_unknown() {
    // 规则写错类别必须报错，否则用户以为规则生效了其实全落到 unknown
    let err = RuleSet::from_toml(
        r#"
[[rule]]
id = "x"
process = ["a.exe"]
category = "not_a_category"
confidence = 0.5
"#,
    );
    assert!(err.is_err(), "非法 category 应报错");
}

#[test]
fn malformed_toml_is_an_error() {
    assert!(RuleSet::from_toml("[[[not toml").is_err());
}

#[test]
fn rules_version_is_recorded_for_reproducibility() {
    let rs = RuleSet::from_toml(SAMPLE).unwrap();
    assert!(!rs.version.is_empty(), "规则集必须有版本号，spec §7.2");
}

#[test]
fn rule_with_no_process_matches_nothing() {
    // 空 process 列表不该变成"匹配所有程序"
    let rs = RuleSet::from_toml(
        r#"
[[rule]]
id = "empty"
process = []
category = "work"
confidence = 0.9
"#,
    )
    .unwrap();
    assert_eq!(rs.classify(Some("anything.exe")).category.as_str(), "unknown");
}

#[test]
fn confidence_out_of_range_is_an_error() {
    let err = RuleSet::from_toml(
        r#"
[[rule]]
id = "x"
process = ["a.exe"]
category = "work"
confidence = 1.5
"#,
    );
    assert!(err.is_err(), "confidence 越界应报错");
}

#[test]
fn default_rules_toml_is_valid() {
    let rs = RuleSet::from_toml(crate::DEFAULT_RULES_TOML).expect("内置默认规则必须能解析");
    assert!(!rs.rules.is_empty());
    assert_eq!(rs.classify(Some("Code.exe")).category.as_str(), "work");
    assert_eq!(rs.classify(Some("msedge.exe")).category.as_str(), "browsing");
}

#[test]
fn classification_defaults_to_unknown() {
    let c = Classification::unknown();
    assert_eq!(c.category.as_str(), "unknown");
    assert_eq!(c.confidence, 0.0);
    assert!(c.rule_id.is_none());
}
