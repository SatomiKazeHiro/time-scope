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

mod date_range;
mod day_replay;
mod engine_runtime;
mod engine_thread;
mod exit_flush;
mod rules;

use activity_collector::signals::RawSignal;
use activity_storage::{open_file_shared, BatchWriter, StoredSegment};
use date_range::{day_range_ms, local_offset};
use day_replay::{replay_day_once, ReplayedDays};
use engine_runtime::EngineRuntime;
use exit_flush::flush_for_exit;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use tauri::Manager;

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
    );

    let stored = {
        let c = state.writer.conn();
        activity_storage::get_segments_in_range(&c, start_ms, end_ms).map_err(|e| e.to_string())?
    };
    let engine = state.engine.lock().map_err(|e| e.to_string())?;
    Ok(engine.segments_for_day(stored, start_ms, end_ms))
}

pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            let conn = open_file_shared(&rules::app_dir().join("time-scope.db"))
                .expect("open db");
            // spec §8.1：每 5s 或满 100 条单事务批量插入
            let writer = Arc::new(BatchWriter::new(Arc::clone(&conn), 5_000, 100));

            let replayed = ReplayedDays::default();
            let (rule_set, config, runtime) = day_replay::bootstrap_today(&conn, &replayed);
            let engine = Arc::new(Mutex::new(runtime));

            let (tx, rx) = channel::<RawSignal>();
            let (ev_tx, ev_rx) = channel::<activity_core::Event>();

            activity_collector::window::spawn_window_watcher(tx.clone());
            // spec §5.3 默认：300s 判空闲，心跳窗口 10s
            activity_collector::input::spawn_input_poller(tx.clone(), 300, 10);
            activity_collector::consumer::spawn_consumer(rx, Arc::clone(&writer), Some(ev_tx));
            engine_thread::spawn_engine_thread(ev_rx, Arc::clone(&conn), Arc::clone(&engine));

            eprintln!("[time-scope] db: {}", rules::app_dir().join("time-scope.db").display());
            app.manage(AppState {
                writer,
                conn: Arc::clone(&conn),
                engine,
                replayed,
                rules: rule_set,
                config,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_segments])
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

