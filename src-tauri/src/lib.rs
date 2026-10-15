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
mod db_open;
mod close_behavior;
mod tray;
mod config;
mod date_range;
mod day_replay;
mod redact;
mod residency;
mod retention;
mod engine_runtime;
mod engine_thread;
mod exit_flush;
mod rules;
mod single_instance;
mod titles;
mod title_norm;

use activity_collector::signals::RawSignal;
use activity_storage::{BatchWriter, StoredSegment};
use date_range::{day_range_ms, local_offset};
use day_replay::{replay_day_once, ReplayedDays};
use engine_runtime::EngineRuntime;
use exit_flush::flush_for_exit;
use redact::Redactor;
use titles::SegmentTitle;
use std::collections::BTreeMap;
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
    /// 当前生效的规则集。设置页「加规则」后**热替换**，不等重启。
    ///
    /// 共享可变是因为两处要读它：engine 线程的分类（`EngineRuntime.rules`）
    /// 与「按需重放」的历史日期重放。两者都是低频读，写只发生在用户点保存时。
    rules: Arc<Mutex<activity_engine::RuleSet>>,
    config: activity_engine::EngineConfig,
    /// 标题脱敏器。consumer 与"按需重放"两条路径共用（spec §11）。
    redactor: Arc<Redactor>,
    /// 原始事件保留天数（B3）。0 = 永不删。
    ///
    /// 与 `rules` 同理做成共享可变：留 365 天改 30 天应当**立刻**开始按新值清理，
    /// 而不是等到下次启动。清理本身仍挂在 `get_segments` 上按 24h 至多一次，
    /// 不为它另开定时线程（见 B3 的原注释）。
    retention_days: Arc<Mutex<u32>>,
    /// `config.toml` 的路径。写回要定位文件；`residency` 里也有一份同样的。
    config_path: std::path::PathBuf,
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

    // B3：每 24h 至多清一次过期原始事件。挂在这条稳定的高频 IPC 上，
    // 而不是为此再开一个定时线程（代价只是每次多一次原子读）。
    // 每次现读现用：用户在设置页改保留期后，下一次轮询就按新值清理。
    let retention_days = *state
        .retention_days
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    retention::prune_if_due(&state.conn, retention_days, activity_storage::now_ms());

    // 首次查看某一天时按需重放：启动只做了"今天"，别的日子可能还留着
    // 上次崩溃前没被引擎处理过的孤儿事件。
    let rules = state
        .rules
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    replay_day_once(
        &state.conn,
        &state.replayed,
        &rules,
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

/// 设置页的「哪些程序还没规则覆盖」列表。
///
/// 回答的是**「你的配置缺了什么」**，与汇总面板的「今天干了什么」是两件事——
/// 同一张表曾在当日汇总里做过并被撤回，因为那会把一张配置表混进
/// 一个描述时间去向的面板。见 STATUS §4.1。
///
/// 走 `activities` 而不是 `events`：这条只需要进程名与时长，
/// `activities` 有 `application` 列且已分好段，不需要回表取 payload。
/// `idx_activities_range` 覆盖时间过滤。
///
/// `async` 是必需的而非风格问题：窗口默认 30 天，段数是万级
/// （实测 5 天约 2 千段，90 天上万），跑在主线程上会冻结界面（A3）。
#[tauri::command(async)]
fn get_uncovered_programs(
    state: tauri::State<'_, AppState>,
    days: Option<u32>,
) -> Result<Vec<activity_storage::UncoveredProgram>, String> {
    const DEFAULT_DAYS: u32 = 30;
    const MAX_DAYS: u32 = 365;
    let days = days.unwrap_or(DEFAULT_DAYS).clamp(1, MAX_DAYS);
    let c = state.writer.conn();
    activity_storage::uncovered_programs(&c, activity_storage::now_ms(), days)
        .map_err(|e| e.to_string())
}

/// 设置页的「给这个程序加一条规则」。
///
/// 写回 `rules.toml` 并**立刻热替换**内存里的规则集 —— 用户加完规则
/// 就不该被要求重启，这正是这个入口存在的理由。
///
/// `confidence` 固定 0.9：内置规则里跟 IDE / 浏览器同级的一律 0.9，
/// 终端那种宽泛匹配才给 0.7。从「程序名精确匹配」这个表单产生的规则
/// 没有理由比同类的低。
#[tauri::command(async)]
fn add_rule(
    state: tauri::State<'_, AppState>,
    application: String,
    category: String,
) -> Result<(), String> {
    let new_set = rules::append_rule(&rules::rules_path(), &application, &category, 0.9)?;

    // 引擎的实时流：换掉规则集，下一个 Event 就按新规则分类
    if let Ok(mut rt) = state.engine.lock() {
        rt.rules = new_set.clone();
    }
    // 「按需重放」历史日期时读的那一份也要换，否则两边口径不一致
    if let Ok(mut rs) = state.rules.lock() {
        *rs = new_set;
    }
    Ok(())
}

/// 前端拿到的配置形状。与 `AppConfig` 字段一一对应，但：
/// - `close_behavior` 用字符串（枚举在 Rust 侧、前端不认）
/// - `needs_restart` 附带**每个 `config.toml` 键改完何时生效**，界面靠它标注
///   「重启后生效」。键用 toml 里的原名（snake_case），前端那侧的连接点。
///
/// 这张表以前只存在于 `set_config` 的返回值里，而角标要的是**加载时**就有 ——
/// 于是前端照着「哪些字段要重启由后端说了算」的说法自己抄了一份，两份慢慢
/// 就对不上了。现在两边读同一个值（`config_restart_honesty_tests` 钉着）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigOut {
    pub idle_threshold_s: u32,
    pub grace_period_s: u32,
    pub min_segment_duration_s: u32,
    pub heartbeat_every_s: u32,
    pub autostart: bool,
    pub close_behavior: String,
    pub events_retention_days: u32,
    pub needs_restart: BTreeMap<&'static str, bool>,
}

/// 读配置。**每次现读磁盘**而不是返回启动时的快照 ——
/// 用户可能手改过 `config.toml`，界面显示陈旧的值比报错还误导。
#[tauri::command(async)]
fn get_config(state: tauri::State<'_, AppState>) -> Result<ConfigOut, String> {
    let path = state.config_path.clone();
    let cfg = config::load_or_create(&path);
    Ok(ConfigOut {
        idle_threshold_s: cfg.idle_threshold_s,
        grace_period_s: cfg.grace_period_s,
        min_segment_duration_s: cfg.min_segment_duration_s,
        heartbeat_every_s: cfg.heartbeat_every_s,
        autostart: cfg.autostart,
        close_behavior: cfg.close_behavior.as_str().to_string(),
        events_retention_days: cfg.events_retention_days,
        needs_restart: needs_restart_map(),
    })
}

/// 改配置的一个字段。
///
/// **只改磁盘上的那一行**（`patch_line`），不覆盖整份文件、不抹掉注释。
/// 文件坏了就拒绝写入并把原因带回去 —— 与 `rules.toml` 的既定行为一致。
///
/// `needs_restart` 是返回值而不是让界面自己猜：**哪些字段热生效、
/// 哪些必须重启，是后端的事实**（引擎参数构造注入、采集线程参数绑定），
/// 让前端复制一份这个表，迟早会跟实现对不上。
#[tauri::command(async)]
fn set_config(
    state: tauri::State<'_, AppState>,
    key: String,
    value: String,
) -> Result<bool, String> {
    // 保留期的数值单独带出来给热生效用 —— 不在下面把 `rendered` 重新
    // parse 一遍：那是第二次解析、且解析失败只能静默忽略（`unwrap_or`）。
    // 这里多带一个值，换掉一条「出错也看不见」的路。
    let mut new_retention: Option<u32> = None;
    let rendered = match key.as_str() {
        // 数字：先解析再回写，让 clamp 与默认值逻辑只有一份（在 config::parse 里）
        "idle_threshold_s" | "grace_period_s" | "min_segment_duration_s"
        | "heartbeat_every_s" | "events_retention_days" => {
            let n: i64 = value
                .parse()
                .map_err(|_| format!("`{value}` 不是数字"))?;
            // clamp 在 parse 里做，这里复用一份默认值走同样的路
            let probe = config::parse(&format!("{key} = {n}\n"));
            let clamped = match key.as_str() {
                "idle_threshold_s" => probe.idle_threshold_s,
                "grace_period_s" => probe.grace_period_s,
                "min_segment_duration_s" => probe.min_segment_duration_s,
                "heartbeat_every_s" => probe.heartbeat_every_s,
                "events_retention_days" => {
                    new_retention = Some(probe.events_retention_days);
                    probe.events_retention_days
                }
                // 具名而不是 `_`：上面那个 arm 加了数字键而这里忘了登记时，
                // 一条 Err 会把疏漏说清楚。写 `_` 的话新键会被悄悄当成保留期。
                other => {
                    return Err(format!(
                        "`{other}` 是数字键却没在 clamp 分支里登记 —— 外层 match 加了键这里要一起改"
                    ))
                }
            };
            clamped.to_string()
        }
        "autostart" => match value.as_str() {
            "true" => "true".to_string(),
            "false" => "false".to_string(),
            other => return Err(format!("`{other}` 不是 true / false")),
        },
        "close_behavior" => {
            let cb = config::CloseBehavior::from_str(&value)
                .ok_or_else(|| "关窗行为只能是 ask / minimize / quit".to_string())?;
            format!("\"{}\"", cb.as_str())
        }
        other => return Err(format!("未知配置项 `{other}`")),
    };

    config::patch_line(&state.config_path, &key, &rendered).map_err(|e| e.to_string())?;

    // 保留期改成热生效：下一次 5 秒轮询就按新值清理
    if let Some(days) = new_retention {
        if let Ok(mut cur) = state.retention_days.lock() {
            *cur = days;
        }
    }

    Ok(config_field_needs_restart(&key))
}

/// `config.toml` 里全部可改的键。**加键时这里和 `set_config` 的 `match` 要一起改**。
/// 漏改 `set_config` 的 `match` → 那条命令直接报「未知配置项」，写不进去。
/// 漏改这里 → 判定表里没这个键，前端 `?? true` 按「要重启」保守显示：
/// 热生效的键会多要求一次重启，但不会谎称已生效。角标不会消失。
const CONFIG_KEYS: &[&str] = &[
    "idle_threshold_s",
    "grace_period_s",
    "min_segment_duration_s",
    "heartbeat_every_s",
    "events_retention_days",
    "autostart",
    "close_behavior",
];

/// 每个键 → 改完是否要重启。**界面上那个「重启后生效」角标读的就是这张表。**
fn needs_restart_map() -> BTreeMap<&'static str, bool> {
    CONFIG_KEYS
        .iter()
        .map(|k| (*k, config_field_needs_restart(k)))
        .collect()
}

/// 这个字段改完**必须重启**才生效吗？
///
/// 依据是各参数被绑到哪里 / 有没有被现读，不是界面偏好：
/// - `events_retention_days` —— `set_config` 把新值写回 `AppState.retention_days`，
///   `get_segments` 每次现读它（见上面 IPC），真热生效
/// - `close_behavior` —— 关窗拦截**现读 `config.toml`**（`residency.rs` 的
///   `effective_close_behavior`），设置页写完下一次关窗就照新的走
/// - `autostart` —— **`Residency` 在 `install()` 时把它快照进 `AtomicBool`**
///   （`residency.rs`），托盘勾选态也按启动值构造，而 `autostart::enable/disable`
///   只在托盘菜单回调里被调过。`set_config` 一样都碰不到：改了要重启。
///   （2026-10-15 更正：这里原来把 `autostart` / `close_behavior` 都算成
///   「已经热生效」——**那是错的**，界面因此标着「已保存，立刻生效」而实际要等
///   下次启动。`close_behavior` 后来改成现读磁盘才是真热生效，`autostart` 没有。）
/// - 四个采集/引擎参数 —— 绑进 `spawn_input_poller` 的线程参数与
///   `EngineConfig` 的构造注入，进程内改不了（STATUS §4.1「配置热重载」那条）
fn config_field_needs_restart(key: &str) -> bool {
    // **未知键一律当作要重启**（`!matches!` 的默认分支）。没登记过的键
    // 宁可多提醒一次重启，也不要悄悄标成「已生效」。
    !matches!(key, "events_retention_days" | "close_behavior")
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
            // B4：打不开数据库就弹一句带路径/原因/下一步的说明，然后让
            // setup 返回 Err（Tauri 干净退出），而不是 `.expect("open db")`
            // panic —— 那句话没有路径也没有原因，用户无从下手。
            let db_path = rules::app_dir().join("time-scope.db");
            let conn = match db_open::open_db(&db_path) {
                Ok(conn) => conn,
                Err(e) => {
                    eprintln!("[time-scope] {e}");
                    db_open::show_error_dialog(&e);
                    return Err(Box::new(e) as Box<dyn std::error::Error>);
                }
            };
            // spec §8.1：每 5s 或满 100 条单事务批量插入
            let writer = Arc::new(BatchWriter::new(Arc::clone(&conn), 5_000, 100));

            // spec §5：行为参数来自 %APPDATA%/time-scope/config.toml，
            // 文件缺失时生成默认模板，坏了就用默认值（都不影响启动）。
            let app_config = config::load_or_create(&config::config_path());

            // B3：启动时清一次过期事件，并在这里 VACUUM 把空间还给文件系统。
            // **必须在采集线程起来之前做** —— VACUUM 会锁住整个库，
            // 放在运行期就是把「20 秒写不进去」那种事变成日常。
            // 运行期只删不 VACUUM：删掉的页进 freelist 会被复用，**这就止住了增长**，
            // 已经很大的文件要缩回去则交给这一次启动时的 VACUUM。
            retention::prune_at_startup(&conn, app_config.events_retention_days);

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
                    activity_collector::session::RegistrationOutcome::Ok => "已就绪".to_string(),
                    // **别笼统说「不可用」**（2026-10-13）：真实事故里
                    // 只有电源注册失败、锁屏那一路其实正常，却被这句话
                    // 说成「该时段都不被记录」，害得以为锁屏没在记。
                    // 具体哪一路坏了，上面那两行各自已经说清了。
                    activity_collector::session::RegistrationOutcome::Failed { reason } => {
                        format!("部分可用（{reason}）")
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
                rules: Arc::new(Mutex::new(rule_set)),
                config,
                redactor,
                retention_days: Arc::new(Mutex::new(app_config.events_retention_days)),
                config_path: config::config_path(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_segments,
            get_segment_titles,
            get_daily_calendar,
            get_summary,
            get_top_titles,
            get_uncovered_programs,
            add_rule,
            get_config,
            set_config,
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

#[cfg(test)]
mod config_restart_honesty_tests {
    use super::{config_field_needs_restart, needs_restart_map, CONFIG_KEYS};

    /// 界面标「已保存，立刻生效」的字段**必须真的会热生效**。
    ///
    /// 2026-10-15 之前这里把 `autostart` / `close_behavior` 都算成热生效，
    /// 但 `Residency` 是在 `install()` 时**快照**了这两个值，`set_config` 只
    /// `patch_line` 写文件、不回写那份快照 —— 界面说「已保存」而行为要等
    /// 下次启动。界面说谎比多要求一次重启更糟：用户会以为设置没生效。
    ///
    /// 两个键后来走了**不同的路**：`close_behavior` 改成关窗时现读磁盘，
    /// 成了真热生效；`autostart` 只能诚实地要求重启 —— 托盘勾选态与
    /// 系统注册表都要重过启动流程，`set_config` 一个都碰不到。
    #[test]
    fn close_behavior_claims_to_be_live_because_the_close_handler_re_reads_the_file() {
        assert!(
            !config_field_needs_restart("close_behavior"),
            "关窗拦截现读 config.toml（residency::effective_close_behavior），真热生效"
        );
    }

    #[test]
    fn autostart_does_not_claim_to_be_live() {
        assert!(
            config_field_needs_restart("autostart"),
            "Residency 是启动快照，托盘勾选态与系统注册表 set_config 都碰不到 —— 必须说要重启"
        );
    }

    /// 保留期仍然热生效（`set_config` 回写 `AppState.retention_days`）。
    #[test]
    fn the_genuinely_live_fields_stay_live() {
        assert!(!config_field_needs_restart("events_retention_days"));
    }

    /// 四个绑在采集线程/引擎构造上的参数仍然要重启。
    #[test]
    fn engine_and_collector_params_still_need_restart() {
        for key in [
            "idle_threshold_s",
            "grace_period_s",
            "min_segment_duration_s",
            "heartbeat_every_s",
        ] {
            assert!(config_field_needs_restart(key), "{key} 绑在构造注入上");
        }
    }

    /// 未知键不能被静默当成「立刻生效」。
    #[test]
    fn an_unknown_key_is_treated_as_needing_restart() {
        assert!(
            config_field_needs_restart("some_key_we_never_saw"),
            "没登记过的键宁可说要多重启，别让它悄悄标成已生效"
        );
    }

    /// `CONFIG_KEYS` 是「界面画不画角标」和「`set_config` 收不收这个键」的
    /// 共同依据。加了键却忘了往这里加，那个控件的角标会保守地按「要重启」显示，
    /// 也改不进去。数字写死是故意的 —— 加键应当是个显眼的动作。
    ///
    /// 表里带的判定就是界面渲染的东西。曾经前端另抄了一份 `FIELDS[].restart`，
    /// 两边**一致地错** —— 那次界面说谎的机制。
    #[test]
    fn every_configurable_key_has_a_verdict() {
        assert_eq!(CONFIG_KEYS.len(), 7, "config.toml 的可改键变了就同步这里");
        let map = needs_restart_map();
        assert_eq!(map.len(), CONFIG_KEYS.len());
        assert_eq!(map.get("close_behavior"), Some(&false));
        assert_eq!(map.get("events_retention_days"), Some(&false));
        assert_eq!(map.get("autostart"), Some(&true));
    }
}
