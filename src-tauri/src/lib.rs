//! Tauri 应用入口：把 collector / storage / IPC 缝在一起。
//!
//! 线程模型（spec §3、§5.1）：
//! ```text
//!   [watcher 线程]  SetWinEventHook + 原生消息泵 ─┐
//!   [poller 线程]   GetLastInputInfo 每 1s 轮询 ─┤─ channel ─▶ [consumer 线程]
//!                                                 │                  │
//!                                            RawSignal               ▼
//!                                                            BatchWriter ─▶ SQLite
//! ```

mod date_range;

use activity_collector::signals::RawSignal;
use activity_storage::{open_file_shared, BatchWriter, StoredEvent};
use date_range::{day_range_ms, local_offset};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::sync::Arc;
use tauri::Manager;

struct AppState {
    writer: Arc<BatchWriter>,
}

fn db_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").expect("APPDATA env var");
    PathBuf::from(appdata).join("time-scope").join("time-scope.db")
}

/// 取某一天的全部原始事件（spec §9）。
///
/// 骨架阶段前端直接消费原始 Event；分类/汇总属 engine task。
#[tauri::command]
fn get_events(
    state: tauri::State<'_, AppState>,
    date: String,
) -> Result<Vec<StoredEvent>, String> {
    let (start_ms, end_ms) = day_range_ms(&date, local_offset()?)?;
    activity_storage::get_events_in_range(&state.writer.conn(), start_ms, end_ms)
        .map_err(|e| e.to_string())
}

/// DB 绝对路径（诊断/手工查数据用）。
#[tauri::command]
fn get_db_path() -> String {
    db_path().to_string_lossy().into_owned()
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let path = db_path();
            let conn = open_file_shared(&path).expect("open db");
            // spec §8.1：每 5s 或满 100 条单事务批量插入
            let writer = Arc::new(BatchWriter::new(conn, 5_000, 100));

            let (tx, rx) = channel::<RawSignal>();
            activity_collector::window::spawn_window_watcher(tx.clone());
            // spec §5.3 默认：300s 判空闲，心跳窗口 10s
            activity_collector::input::spawn_input_poller(tx.clone(), 300, 10);
            activity_collector::consumer::spawn_consumer(rx, Arc::clone(&writer));

            eprintln!("[time-scope] db: {}", path.display());
            app.manage(AppState { writer });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_events, get_db_path])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
