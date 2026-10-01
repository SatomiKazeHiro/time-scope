use regex::Regex;
use std::borrow::Cow;

/// 脱敏占位符。写死在代码里，用户不该能改——它本身就是"这里原来有内容"的信号。
pub const PLACEHOLDER: &str = "[redacted]";

/// 窗口标题脱敏器（spec §11）。
///
/// 持有**启动时编译好**的正则：窗口标题变化很频繁（实测 100 秒能来 100+ 次），
/// 每条都重新编译是不可接受的。
#[derive(Debug, Default)]
pub struct Redactor {
    regexes: Vec<Regex>,
    /// 编译失败被跳过的模式。只用于诊断与提示用户。
    skipped: Vec<String>,
}

impl Redactor {
    /// 从 `rules.toml` 的 `[[redact]]` 原始模式构建。
    ///
    /// **编译失败的模式被跳过而不是让整个规则文件失效**：脱敏和分类是独立的
    /// 关注点，一条正则写错不该让用户的分类规则也一起报废。
    pub fn new(patterns: &[String]) -> Self {
        let mut regexes = Vec::new();
        let mut skipped = Vec::new();
        for p in patterns {
            match Regex::new(p) {
                Ok(r) => regexes.push(r),
                Err(e) => {
                    eprintln!("[time-scope] 脱敏规则 {p:?} 不是合法正则，已跳过: {e}");
                    skipped.push(p.clone());
                }
            }
        }
        Self { regexes, skipped }
    }

    /// 没有任何脱敏规则时，直接返回原字符串（零分配）。
    pub fn is_empty(&self) -> bool {
        self.regexes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.regexes.len()
    }

    /// 因编译失败被跳过的模式
    pub fn skipped(&self) -> &[String] {
        &self.skipped
    }

    /// 脱敏一条标题：命中任一模式的部分替换为 `[redacted]`，其余保留。
    pub fn redact<'a>(&self, title: &'a str) -> Cow<'a, str> {
        if self.regexes.is_empty() || title.is_empty() {
            return Cow::Borrowed(title);
        }
        let mut out: Option<String> = None;
        for re in &self.regexes {
            if !re.is_match(title) {
                continue;
            }
            // 用正则在原串上替换，而不是反复累积到上一次的结果，
            // 免得第二条规则的匹配作用在第一条的占位符上。
            let replaced = re.replace_all(title, PLACEHOLDER);
            out = Some(match out {
                None => replaced.into_owned(),
                Some(prev) => re.replace_all(&prev, PLACEHOLDER).into_owned(),
            });
        }
        match out {
            Some(s) => Cow::Owned(s),
            None => Cow::Borrowed(title),
        }
    }

    /// 脱敏一段 payload JSON 里所有 `window_title` 字段的值。
    ///
    /// 用于**历史数据**：本功能上线前已经落库的标题不会被回溯重写，
    /// 但重放时经过这里，界面上就不会再露出旧数据里的敏感片段。
    pub fn redact_payload(&self, payload: &str) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut value: serde_json::Value = serde_json::from_str(payload).ok()?;
        let mut changed = false;
        redact_value(&mut value, self, &mut changed);
        if changed {
            serde_json::to_string(&value).ok()
        } else {
            None
        }
    }
}

fn redact_value(value: &mut serde_json::Value, r: &Redactor, changed: &mut bool) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if k == "window_title" {
                    if let serde_json::Value::String(s) = v {
                        let redacted = r.redact(s);
                        if matches!(redacted, Cow::Owned(_)) {
                            *v = serde_json::Value::String(redacted.into_owned());
                            *changed = true;
                        }
                    }
                } else {
                    redact_value(v, r, changed);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for v in items {
                redact_value(v, r, changed);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redactor(patterns: &[&str]) -> Redactor {
        Redactor::new(&patterns.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn no_patterns_leaves_the_title_untouched() {
        let r = Redactor::default();
        assert!(r.is_empty());
        assert_eq!(r.redact("Alice - Slack"), "Alice - Slack");
    }

    #[test]
    fn a_match_is_replaced_but_the_rest_is_kept() {
        let r = redactor(&["Alice"]);
        assert_eq!(r.redact("Alice - Slack"), "[redacted] - Slack");
        assert_eq!(r.redact("Bob - Slack"), "Bob - Slack");
    }

    #[test]
    fn every_occurrence_is_replaced() {
        let r = redactor(&["secret"]);
        assert_eq!(
            r.redact("secret and secret again"),
            "[redacted] and [redacted] again"
        );
    }

    #[test]
    fn several_patterns_apply_in_sequence() {
        let r = redactor(&["Alice", "订单 12345"]);
        let out = r.redact("Alice - 订单 12345 已发货");
        assert_eq!(out, "[redacted] - [redacted] 已发货");
    }

    #[test]
    fn a_pattern_can_match_the_whole_title() {
        let r = redactor(&[".*"]);
        assert_eq!(r.redact("完全命中"), "[redacted]");
    }

    #[test]
    fn an_invalid_regex_is_skipped_without_losing_the_others() {
        // 一条写错不该让其余脱敏规则也失效
        let r = redactor(&["([unclosed", "Alice"]);
        assert_eq!(r.len(), 1);
        assert_eq!(r.skipped().len(), 1);
        assert_eq!(r.redact("Alice here"), "[redacted] here");
    }

    #[test]
    fn an_all_invalid_set_behaves_as_empty() {
        let r = redactor(&["([unclosed", "[bad"]);
        assert!(r.is_empty());
        assert_eq!(r.redact("Alice"), "Alice");
    }

    #[test]
    fn case_sensitivity_follows_the_pattern() {
        // 用户可以用 (?i) 自己要求不敏感
        let sensitive = redactor(&["secret"]);
        assert_eq!(sensitive.redact("Secret"), "Secret");
        let insensitive = redactor(&["(?i)secret"]);
        assert_eq!(insensitive.redact("Secret"), "[redacted]");
    }

    #[test]
    fn an_empty_title_stays_empty() {
        let r = redactor(&["Alice"]);
        assert_eq!(r.redact(""), "");
    }

    #[test]
    fn numeric_patterns_work() {
        let r = redactor(&[r"\d{4,}", "订单"]);
        assert_eq!(r.redact("订单 12345 已发货"), "[redacted] [redacted] 已发货");
    }

    #[test]
    fn payload_window_title_is_redacted() {
        let r = redactor(&["Alice"]);
        let payload = r#"{"type":"window_focus","process_name":"slack.exe","window_title":"Alice - #general"}"#;
        let out = r.redact_payload(payload).expect("应发生变化");
        assert!(!out.contains("Alice"), "实际={}", out);
        assert!(out.contains("[redacted]"));
        assert!(out.contains("slack.exe"), "进程名不该被动");
    }

    #[test]
    fn payload_without_a_title_match_is_returned_as_none() {
        // 没变化就不重写，避免无谓的字符串分配与内容变动
        let r = redactor(&["Alice"]);
        let payload = r#"{"type":"window_focus","window_title":"Bob"}"#;
        assert!(r.redact_payload(payload).is_none());
    }

    #[test]
    fn payload_with_a_null_title_is_left_alone() {
        // Review Focus #4：null 标题不能让脱敏炸掉
        let r = redactor(&["Alice"]);
        let payload = r#"{"type":"window_title_change","window_title":null}"#;
        assert!(r.redact_payload(payload).is_none());
    }

    #[test]
    fn malformed_payload_is_left_alone_instead_of_panicking() {
        let r = redactor(&["Alice"]);
        assert!(r.redact_payload("{not json").is_none());
    }

    #[test]
    fn nested_window_title_is_redacted() {
        // 变体里带嵌套结构的 payload
        let r = redactor(&["Alice"]);
        let payload = r#"{"type":"window_focus","extra":{"window_title":"Alice deep"}}"#;
        let out = r.redact_payload(payload).unwrap();
        assert!(!out.contains("Alice"), "实际={}", out);
    }

    #[test]
    fn an_empty_redactor_never_rewrites_a_payload() {
        let r = Redactor::default();
        let payload = r#"{"window_title":"Alice"}"#;
        assert!(r.redact_payload(payload).is_none());
    }
}

#[cfg(test)]
mod end_to_end_tests {
    use super::*;
    use activity_collector::consumer::{redact_event, TitleRedactor};
    use activity_core::{Event, EventType, WindowFocusPayload};
    use activity_storage::{get_events_in_range, open_in_memory_shared, BatchWriter};
    use std::sync::mpsc::channel;
    use std::sync::Arc;

    /// 从"用户写的 rules.toml 文本"一路走到"落库的 payload"，
    /// 全程用真实组件（RuleSet -> Redactor -> consumer -> BatchWriter），
    /// 不注入假的脱敏函数。
    #[test]
    fn toml_text_to_stored_payload_is_redacted() {
        let toml_src = r#"
[[rule]]
id = "slack"
process = ["slack.exe"]
category = "communication"
confidence = 0.8

[[redact]]
pattern = '客户\d+'

[[redact]]
pattern = '(?i)salary'
"#;
        let rules = activity_engine::RuleSet::from_toml(toml_src).unwrap();
        assert_eq!(rules.redact.len(), 2);

        let redactor = Arc::new(Redactor::new(&rules.redact));
        assert_eq!(redactor.len(), 2);
        assert!(redactor.skipped().is_empty());

        let conn = open_in_memory_shared();
        let writer = Arc::new(BatchWriter::new(Arc::clone(&conn), 600_000, 1000));
        let (_tx, rx) = channel::<activity_collector::signals::RawSignal>();
        let injected: TitleRedactor = {
            let r = Arc::clone(&redactor);
            Arc::new(move |t: &str| r.redact(t).into_owned())
        };
        activity_collector::consumer::spawn_consumer_with(rx, Arc::clone(&writer), None, Some(injected));

        let raw = Event::new(
            EventType::WindowFocus(WindowFocusPayload {
                process_name: "slack.exe".into(),
                window_title: Some("客户123456 — salary review".into()),
                exe_path: Some(r"C:\slack.exe".into()),
            }),
            1_700_000_000_000,
        );
        let redacted = redact_event(&raw, &|t: &str| redactor.redact(t).into_owned());
        writer.push(redacted);
        writer.flush().unwrap();

        let rows = get_events_in_range(&conn.lock().unwrap(), 0, i64::MAX).unwrap();
        assert_eq!(rows.len(), 1);
        let stored: EventType = serde_json::from_str(&rows[0].payload).unwrap();
        match stored {
            EventType::WindowFocus(p) => {
                let t = p.window_title.as_deref().unwrap_or("");
                assert!(!t.contains("123456"), "订单号泄漏：{t:?}");
                assert!(!t.to_lowercase().contains("salary"), "大小写不敏感规则应命中：{t:?}");
                assert_eq!(t.matches(PLACEHOLDER).count(), 2, "两处都该被替换：{t:?}");
                assert_eq!(p.process_name, "slack.exe", "进程名不动");
                assert!(p.exe_path.is_some(), "exe 路径不动");
            }
            other => panic!("variant 错误: {other:?}"),
        }
    }

    /// 附带的：分类仍然照常工作（脱敏不能把规则表搞坏）
    #[test]
    fn classification_still_works_alongside_redaction() {
        let toml_src = r#"
[[rule]]
id = "slack"
process = ["slack.exe"]
category = "communication"
confidence = 0.8

[[redact]]
pattern = 'secret'
"#;
        let rules = activity_engine::RuleSet::from_toml(toml_src).unwrap();
        let redactor = Redactor::new(&rules.redact);
        assert_eq!(rules.classify(Some("slack.exe")).category.as_str(), "communication");
        assert_eq!(redactor.redact("a secret thing"), "a [redacted] thing");
        assert!(rules.version.contains("redact:1"), "版本号应反映脱敏规则数：{}", rules.version);
    }
}
