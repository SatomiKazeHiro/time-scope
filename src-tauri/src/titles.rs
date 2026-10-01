//! 取某个 ActivitySegment 关联的窗口标题（spec §10 的详情展示）。
//!
//! 标题存在 `events.payload` 里，段本身不存——一个段可能对应几十个标题，
//! 塞进 activities 表既冗余又容易过时。这里按 evidence id 反查。
//!
//! **脱敏标记由后端判定**：前端不该硬编码 `[redacted]` 这个占位符，
//! 否则改了占位符就会出现"标记失效但数据仍脱敏"的诡异状态。

use crate::redact::PLACEHOLDER;
use activity_core::EventType;
use activity_storage::SharedConn;
use serde::Serialize;

/// 一条去重后的窗口标题。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SegmentTitle {
    pub title: String,
    /// 该标题是否经过脱敏（含有占位符）
    pub redacted: bool,
    /// 这个标题在证据里出现了多少次
    pub count: i64,
}

/// 按 evidence id 取去重后的标题，按首次出现顺序排列。
///
/// 传入的 id 里有些可能已经查不到（事件被删、id 拼错），那些会被静默跳过——
/// 一条缺标题不该让整个详情面板报错。
pub fn titles_for(conn: &SharedConn, event_ids: &[String]) -> Vec<SegmentTitle> {
    if event_ids.is_empty() {
        return Vec::new();
    }
    let Ok(c) = conn.lock() else {
        return Vec::new();
    };

    let mut stmt = match c.prepare(
        "SELECT payload FROM events WHERE id = ?1",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    // 保持首次出现顺序：用 Vec + 线性查找，标题数量级很小（几十）
    let mut out: Vec<SegmentTitle> = Vec::new();
    for id in event_ids {
        let Ok(rows) = stmt.query_map(rusqlite::params![id], |r| r.get::<_, String>(0)) else {
            continue;
        };
        for row in rows.flatten() {
            let Ok(event_type) = serde_json::from_str::<EventType>(&row) else {
                continue;
            };
            let raw_title = match &event_type {
                EventType::WindowFocus(p) => p.window_title.clone(),
                EventType::WindowTitleChange(p) => p.window_title.clone(),
                _ => None,
            };
            let Some(title) = raw_title.filter(|t| !t.trim().is_empty()) else {
                continue;
            };
            let redacted = title.contains(PLACEHOLDER);
            match out.iter_mut().find(|t| t.title == title) {
                Some(existing) => existing.count += 1,
                None => out.push(SegmentTitle {
                    title,
                    redacted,
                    count: 1,
                }),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use activity_core::{Event, EventType, WindowFocusPayload, WindowTitleChangePayload};
    use activity_storage::{insert_events, open_in_memory_shared};

    fn focus_event(id: &str, ts: i64, title: &str) -> Event {
        let mut e = Event::new(
            EventType::WindowFocus(WindowFocusPayload {
                process_name: "Code.exe".into(),
                window_title: Some(title.into()),
                exe_path: None,
            }),
            ts,
        );
        e.id = id.into();
        e
    }

    fn title_event(id: &str, ts: i64, title: &str) -> Event {
        let mut e = Event::new(
            EventType::WindowTitleChange(WindowTitleChangePayload {
                process_name: "Code.exe".into(),
                window_title: Some(title.into()),
            }),
            ts,
        );
        e.id = id.into();
        e
    }

    fn seed(conn: &SharedConn, events: Vec<Event>) -> Vec<String> {
        let ids: Vec<String> = events.iter().map(|e| e.id.clone()).collect();
        insert_events(&conn.lock().unwrap(), &events).unwrap();
        ids
    }

    #[test]
    fn collects_distinct_titles_with_counts() {
        let conn = open_in_memory_shared();
        let ids = seed(
            &conn,
            vec![
                focus_event("e1", 1, "main.rs - Code"),
                title_event("e2", 2, "main.rs - Code"),
                title_event("e3", 3, "lib.rs - Code"),
            ],
        );
        let out = titles_for(&conn, &ids);
        assert_eq!(out.len(), 2, "去重后应只剩 2 个标题");
        assert_eq!(out[0].title, "main.rs - Code");
        assert_eq!(out[0].count, 2, "同一标题出现两次应合并计数");
        assert_eq!(out[1].title, "lib.rs - Code");
        assert_eq!(out[1].count, 1);
    }

    #[test]
    fn preserves_first_appearance_order() {
        let conn = open_in_memory_shared();
        let ids = seed(
            &conn,
            vec![
                focus_event("e1", 1, "first"),
                focus_event("e2", 2, "second"),
                focus_event("e3", 3, "third"),
            ],
        );
        let out = titles_for(&conn, &ids);
        let order: Vec<&str> = out.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(order, vec!["first", "second", "third"]);
    }

    #[test]
    fn marks_redacted_titles() {
        let conn = open_in_memory_shared();
        let ids = seed(
            &conn,
            vec![
                focus_event("e1", 1, "registry.ts - [redacted] - Code"),
                focus_event("e2", 2, "plain.rs - Code"),
            ],
        );
        let out = titles_for(&conn, &ids);
        let r = out.iter().find(|t| t.title.contains("redacted")).unwrap();
        assert!(r.redacted, "含占位符的标题应被标记");
        let plain = out.iter().find(|t| !t.title.contains("redacted")).unwrap();
        assert!(!plain.redacted, "普通标题不该被标记");
    }

    #[test]
    fn redacted_flag_follows_the_placeholder_constant() {
        // 占位符改了，这里必须跟着变——前端因此不需要硬编码字符串
        let conn = open_in_memory_shared();
        let title = format!("a {} b", PLACEHOLDER);
        let ids = seed(&conn, vec![focus_event("e1", 1, &title)]);
        let out = titles_for(&conn, &ids);
        assert!(out[0].redacted);
    }

    #[test]
    fn empty_input_yields_nothing() {
        let conn = open_in_memory_shared();
        assert!(titles_for(&conn, &[]).is_empty());
    }

    #[test]
    fn unknown_event_ids_are_skipped_silently() {
        let conn = open_in_memory_shared();
        seed(&conn, vec![focus_event("e1", 1, "known")]);
        let out = titles_for(&conn, &["e1".to_string(), "nope".to_string()].to_vec());
        assert_eq!(out.len(), 1, "查不到的 id 不该让整体失败");
        assert_eq!(out[0].title, "known");
    }

    #[test]
    fn null_and_blank_titles_are_dropped() {
        // Review Focus #4：无标题的窗口不该在详情里显示空行
        let conn = open_in_memory_shared();
        let mut null_title = focus_event("e1", 1, "x");
        null_title.event_type = EventType::WindowFocus(WindowFocusPayload {
            process_name: "a.exe".into(),
            window_title: None,
            exe_path: None,
        });
        let mut blank = focus_event("e2", 2, "x");
        blank.event_type = EventType::WindowTitleChange(WindowTitleChangePayload {
            process_name: "a.exe".into(),
            window_title: Some("   ".into()),
        });
        let ids = seed(&conn, vec![focus_event("e3", 3, "real"), null_title, blank]);
        let out = titles_for(&conn, &ids);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "real");
    }

    #[test]
    fn events_without_a_title_are_ignored() {
        let conn = open_in_memory_shared();
        let idle = Event::new(EventType::SystemIdle, 1);
        let mut idle = idle;
        idle.id = "e1".into();
        let ids = seed(&conn, vec![idle, focus_event("e2", 2, "real")]);
        let out = titles_for(&conn, &ids);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "real");
    }
}
