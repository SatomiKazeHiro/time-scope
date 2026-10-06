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
//!
//! **所有 IPC 命令都标了 `async`**，所以它们跑在 Tauri 的异步运行时上，
//! 不是主线程。Tauri v2 文档：「Commands without the async keyword are
//! executed on the main thread.」`get_top_titles` 那条 `json_extract` 全表扫
//! 实测一年数据 ≈20 秒，挂在主线程上窗口会整个"未响应"、用户十有八九直接
//! 强杀进程。`tests/ipc_threading.rs` 守着这条不变量。

mod autostart;
mod close_behavior;
mod tray;
mod config;
mod date_range;
mod day_replay;
mod redact;
mod residency;
mod engine_runtime;
mod engine_thread;
mod exit_flush;
mod rules;
mod single_instance;
mod titles;
mod title_norm;

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
#[tauri::command(async)]
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
///
/// `titles_for` 返回前会再过一遍脱敏器（A4）：落库时没脱敏的历史明文标题
/// 也在此刻被遮住，界面上不会露出。
#[tauri::command(async)]
fn get_segment_titles(
    state: tauri::State<'_, AppState>,
    event_ids: Vec<String>,
) -> Result<Vec<SegmentTitle>, String> {
    Ok(titles::titles_for(&state.conn, &event_ids, &state.redactor))
}

/// 汇总页的热力图数据。**无参数** —— 热力图永远渲染全部数据，
/// 不受选中范围影响（spec §2.3）。
///
/// 库为空时返回 `None`：那是「刚装完还没跑满一天」的真实状态，
/// 前端据此显示空状态，而不是拿到 first/last 为空串的半成品。
#[tauri::command(async)]
fn get_daily_calendar(
    state: tauri::State<'_, AppState>,
) -> Result<Option<activity_storage::DailyCalendar>, String> {
    let c = state.writer.conn();
    activity_storage::daily_calendar(&c).map_err(|e| e.to_string())
}

/// `get_summary` 的返回形状。storage 的两个结果结构在这里合成一个 ——
/// 前端要的是一个对象，不是两个。
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SummaryOut {
    total_ms: i64,
    active_ms: i64,
    idle_ms: i64,
    segment_count: i64,
    switch_count: i64,
    hourly_ms: Vec<i64>,
    donut: Vec<activity_storage::DonutSlice>,
    top_apps: Vec<activity_storage::AppSlice>,
}

/// 汇总页顶部指标。`from` / `to` 都是 `YYYY-MM-DD` 的**闭区间**
/// （点「某一天」时两者相同），内部转成半开区间。
#[tauri::command(async)]
fn get_summary(
    state: tauri::State<'_, AppState>,
    from: String,
    to: String,
) -> Result<SummaryOut, String> {
    let offset = local_offset()?;
    let (start_ms, _) = day_range_ms(&from, offset)?;
    // `day_range_ms(to)` 的第二个返回值就是「to 那天的次日零点」，
    // 正好是闭区间 [from, to] 转半开区间 [from00:00, to+1 00:00) 的终点。
    // 不必自己给日期加一天 —— 加一天要碰 Date::next_day，day_range_ms 已经做了。
    let (_, end_ms) = day_range_ms(&to, offset)?;
    let offset_secs = offset.whole_seconds();

    let c = state.writer.conn();
    let totals =
        activity_storage::range_totals(&c, start_ms, end_ms).map_err(|e| e.to_string())?;
    let pacing = activity_storage::range_pacing(&c, start_ms, end_ms, offset_secs)
        .map_err(|e| e.to_string())?;

    Ok(SummaryOut {
        total_ms: totals.total_ms,
        active_ms: totals.active_ms,
        idle_ms: totals.idle_ms,
        segment_count: totals.segment_count,
        switch_count: pacing.switch_count,
        // Vec 而非 [i64; 24]：前端拿到的就是 24 个数的普通数组。
        hourly_ms: pacing.hourly_ms.to_vec(),
        donut: totals.donut,
        top_apps: totals.top_apps,
    })
}

/// 窗口标题排名。**懒加载**——底部面板进入视口才发这一发（spec §3.3）。
///
/// 不设范围上限：当前 5 天数据实测 199ms；按每天 10,092 事件外推，
/// 一年约 3.7M 行 → 约 15s。这是 spec §9 第 7 条记录的已知代价。
///
/// `async` 在这里是**必需**而非风格问题：这条 SQL 里的
/// `json_extract(payload,...)` 必须回表取 payload、无法用索引覆盖，
/// 只能全表扫；跑在主线程上会冻结整个界面（A3）。
#[tauri::command(async)]
fn get_top_titles(
    state: tauri::State<'_, AppState>,
    from: String,
    to: String,
    limit: Option<usize>,
) -> Result<Vec<title_norm::MergedTitle>, String> {
    const DEFAULT_LIMIT: usize = 10;
    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(50);
    let offset = local_offset()?;
    let (start_ms, _) = day_range_ms(&from, offset)?;
    let (_, end_ms) = day_range_ms(&to, offset)?;

    let c = state.writer.conn();
    let raw = activity_storage::title_counts_in_range(&c, start_ms, end_ms)
        .map_err(|e| e.to_string())?;
    // A4：`title_counts_in_range` 给的是 events.payload 里的**原文**，
    // 合并前必须再过一遍脱敏器，否则配了 [[redact]] 的用户在这里
    // 仍然看得到明文客户名/订单号。
    Ok(title_norm::merge_top_titles(raw, limit, &state.redactor))
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

            // spec §5：行为参数来自 %APPDATA%/time-scope/config.toml，
            // 文件缺失时生成默认模板，坏了就用默认值（都不影响启动）。
            let app_config = config::load_or_create(&config::config_path());

            let replayed = ReplayedDays::default();
            let (rule_set, config, runtime) =
                day_replay::bootstrap_today(&conn, &replayed, &app_config);
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
            // 空闲阈值与心跳窗口来自 config.toml（spec §5.2）
            activity_collector::input::spawn_input_poller(
                tx.clone(),
                app_config.idle_threshold_s,
                app_config.heartbeat_every_s,
            );
            activity_collector::consumer::spawn_consumer_with(
                rx,
                Arc::clone(&writer),
                Some(ev_tx),
                Some(title_redactor),
            );
            engine_thread::spawn_engine_thread(ev_rx, Arc::clone(&conn), Arc::clone(&engine));

            // spec §5.3 / §3：锁屏与合盖监听。监听自己建了一个 message-only 窗口，
            // 所以不依赖主窗口，也就不需要改动窗口结构。
            let session = activity_collector::session::spawn_listener(tx.clone());
            eprintln!(
                "[time-scope] 锁屏/睡眠监听 {}",
                match session {
                    activity_collector::session::RegistrationOutcome::Ok => "已就绪",
                    activity_collector::session::RegistrationOutcome::Failed { .. } => {
                        "不可用，该时段将不被记录"
                    }
                }
            );

            // spec §4：托盘 + 关窗行为 + 自启同步。托盘建不起来时内部会退回
            // "关窗即退出"，主链路不受影响（spec §7）。
            let tray_ok = residency::install(app, app_config);
            eprintln!(
                "[time-scope] db: {}（托盘 {}）",
                rules::app_dir().join("time-scope.db").display(),
                if tray_ok { "已就绪" } else { "不可用，关窗即退出" }
            );
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
        .invoke_handler(tauri::generate_handler![
            get_segments,
            get_segment_titles,
            get_daily_calendar,
            get_summary,
            get_top_titles,
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

