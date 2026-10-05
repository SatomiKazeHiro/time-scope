//! 汇总页的聚合查询。
//!
//! 全部聚合都在这里，不散落到 app 层——app 层只做时区换算和 JSON 序列化。
//!
//! **按天分组用 SQL 的 `localtime`**，这依赖进程时区，且在夏令时切换日
//! 会差一小时。全项目已经建立在「单个固定 offset」的假设上
//! （`date_range::day_range_ms` 同款），单独给汇总页做 DST 正确的分组
//! 会制造「只有汇总页对、其他页错」的不一致。见 spec §3.4 与 §9 第 1 条。

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// 热力图的一格：某一天的监控总时长（**含 idle**）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayCell {
    /// 本地日 `YYYY-MM-DD`
    pub date: String,
    pub total_ms: i64,
}

/// 热力图的数据源。**`days` 只含有记录的日期**——补齐空缺是前端的活，
/// 因为 `GROUP BY` 会跳过没开机的那天，直接用它的结果排格子会整片错位。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyCalendar {
    /// 首个有记录的日子
    pub first: String,
    /// 最后一天（通常是今天）
    pub last: String,
    pub days: Vec<DayCell>,
}

/// 取全部数据的逐日监控时长。**无参数**——热力图永远渲染全部数据，
/// 不受选中范围影响（spec §2.3）。
///
/// 库为空时返回 `Ok(None)`：那是「刚装完还没跑满一天」的真实状态，
/// 调用方据此显示空状态，而不是拿到一个 first/last 为空串的半成品。
pub fn daily_calendar(conn: &Connection) -> Result<Option<DailyCalendar>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT date(start_at/1000,'unixepoch','localtime') AS d,
                SUM(end_at - start_at) AS ms
         FROM activities
         GROUP BY d
         ORDER BY d ASC",
    )?;
    let mut days = Vec::new();
    for row in stmt.query_map([], |row| {
        Ok(DayCell {
            date: row.get(0)?,
            total_ms: row.get::<_, i64>(1)?,
        })
    })? {
        days.push(row?);
    }

    // 一行都没有 = 库空。不给半成品（first/last 为空串的 DailyCalendar）。
    let Some(first_cell) = days.first() else {
        return Ok(None);
    };
    let first = first_cell.date.clone();
    let last = days
        .last()
        .map(|d| d.date.clone())
        .unwrap_or_else(|| first.clone());
    Ok(Some(DailyCalendar { first, last, days }))
}

/// 圆环的一档。**只有 4 种 key**，前端的 `DonutChart` 按下标取色，
/// 所以顺序固定为 work / browsing / idle / unknown。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DonutSlice {
    pub key: String,
    pub ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSlice {
    pub name: String,
    pub ms: i64,
}

/// 范围内 SQL 能一次算完的那些指标。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeTotals {
    pub total_ms: i64,
    pub active_ms: i64,
    pub idle_ms: i64,
    pub segment_count: i64,
    pub donut: Vec<DonutSlice>,
    pub top_apps: Vec<AppSlice>,
}

/// 圆环只画 4 档。其余四个类别（学习/娱乐/社交/生活）并入 `unknown` ——
/// spec §6：那样环才是完整 360°，且 `rules.toml` 命中它们时不至于无处可去。
const DONUT_KEYS: [&str; 4] = ["work", "browsing", "idle", "unknown"];

const TOP_APPS_LIMIT: i64 = 5;

/// `[start_ms, end_ms)` 半开区间内的时长三件套、段数、圆环、应用 Top。
///
/// 区间倒置（`start_ms >= end_ms`）不是错误，返回全 0 —— 公开的 IPC
/// 可能收到这种输入，让它炸没有好处。
pub fn range_totals(
    conn: &Connection,
    start_ms: i64,
    end_ms: i64,
) -> Result<RangeTotals, rusqlite::Error> {
    if start_ms >= end_ms {
        return Ok(RangeTotals {
            total_ms: 0,
            active_ms: 0,
            idle_ms: 0,
            segment_count: 0,
            donut: empty_donut(),
            top_apps: Vec::new(),
        });
    }

    let mut by_category: Vec<(String, i64)> = Vec::new();
    for row in conn.prepare(
        "SELECT category, SUM(end_at - start_at) AS ms FROM activities
         WHERE start_at >= ?1 AND start_at < ?2
         GROUP BY category",
    )?
    .query_map(rusqlite::params![start_ms, end_ms], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })? {
        by_category.push(row?);
    }

    let total_ms: i64 = by_category.iter().map(|(_, ms)| ms).sum();
    let idle_ms: i64 = by_category
        .iter()
        .filter(|(c, _)| c == "idle")
        .map(|(_, ms)| ms)
        .sum();

    // 四档恒定存在，缺的补 0 —— 前端不用处理「档位数量会变」。
    let donut: Vec<DonutSlice> = DONUT_KEYS
        .iter()
        .map(|key| DonutSlice {
            key: (*key).to_string(),
            ms: by_category
                .iter()
                .filter(|(cat, _)| donut_bucket(cat) == *key)
                .map(|(_, ms)| ms)
                .sum(),
        })
        .collect();

    let mut top_apps: Vec<AppSlice> = Vec::new();
    for row in conn.prepare(
        "SELECT application, SUM(end_at - start_at) AS ms FROM activities
         WHERE start_at >= ?1 AND start_at < ?2 AND application IS NOT NULL
         GROUP BY application ORDER BY ms DESC LIMIT ?3",
    )?
    .query_map(rusqlite::params![start_ms, end_ms, TOP_APPS_LIMIT], |r| {
        Ok(AppSlice {
            name: r.get::<_, String>(0)?,
            ms: r.get::<_, i64>(1)?,
        })
    })? {
        top_apps.push(row?);
    }

    let segment_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM activities WHERE start_at >= ?1 AND start_at < ?2",
        rusqlite::params![start_ms, end_ms],
        |r| r.get(0),
    )?;

    Ok(RangeTotals {
        total_ms,
        active_ms: total_ms - idle_ms,
        idle_ms,
        segment_count,
        donut,
        top_apps,
    })
}

/// 类别落在圆环的哪一档。`work` / `browsing` / `idle` 各归各位，
/// **其余全部并入 `unknown`**（含 `unknown` 自己）。
fn donut_bucket(category: &str) -> &'static str {
    match category {
        "work" => "work",
        "browsing" => "browsing",
        "idle" => "idle",
        _ => "unknown",
    }
}

/// 四档全 0。空范围时用。
fn empty_donut() -> Vec<DonutSlice> {
    DONUT_KEYS
        .iter()
        .map(|k| DonutSlice {
            key: (*k).to_string(),
            ms: 0,
        })
        .collect()
}
