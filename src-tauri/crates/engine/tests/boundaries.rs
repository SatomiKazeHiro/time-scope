//! spec §4 的硬边界：engine 是纯库。
//!
//! 这不是"检查代码风格"，而是防止未来某次改动悄悄把 IO 引进引擎——
//! 一旦 engine 开始读文件或连数据库，可重放（spec §7.4）和独立测试就没了。
//!
//! 这条测试会在有人破坏边界时立刻变红，这是它的全部意义。

use std::path::{Path, PathBuf};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read_dir") {
        let p = entry.expect("dir entry").path();
        if p.is_dir() {
            rust_sources(&p, out);
        } else if p.extension().map(|e| e == "rs").unwrap_or(false) {
            out.push(p);
        }
    }
}

#[test]
fn engine_manifest_pulls_in_no_io_or_tauri() {
    let raw = std::fs::read_to_string(crate_root().join("Cargo.toml")).expect("read Cargo.toml");
    // 注释里可能出现这些词（例如"刻意没有 tauri"），先剥掉再匹配，
    // 否则这个检查会一直假阳性。
    let manifest: String = raw
        .lines()
        .map(|l| l.split('#').next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("
");

    for forbidden in ["tauri", "rusqlite", "activity-storage", "activity-collector", "reqwest"] {
        assert!(
            !manifest.contains(forbidden),
            "engine 的 Cargo.toml 出现了 {forbidden}——spec §4 要求 engine 是纯库"
        );
    }
}

#[test]
fn engine_source_never_touches_io() {
    let mut files = Vec::new();
    rust_sources(&crate_root().join("src"), &mut files);
    assert!(files.len() >= 5, "应该扫到多个源文件，实际 {}", files.len());

    for f in &files {
        let text = std::fs::read_to_string(f).expect("read source");
        for bad in [
            "std::fs::",
            "std::net::",
            "std::process::Command",
            "std::io::Write",
            "println!",
            "eprintln!",
        ] {
            assert!(
                !text.contains(bad),
                "{} 里出现了 {bad}——engine 不该做 IO 或直接打印",
                f.display()
            );
        }
    }
}

#[test]
fn engine_does_not_depend_on_the_app_crate() {
    // engine 位于 workspace 之下，不能反过来认识 app 层
    for f in ["src/lib.rs"] {
        let text = std::fs::read_to_string(crate_root().join(f)).expect("read");
        assert!(
            !text.contains("time_scope"),
            "{f} 引用了 app crate，engine 应与 app 层解耦"
        );
    }
}

#[test]
fn reduce_is_reachable_without_any_io() {
    // 一个能跑通的最小闭环：构造事件 -> reduce -> 拿到段，全程不碰 DB
    use activity_core::{Event, EventType, WindowFocusPayload};
    use activity_engine::{reduce, EngineConfig, EngineState, RuleSet};

    let rules = RuleSet::from_toml(
        r#"
[[rule]]
id = "x"
process = ["a.exe"]
category = "work"
confidence = 0.8
"#,
    )
    .expect("parse rules");
    let cfg = EngineConfig::default()
        .with_min_segment_duration_s(0)
        .with_grace_period_s(0);

    let ev = |ts: i64, app: &str| {
        Event::new(
            EventType::WindowFocus(WindowFocusPayload {
                process_name: app.into(),
                window_title: None,
                exe_path: None,
            }),
            ts,
        )
    };

    let mut st = EngineState::initial();
    let out = reduce(&st, &ev(0, "a.exe"), &rules, &cfg);
    st = out.state;
    let out = reduce(&st, &ev(600_000, "b.exe"), &rules, &cfg);

    assert_eq!(out.closed_segments.len(), 1);
    assert_eq!(out.closed_segments[0].category.as_str(), "work");
}
