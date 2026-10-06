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

// --- 脱敏规则（spec §11）---

#[test]
fn parses_redact_patterns_alongside_rules() {
    let rs = RuleSet::from_toml(
        r#"
[[rule]]
id = "x"
process = ["a.exe"]
category = "work"
confidence = 0.5

[[redact]]
pattern = "Alice"

# TOML 单引号是 literal string，不处理转义 —— 写正则必须用它
[[redact]]
pattern = '订单 \d+'
"#,
    )
    .unwrap();
    assert_eq!(rs.redact.len(), 2);
    assert!(rs.redact.iter().any(|p| p == "Alice"));
    assert!(rs.redact.iter().any(|p| p.contains("订单")));
}

#[test]
fn redact_list_is_optional_and_defaults_to_empty() {
    // 没有 [[redact]] 的旧 rules.toml 必须仍能解析
    let rs = RuleSet::from_toml(
        r#"
[[rule]]
id = "x"
process = ["a.exe"]
category = "work"
confidence = 0.5
"#,
    )
    .unwrap();
    assert!(rs.redact.is_empty());
}

#[test]
fn rules_file_with_only_redact_entries_parses() {
    let rs = RuleSet::from_toml("[[redact]]\npattern = \"secret\"\n").unwrap();
    assert!(rs.rules.is_empty());
    assert_eq!(rs.redact.len(), 1);
}

#[test]
fn an_invalid_redact_regex_does_not_break_the_whole_file() {
    // 脱敏规则写坏不该让分类规则一起失效——两者是独立的关注点
    let rs = RuleSet::from_toml(
        r#"
[[rule]]
id = "x"
process = ["a.exe"]
category = "work"
confidence = 0.5

[[redact]]
pattern = "([unclosed"
"#,
    )
    .unwrap();
    assert_eq!(rs.rules.len(), 1, "分类规则应照常加载");
    assert_eq!(rs.redact.len(), 1, "脱敏模式原样保留，由上层决定是否跳过");
}

#[test]
fn default_rules_toml_declares_the_redact_key() {
    // 默认规则里要有 [[redact]] 的说明，否则用户不知道这个键存在
    let rs = RuleSet::from_toml(crate::DEFAULT_RULES_TOML).unwrap();
    assert!(rs.redact.is_empty(), "默认不预置任何脱敏规则，避免过度脱敏");
}

#[test]
fn redact_pattern_version_is_part_of_the_ruleset() {
    // 改了脱敏规则，重算结果的 classifier_version 应该能区分开
    let a = RuleSet::from_toml("[[redact]]\npattern = \"x\"\n").unwrap();
    let b = RuleSet::from_toml("").unwrap();
    assert_ne!(a.version, b.version);
}

#[test]
fn redact_patterns_need_toml_literal_strings_for_backslashes() {
    // TOML 的双引号串会处理转义，正则里的 \d 会被当成非法转义而解析失败。
    // 写正则必须用单引号的 literal string。这是个用户一定会踩的坑。
    assert!(
        RuleSet::from_toml(r#"[[redact]]
pattern = "\d+"
"#)
        .is_err(),
        "双引号里的裸 \\d 应解析失败"
    );
    let ok = RuleSet::from_toml("[[redact]]\npattern = '\\d+'\n");
    assert!(ok.is_ok(), "单引号里的 \\d 应解析成功，实际 {:?}", ok.err());
    assert_eq!(ok.unwrap().redact, vec!["\\d+".to_string()]);
}

// --- B2：版本号必须由规则**内容**派生，而不是由条数 ---

#[test]
fn changing_a_rules_category_changes_the_version() {
    // B2。原来版本号是 `format!("rules:{}+redact:{}", rules.len(), redact.len())`
    // —— 只由**条数**派生。把某条规则的 category 从 work 改成 study，
    // 条数不变、版本号一模一样，于是重算出来的段看起来和旧段同源，
    // 注释里"改了规则就能被区分开"这句话不成立。
    let a = RuleSet::from_toml(
        "[[rule]]\nid = \"a\"\nprocess = [\"x.exe\"]\ncategory = \"work\"\nconfidence = 1.0\n",
    )
    .unwrap();
    let b = RuleSet::from_toml(
        "[[rule]]\nid = \"a\"\nprocess = [\"x.exe\"]\ncategory = \"study\"\nconfidence = 1.0\n",
    )
    .unwrap();
    assert_eq!(a.rules.len(), b.rules.len(), "两条规则的条数必须一样");
    assert_ne!(
        a.version, b.version,
        "只改 category 也要换版本号，否则重算出来的段无法与旧段区分"
    );
}

#[test]
fn changing_a_rules_process_or_confidence_changes_the_version() {
    let base = "[[rule]]\nid = \"a\"\nprocess = [\"x.exe\"]\ncategory = \"work\"\nconfidence = 1.0\n";
    let orig = RuleSet::from_toml(base).unwrap();
    for (what, changed) in [
        ("process", "[[rule]]\nid = \"a\"\nprocess = [\"y.exe\"]\ncategory = \"work\"\nconfidence = 1.0\n"),
        ("confidence", "[[rule]]\nid = \"a\"\nprocess = [\"x.exe\"]\ncategory = \"work\"\nconfidence = 0.5\n"),
        ("id", "[[rule]]\nid = \"z\"\nprocess = [\"x.exe\"]\ncategory = \"work\"\nconfidence = 1.0\n"),
    ] {
        let other = RuleSet::from_toml(changed).unwrap();
        assert_ne!(orig.version, other.version, "改了 {what} 也要换版本号");
    }
}

#[test]
fn changing_a_redact_patterns_text_changes_the_version() {
    // 与 `redact_pattern_version_is_part_of_the_ruleset` 互补：
    // 那条比的是"有/没有"，这条比的是"内容不同但条数相同"。
    let a = RuleSet::from_toml("[[redact]]\npattern = '客户\\d+'\n").unwrap();
    let b = RuleSet::from_toml("[[redact]]\npattern = '(?i)salary'\n").unwrap();
    assert_eq!(a.redact.len(), b.redact.len(), "脱敏规则条数必须一样");
    assert_ne!(a.version, b.version, "只改正则文本也要换版本号");
}

#[test]
fn the_version_is_deterministic_across_runs() {
    // 版本号会**落库**（activities.classifier_version），所以同一份规则
    // 必须永远得到同一个值。不能用 HashMap 的迭代顺序或随机种子。
    let first = RuleSet::from_toml(SAMPLE).unwrap().version;
    for _ in 0..5 {
        assert_eq!(RuleSet::from_toml(SAMPLE).unwrap().version, first);
    }
}

#[test]
fn reordering_rules_changes_the_version() {
    // 顺序变了就该换版本号：命中顺序决定最终分类（先匹配先赢）。
    let a = RuleSet::from_toml(
        "[[rule]]\nid = \"a\"\nprocess = [\"x.exe\"]\ncategory = \"work\"\nconfidence = 1.0\n\
         [[rule]]\nid = \"b\"\nprocess = [\"x.exe\"]\ncategory = \"study\"\nconfidence = 1.0\n",
    )
    .unwrap();
    let b = RuleSet::from_toml(
        "[[rule]]\nid = \"b\"\nprocess = [\"x.exe\"]\ncategory = \"study\"\nconfidence = 1.0\n\
         [[rule]]\nid = \"a\"\nprocess = [\"x.exe\"]\ncategory = \"work\"\nconfidence = 1.0\n",
    )
    .unwrap();
    assert_ne!(
        a.classify(Some("x.exe")).category,
        b.classify(Some("x.exe")).category,
        "两条规则都匹配 x.exe，顺序决定谁赢"
    );
    assert_ne!(a.version, b.version, "命中顺序变了也要换版本号");
}
