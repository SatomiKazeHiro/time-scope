//! IPC 命令：前端能调用的全部 9 条。
//!
//! 从 `lib.rs` 拆出来的原因：`lib.rs` 同时是应用入口、命令宿主和启动编排，
//! 改其中任何一件事都要在同一百来行里找位置。现在边界是：
//! - `lib.rs` —— 插件顺序、启动、退出钩子
//! - `boot.rs` —— 启动时那六步（开库 → 清理 → 装配重放 → 起线程 → 托盘 → manage）
//! - `ipc.rs` ← 本文件 —— 命令
//! - `state.rs` —— 命令与线程共享的东西
//! - `maintenance.rs` —— 挂在轮询上的维护
//!
//! **所有命令都标了 `async`**，所以它们跑在 Tauri 的异步运行时上，不是主线程。
//! Tauri v2 文档：「Commands without the async keyword are executed on the main
//! thread.」`get_top_titles` 那条 `json_extract` 全表扫实测一年数据 ≈20 秒，
//! 挂在主线程上窗口会整个"未响应"、用户十有八九直接强杀进程。
//! `tests/ipc_threading.rs` 守着这条不变量（它扫的就是本文件）。

use crate::config;
use crate::date_range::{day_range_ms, local_offset};
use crate::day_replay::replay_day_once;
use crate::maintenance;
use crate::rules;
use crate::state::AppState;
use crate::titles;
use crate::title_norm;
use activity_storage::StoredSegment;
use std::collections::BTreeMap;

/// 取某天的全部 ActivitySegment（spec §9）。
///
/// 结果 = 已落库的段 + 正在生长的当前段（还没落库，投影出来只为实时）。
///
/// 这个函数同时是三轮车的车头：**维护 → 按需重放 → 读**。三件事各自独立，
/// 但都挂在这条稳定高频的 IPC 上（理由见各自的注释）；写成三个具名调用而不是
/// 揉进一段，是因为「改轮询频率」不该悄悄改掉另外两件事的行为。
#[tauri::command(async)]
pub fn get_segments(
    state: tauri::State<'_, AppState>,
    date: String,
) -> Result<Vec<StoredSegment>, String> {
    let offset = local_offset()?;
    let (start_ms, end_ms) = day_range_ms(&date, offset)?;

    // ① 维护：至多每 24h 清一次过期原始事件（`maintenance` 里有取舍说明）
    maintenance::tick(&state);

    // ② 首次查看某一天时按需重放：启动只做了"今天"，别的日子可能还留着
    //    上次崩溃前没被引擎处理过的孤儿事件。
    replay_day_once(
        &state.conn,
        &state.replayed,
        &state.rules_snapshot(),
        &state.config,
        &date,
        offset,
        &state.redactor,
    );

    // ③ 读
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
pub fn get_segment_titles(
    state: tauri::State<'_, AppState>,
    event_ids: Vec<String>,
) -> Result<Vec<titles::SegmentTitle>, String> {
    Ok(titles::titles_for(&state.conn, &event_ids, &state.redactor))
}

/// 汇总页的热力图数据。**无参数** —— 热力图永远渲染全部数据，
/// 不受选中范围影响（spec §2.3）。
///
/// 库为空时返回 `None`：那是「刚装完还没跑满一天」的真实状态，
/// 前端据此显示空状态，而不是拿到 first/last 为空串的半成品。
#[tauri::command(async)]
pub fn get_daily_calendar(
    state: tauri::State<'_, AppState>,
) -> Result<Option<activity_storage::DailyCalendar>, String> {
    let c = state.writer.conn();
    activity_storage::daily_calendar(&c).map_err(|e| e.to_string())
}

/// `get_summary` 的返回形状。storage 的两个结果结构在这里合成一个 ——
/// 前端要的是一个对象，不是两个。
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryOut {
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
pub fn get_summary(
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
pub fn get_top_titles(
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
pub fn get_uncovered_programs(
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
pub fn add_rule(
    state: tauri::State<'_, AppState>,
    application: String,
    category: String,
) -> Result<(), String> {
    let new_set = rules::append_rule(&rules::rules_path(), &application, &category, 0.9)?;
    // 引擎实时流与按需重放两侧一起换 —— 唯一入口在 `AppState::replace_rules`
    state.replace_rules(new_set);
    Ok(())
}

/// 前端拿到的配置形状。与 `AppConfig` 字段一一对应，但：
/// - `close_behavior` 用字符串（枚举在 Rust 侧、前端不认）
/// - `needs_restart` 附带**每个 `config.toml` 键改完何时生效**，界面靠它标注
///   「重启后生效」。键用 toml 里的原名（snake_case），前端那侧的连接点。
///
/// 这张表以前只存在于 `set_config` 的返回值里，而角标要的是**加载时**就有 ——
/// 于是前端照着「哪些字段要重启由后端说了算」的说法自己抄了一份，两份慢慢
/// 就对不上了。现在两边读同一个值（`config` 模块里的测试钉着）。
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
pub fn get_config(state: tauri::State<'_, AppState>) -> Result<ConfigOut, String> {
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
        // 「哪些键要重启」是**配置项自己的属性**，住在 `config::FIELDS`。
        // 这里只把它翻译成前端要的形状，不再维护第二份键名清单。
        needs_restart: config::needs_restart_map(),
    })
}

/// 改配置的一个字段。校验、夹取、写回、要不要重启的判定全在 `config::set_value`
/// —— 这个命令只负责把「保留期」这一项的热生效接上。
#[tauri::command(async)]
pub fn set_config(
    state: tauri::State<'_, AppState>,
    key: String,
    value: String,
) -> Result<bool, String> {
    let outcome = config::set_value(&state.config_path, &key, &value)?;

    // 保留期改成热生效：下一次 5 秒轮询就按新值清理。
    // 这是唯一一个「写完还要回写内存」的字段，所以由这里显式接一下，
    // 而不是让 `set_value` 去认识 `AppState`。
    if key == "events_retention_days" {
        if let Some(days) = outcome.int {
            if let Ok(mut cur) = state.retention_days.lock() {
                *cur = days;
            }
        }
    }

    Ok(outcome.needs_restart)
}
