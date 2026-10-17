//! 启动编排：六个具名步骤。
//!
//! 顺序本身是语义：VACUUM 必须在采集线程之前（它会锁住整个库），今日重放必须
//! 在实时流之前（否则正在生长的段会丢），托盘与自启要在 `manage` 之前拿到配置。

use crate::config;
use crate::day_replay::{self, ReplayedDays};
use crate::db_open;
use crate::engine_thread;
use crate::redact::Redactor;
use crate::residency;
use crate::retention;
use crate::state::AppState;
use activity_collector::signals::RawSignal;
use activity_storage::{BatchWriter, SharedConn};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use tauri::Manager;

/// 启动整条链路。返回 `Err` 时 Tauri 会干净退出（不 panic）。
pub fn bootstrap(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // ① 打开数据库。打不开就弹一句带路径/原因/下一步的说明再返回 Err，
    //    别 `.expect()` panic —— 那句话没有路径也没有原因，用户无从下手。
    let data_dir = config::data_dir();
    let db_path = data_dir.join("time-scope.db");
    let conn: SharedConn = match db_open::open_db(&db_path) {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("[time-scope] {e}");
            db_open::show_error_dialog(&e);
            return Err(Box::new(e) as Box<dyn std::error::Error>);
        }
    };

    // spec §8.1：每 5s 或满 100 条单事务批量插入
    let writer = Arc::new(BatchWriter::new(Arc::clone(&conn), 5_000, 100));

    // spec §5：行为参数来自数据目录里的 config.toml（`config::data_dir()`），
    // 文件缺失时生成默认模板，坏了就用默认值（都不影响启动）。
    let app_config = config::load_or_create(&config::config_path());

    // ② 启动清理 + VACUUM，必须在采集线程起来之前做（VACUUM 会锁住整个库）。
    //    运行期只删不 VACUUM：删掉的页进 freelist 会被复用，增长就止住了。
    retention::prune_at_startup(&conn, app_config.events_retention_days);

    // ③ 读规则 + 按它重放今天
    let replayed = ReplayedDays::default();
    let (rule_set, config, runtime) = day_replay::bootstrap_today(&conn, &replayed, &app_config);
    let engine = Arc::new(Mutex::new(runtime));

    // ④ 脱敏器：app 层构造，注入 consumer。敏感标题在入队前就被替换，
    //    不会以明文在内存队列里存在（spec §11）。
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

    // ⑤ 采集与处理线程。channel 在这里建，线程各自消费一口。
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
            activity_collector::session::RegistrationOutcome::Ok => "已就绪".to_string(),
            // 别笼统说「不可用」（2026-10-13）：真实事故里只有电源注册失败、
            // 锁屏那一路其实正常，却被这句话说成「该时段都不被记录」，
            // 害得以为锁屏没在记。具体哪一路坏了，上面那两行各自已经说清了。
            activity_collector::session::RegistrationOutcome::Failed { reason } => {
                format!("部分可用（{reason}）")
            }
        }
    );

    // ⑥ 托盘 + 关窗行为 + 自启同步。托盘建不起来时内部会退回"关窗即退出"，
    //    主链路不受影响（spec §7）。
    let tray_ok = residency::install(app, app_config);
    // 账本在哪，启动日志里就要看得见 —— 数据跟着 exe 走，用户该一眼确认这件事
    eprintln!(
        "[time-scope] 数据目录: {}（托盘 {}）",
        data_dir.display(),
        if tray_ok { "已就绪" } else { "不可用，关窗即退出" }
    );
    eprintln!("[time-scope] db: {}", db_path.display());

    // ⑦ 交给 IPC 与退出钩子
    app.manage(AppState {
        writer,
        conn: Arc::clone(&conn),
        engine,
        replayed,
        rules: Arc::new(Mutex::new(rule_set)),
        config,
        redactor,
        retention_days: Arc::new(Mutex::new(app_config.events_retention_days)),
        config_path: config::config_path(),
    });
    Ok(())
}
