//! Tauri 应用入口：插件顺序、启动一行、退出钩子。
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
//!
//! 这个文件只做三件事：装插件（顺序是语义）、叫一声 `boot::bootstrap`、
//! 挂退出钩子。命令在 `ipc.rs`，启动编排在 `boot.rs`，共享状态在 `state.rs` ——
//! 2026-10-17 之前它们全挤在这里，改任何一件都要在同一百来行里找位置。

mod autostart;
mod boot;
mod close_behavior;
mod config;
mod date_range;
mod day_replay;
mod db_open;
mod engine_runtime;
mod engine_thread;
mod exit_flush;
mod ipc;
mod maintenance;
mod redact;
mod residency;
mod retention;
mod rules;
mod single_instance;
mod state;
mod title_norm;
mod titles;
mod tray;

use exit_flush::flush_for_exit;
use state::AppState;
use tauri::Manager;

/// 单实例：第二个实例启动时，把已有实例的窗口叫醒并置前（spec §12）
/// 插件在 `run()` 里最先注册；这里只描述"应该做什么"，便于单测。
pub use single_instance::{on_second_instance, SecondInstanceAction};

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
        .setup(|app| boot::bootstrap(app))
        .invoke_handler(tauri::generate_handler![
            ipc::get_segments,
            ipc::get_segment_titles,
            ipc::get_daily_calendar,
            ipc::get_summary,
            ipc::get_top_titles,
            ipc::get_uncovered_programs,
            ipc::add_rule,
            ipc::get_config,
            ipc::set_config,
        ])
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
