//! Tauri 应用入口：把 collector / storage / engine / IPC 缝在一起。
//!
//! 线程模型（spec §3、§5.1）：
//! ```text
//!   [watcher 线程]  SetWinEventHook + 原生消息泵 ─┐
//!   [poller 线程]   GetLastInputInfo 每 1s 轮询 ─┤─ RawSignal ─▶ [consumer 线程]
//!                                                 │                  │
//!                                            RawSignal          ┌───┴──────────────┐
//!                                                 │              ▼                  ▼
//!                                            [BatchWriter]  [engine 线程]      SQLite(WAL)
//!                                            原始 Event       ActivitySegment
//! ```

mod autostart;
mod close_behavior;
mod tray;
mod config;
mod date_range;
mod day_replay;
mod redact;
mod engine_runtime;
mod engine_thread;
mod exit_flush;
mod rules;
mod single_instance;
mod titles;

use activity_collector::signals::RawSignal;
use activity_storage::{open_file_shared, BatchWriter, StoredSegment};
use date_range::{day_range_ms, local_offset};
use day_replay::{replay_day_once, ReplayedDays};
use engine_runtime::EngineRuntime;
use exit_flush::flush_for_exit;
use redact::Redactor;
use titles::SegmentTitle;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use tauri::Manager;

/// 单实例：第二个实例启动时，把已有实例的窗口叫醒并置前（spec §12）
/// 插件在 `run()` 里最先注册；这里只描述"应该做什么"，便于单测。
pub use single_instance::{on_second_instance, SecondInstanceAction};

struct AppState {
    writer: Arc<BatchWriter>,
    /// 与 writer 共用同一个连接，供"按需重放历史日期"用。
    conn: activity_storage::SharedConn,
    engine: Arc<Mutex<EngineRuntime>>,
    /// 已重放过的日期。启动只重放"今天"；用户翻到别的日子时按需重放，
    /// 把上次崩溃留下的孤儿事件补成分段（spec §7.4）。
    replayed: ReplayedDays,
    rules: activity_engine::RuleSet,
    config: activity_engine::EngineConfig,
    /// 标题脱敏器。consumer 与"按需重放"两条路径共用（spec §11）。
    redactor: Arc<Redactor>,
}

/// 取某天的全部 ActivitySegment（spec §9）。
///
/// 结果 = 已落库的段 + 正在生长的当前段（还没落库，投影出来只为实时）。
#[tauri::command]
fn get_segments(
    state: tauri::State<'_, AppState>,
    date: String,
) -> Result<Vec<StoredSegment>, String> {
    let offset = local_offset()?;
    let (start_ms, end_ms) = day_range_ms(&date, offset)?;

    // 首次查看某一天时按需重放：启动只做了"今天"，别的日子可能还留着
    // 上次崩溃前没被引擎处理过的孤儿事件。
    replay_day_once(
        &state.conn,
        &state.replayed,
        &state.rules,
        &state.config,
        &date,
        offset,
        &state.redactor,
    );

    let stored = {
        let c = state.writer.conn();
        activity_storage::get_segments_in_range(&c, start_ms, end_ms).map_err(|e| e.to_string())?
    };
    let engine = state.engine.lock().map_err(|e| e.to_string())?;
    Ok(engine.segments_for_day(stored, start_ms, end_ms))
}

/// 取某个段的窗口标题（spec §10 的详情展示）。
///
/// 标题存在 events 表里，段本身不存——一个段可能对应几十个标题。
/// 返回值已去重并带 `redacted` 标记，前端据此做视觉区分，不必硬编码占位符。
#[tauri::command]
fn get_segment_titles(
    state: tauri::State<'_, AppState>,
    event_ids: Vec<String>,
) -> Result<Vec<SegmentTitle>, String> {
    Ok(titles::titles_for(&state.conn, &event_ids))
}

pub fn run() {
    let app = tauri::Builder::default()
        // 必须第一个注册（spec §12）：它靠抢全局锁判定"是不是第一个实例"，
        // 而 webview 初始化发生在插件之后。顺序反了的话，第二个进程会先把
        // webview 建起来、然后才发现自己是多余的 —— 实测会崩在
        // WebView2 "HRESULT(0x800700AA) 请求的资源在使用中"。
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // 第二个实例：把第一个叫醒，然后自己退出（`run` 返回即退出）
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let conn = open_file_shared(&rules::app_dir().join("time-scope.db"))
                .expect("open db");
            // spec §8.1：每 5s 或满 100 条单事务批量插入
            let writer = Arc::new(BatchWriter::new(Arc::clone(&conn), 5_000, 100));

            let replayed = ReplayedDays::default();
            let (rule_set, config, runtime) = day_replay::bootstrap_today(&conn, &replayed);
            let engine = Arc::new(Mutex::new(runtime));

            // 脱敏器：app 层构造，注入 consumer。敏感标题在**入队前**就被替换，
            // 不会以明文在内存队列里存在（spec §11）。
            let redactor = Arc::new(Redactor::new(&rule_set.redact));
            if !redactor.is_empty() {
                eprintln!(
                    "[time-scope] 已启用 {} 条标题脱敏规则{}",
                    redactor.len(),
                    if redactor.skipped().is_empty() {
                        String::new()
                    } else {
                        format!("（{} 条因正则非法被跳过）", redactor.skipped().len())
                    }
                );
            }
            let title_redactor: activity_collector::consumer::TitleRedactor = {
                let r = Arc::clone(&redactor);
                Arc::new(move |t: &str| r.redact(t).into_owned())
            };

            let (tx, rx) = channel::<RawSignal>();
            let (ev_tx, ev_rx) = channel::<activity_core::Event>();

            activity_collector::window::spawn_window_watcher(tx.clone());
            // spec §5.3 默认：300s 判空闲，心跳窗口 10s
            activity_collector::input::spawn_input_poller(tx.clone(), 300, 10);
            activity_collector::consumer::spawn_consumer_with(
                rx,
                Arc::clone(&writer),
                Some(ev_tx),
                Some(title_redactor),
            );
            engine_thread::spawn_engine_thread(ev_rx, Arc::clone(&conn), Arc::clone(&engine));

            eprintln!("[time-scope] db: {}", rules::app_dir().join("time-scope.db").display());
            app.manage(AppState {
                writer,
                conn: Arc::clone(&conn),
                engine,
                replayed,
                rules: rule_set,
                config,
                redactor,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_segments, get_segment_titles])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // spec §8.1：正常退出时 flush。BatchWriter 的后台线程每 100ms 才醒一次，
    // 而进程退出时它不保证跑得到——所以必须在这里显式同步刷一次。
    app.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            let n = flush_for_exit(&app_handle.state::<AppState>().writer);
            if n > 0 {
                eprintln!("[time-scope] flushed {n} events on exit");
            }
        }
    });
}

