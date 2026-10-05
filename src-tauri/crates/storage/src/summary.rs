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
