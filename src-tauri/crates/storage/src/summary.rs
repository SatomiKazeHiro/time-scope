//! 汇总页的聚合查询。聚合全在这里，app 层只做时区换算与序列化。
//!
//! 按天分组用 SQL 的 `localtime` —— 依赖进程时区，夏令时切换日会差一小时。
//! 跟随全项目的单固定 offset 假设（`date_range::day_range_ms` 同款）：
//! 单独为汇总页修 DST 会制造「只有这一页对、其他页错」（spec §3.4）。

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use crate::activity::SegmentSpan;

/// 热力图的一格：某一天的监控总时长（含 idle）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayCell {
    /// 本地日 `YYYY-MM-DD`
    pub date: String,
    pub total_ms: i64,
}

/// 热力图的数据源。`days` 只含有记录的日期——补齐空缺是前端的活，
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

/// 全库的逐日监控时长。无参数 —— 热力图永远渲染全部数据（spec §2.3）。
///
/// 跨零点的段按本地午夜切到两天：按 `start_at` 分组会让 23:57 起的段整段
/// 落给第一天，第二天空一格，与时间线看到的事实矛盾。
///
/// 不变式：某天的格子恒等于 `range_totals` 对那天算出的 `total_ms`。
/// 两处都走 `clip_to_uncovered` 才成立 —— 之前各写一条 SQL，15 天里 5 天对不上。
///
/// 空库返回 `Ok(None)`（「刚装完还没跑满一天」），不给 first/last 为空的半成品。
pub fn daily_calendar(conn: &Connection) -> Result<Option<DailyCalendar>, rusqlite::Error> {
    // ① 本地日边界与日期标签交给 SQL（`localtime` 口径与全项目一致）。
    //    拼 ` 00:00:00` 再 `'utc'` 换回 Unix 时刻；少写 `'utc'` 会差一个 offset。
    //    别用 `datetime(...,'localtime','start of day')`：那是把本地零点当 UTC 读。
    let mut day_stmt = conn.prepare(
        "WITH RECURSIVE
             span AS (SELECT MIN(start_at) AS lo, MAX(end_at) AS hi FROM activities),
             days(day_ms) AS (
                 SELECT CAST(strftime('%s', date(lo/1000,'unixepoch','localtime')||' 00:00:00','utc') AS INTEGER) * 1000
                 FROM span
                 UNION ALL
                 SELECT day_ms + 86400000 FROM days, span WHERE day_ms + 86400000 < hi
             )
         SELECT day_ms, date(day_ms/1000,'unixepoch','localtime') FROM days
          WHERE day_ms IS NOT NULL
          ORDER BY day_ms",
    )?;
    let day_starts: Vec<(i64, String)> = day_stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if day_starts.is_empty() {
        return Ok(None);
    }

    // ② 裁剪走 `clip_to_uncovered`（与 `range_totals` 同一份实现）。
    //    要全库，所以给一个足够大的范围。
    let spans = crate::activity::get_segment_spans_in_range(conn, 0, i64::MAX)?;
    let clipped = clip_to_uncovered(&spans);

    // ③ 按本地日切分。`clipped` 已升序，每天从「还没结束的那一段」往后扫即可。
    let mut days: Vec<DayCell> = Vec::new();
    let mut cursor = 0usize;
    for (i, (day_ms, label)) in day_starts.iter().enumerate() {
        let day_end = day_starts
            .get(i + 1)
            .map(|(d, _)| *d)
            .unwrap_or(day_ms + 86_400_000);
        while cursor < clipped.len() && clipped[cursor].1 <= *day_ms {
            cursor += 1;
        }
        let mut total: i64 = 0;
        for (start, end, _cat, _app) in &clipped[cursor..] {
            if *start >= day_end {
                break;
            }
            let lo = (*start).max(*day_ms);
            let hi = (*end).min(day_end);
            if hi > lo {
                total += hi - lo;
            }
        }
        if total > 0 {
            days.push(DayCell {
                date: label.clone(),
                total_ms: total,
            });
        }
    }

    // 一行都没有 = 库空（或全被更早的段盖住）。不给半成品。
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

/// 圆环的一档。只有 4 种 key，前端的 `DonutChart` 按下标取色，
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

const TOP_APPS_LIMIT: usize = 5;

/// 一段只被计入它未被更早的段占用的时间（first-wins by start_at）。
///
/// 输入必须已按 `start_at` 升序。库里确实存在重叠的段，逐段累加会把同一段
/// 时间算两次（2026-10-12 那天算出全天 24.25 小时），所以必须裁。
///
/// 首尾相接不算重叠（`covered_until` 用 `<=`）。归属选「先开始者拥有」：
/// 确定、不需要新数据、与时间线的阅读方向一致。
///
/// 藏起来的量必须报出来（见 `OverlapReporter`）：静默去重时数字仍然对，
/// 问题就隐形了 —— 那让「重叠」被当成新 bug 查了三轮。
pub(crate) fn clip_to_uncovered(spans: &[SegmentSpan]) -> Vec<(i64, i64, &str, Option<&str>)> {
    let mut out = Vec::with_capacity(spans.len());
    let mut covered_until = i64::MIN;
    let mut hidden_ms: i64 = 0;
    let mut hidden_pairs = 0usize;
    for s in spans {
        let start = s.start_at.max(covered_until);
        if start < s.end_at {
            out.push((start, s.end_at, s.category.as_str(), s.application.as_deref()));
            covered_until = s.end_at;
        } else {
            // 整段被更早的段盖住（或零长度）—— 这只可能是重叠造成的数据缺陷
            hidden_ms += s.end_at - s.start_at;
            hidden_pairs += 1;
        }
    }
    if hidden_pairs > 0 {
        OverlapReporter::record(hidden_pairs, hidden_ms);
    }
    out
}

/// 重叠的节流汇报：累积总量，每分钟最多一行。
///
/// 前端每 5 秒轮询、一次刷新跑好几个查询，逐次报会把日志淹掉。
struct OverlapReporter {
    pairs: std::sync::atomic::AtomicU64,
    ms: std::sync::atomic::AtomicI64,
    last_printed: std::sync::Mutex<Option<std::time::Instant>>,
}

impl OverlapReporter {
    const MIN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

    fn record(pairs: usize, ms: i64) {
        use std::sync::atomic::Ordering;
        static R: std::sync::OnceLock<OverlapReporter> = std::sync::OnceLock::new();
        let r = R.get_or_init(|| OverlapReporter {
            pairs: Default::default(),
            ms: Default::default(),
            last_printed: std::sync::Mutex::new(None),
        });
        r.pairs.fetch_add(pairs as u64, Ordering::Relaxed);
        r.ms.fetch_add(ms, Ordering::Relaxed);

        let Ok(mut last) = r.last_printed.lock() else { return };
        let now = std::time::Instant::now();
        if last.is_some_and(|t| now.duration_since(t) < Self::MIN_INTERVAL) {
            return;
        }
        *last = Some(now);
        drop(last);
        eprintln!(
            "[time-scope] activities 表存在重叠段：累计藏起 {} 段 / {} ms。\
             时长已按「先开始的段拥有重叠部分」去重，数字是对的；\
             但这说明引擎产出了本不该有的重叠段（2026-10-08 修过一次，\
             见 segmenter 的 fold_short_segments open 分支的相邻检查）。\
             历史数据需重放才能修掉。同一情况每分钟最多报一次。",
            r.pairs.load(Ordering::Relaxed),
            r.ms.load(Ordering::Relaxed),
        );
    }
}

/// 区间内的时长三件套、段数、圆环、应用 Top。
///
/// 区间倒置不算错误，返回全 0 —— 公开 IPC 可能收到，让它炸没有好处。
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

    // 用并集累加，不用 `SUM(MIN(end,?2)-MAX(start,?1))`：那样逐段相加，重叠会数两遍。
    let spans = crate::activity::get_segment_spans_in_range(conn, start_ms, end_ms)?;
    let mut by_category: Vec<(String, i64)> = Vec::new();
    for (start, end, cat, _app) in clip_to_uncovered(&spans) {
        let ms = end - start;
        match by_category.iter_mut().find(|(c, _)| c == cat) {
            Some((_, acc)) => *acc += ms,
            None => by_category.push((cat.to_string(), ms)),
        }
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

    // top_apps 与 by_category 必须用同一个去重结果，否则重叠时「某应用时长」
    // 会和它出现在那些类别里的时长之和对不上。
    let mut by_app: Vec<(String, i64)> = Vec::new();
    for (start, end, _cat, app) in clip_to_uncovered(&spans) {
        let Some(app) = app else { continue };
        let ms = end - start;
        match by_app.iter_mut().find(|(a, _)| a == app) {
            Some((_, acc)) => *acc += ms,
            None => by_app.push((app.to_string(), ms)),
        }
    }
    by_app.sort_by_key(|(_, ms)| std::cmp::Reverse(*ms));
    let top_apps: Vec<AppSlice> = by_app
        .into_iter()
        .take(TOP_APPS_LIMIT)
        .map(|(name, ms)| AppSlice { name, ms })
        .collect();

    let segment_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM activities WHERE start_at < ?2 AND end_at > ?1",
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
/// 其余全部并入 `unknown`（含 `unknown` 自己）。
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

const HOUR_MS: i64 = 3_600_000;

/// 24h 活跃分布。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RangePacing {
    pub hourly_ms: [i64; 24],
}

///
/// `offset_secs` 由 app 层传（`UtcOffset::whole_seconds()`）—— storage 不依赖 `time`。
/// 原来的 `switch_count` 已删：与时间线的「专注切换」同名不同义，实测也没有信息量。
pub fn range_pacing(
    conn: &Connection,
    start_ms: i64,
    end_ms: i64,
    offset_secs: i32,
) -> Result<RangePacing, rusqlite::Error> {
    let mut hourly_ms = [0i64; 24];
    if start_ms >= end_ms {
        return Ok(RangePacing {
            hourly_ms,
        });
    }

    // 用三列查询而不是 `get_segments_in_range`：后者每段还要再查一次 evidence（N+1）。
    let segs = crate::activity::get_segment_spans_in_range(conn, start_ms, end_ms)?;

    // 先裁再劈小时：逐段劈完累加会把重叠的时间劈进两次（18 小时伪造段就是
    // 这么撒成 18/24 根满格的柱子）。
    for (start, end, cat, _app) in clip_to_uncovered(&segs) {
        if cat == "idle" {
            continue;
        }
        add_to_hours(&mut hourly_ms, start, end, offset_secs);
    }

    Ok(RangePacing { hourly_ms })
}

/// 把 `[start_ms, end_ms)` 按本地小时边界劈开，累加进 `buckets`。
fn add_to_hours(buckets: &mut [i64; 24], start_ms: i64, end_ms: i64, offset_secs: i32) {
    let shift = (offset_secs as i64) * 1000;
    let mut t = start_ms;
    while t < end_ms {
        let local = t + shift;
        // 到「当前这一小时」的末尾，不是到下一个整点起点
        let to_boundary = t + (HOUR_MS - local.rem_euclid(HOUR_MS));
        let seg_end = to_boundary.min(end_ms);
        buckets[local.div_euclid(HOUR_MS).rem_euclid(24) as usize] += seg_end - t;
        t = seg_end;
    }
}

/// 范围内按原始字符串分组的窗口标题与次数。
///
/// 不做归一化（在 `title_norm.rs`，它要 regex 与脱敏占位符）。两段式：
/// 先让 SQL 把 3.7M 行压到约 1000 行，再在 Rust 里合并 —— 反过来会把整库拉进内存。
pub fn title_counts_in_range(
    conn: &Connection,
    start_ms: i64,
    end_ms: i64,
) -> Result<Vec<(String, i64)>, rusqlite::Error> {
    if start_ms >= end_ms {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for row in conn.prepare(
        "SELECT json_extract(payload,'$.window_title') AS t, COUNT(*) AS n
         FROM events
         WHERE type IN ('window_focus','window_title_change')
           AND timestamp >= ?1 AND timestamp < ?2
           AND json_extract(payload,'$.window_title') IS NOT NULL
         GROUP BY t
         ORDER BY n DESC",
    )?
    .query_map(rusqlite::params![start_ms, end_ms], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })? {
        out.push(row?);
    }
    Ok(out)
}

/// 一个「有记录但没规则覆盖」的程序。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UncoveredProgram {
    /// 进程名（只有文件名部分，规则匹配也只比这个）
    pub application: String,
    pub total_ms: i64,
    /// 占未覆盖总时长的比例，0–1。
    ///
    /// 分母刻意不是全天：这个列表回答「我漏配的时间里有多少给了它」。
    /// 用全天当分母的话每行都是个位数百分比，大头就消失在分母里了。
    pub share_of_uncovered: f64,
    pub segment_count: i64,
}

const DAY_MS: i64 = 86_400_000;

/// 最近 `days` 天落进 `unknown` 的程序，按耗时降序。
///
/// 它回答「你的配置缺了什么」，与汇总面板的「今天干了什么」是两件事 ——
/// 曾在当日汇总里做过又被撤回。设置页是它的家。
///
/// 命中口径与 `range_totals` 同源（区间相交 + 裁剪到窗口）—— 改一处要同时
/// 改另一处，否则同一天在两处算出不同时长（STATUS 第 22 条）。
///
/// 没有未覆盖段时返回空列表而不是 Err：「全都覆盖了」是好状态。
pub fn uncovered_programs(
    conn: &Connection,
    now_ms: i64,
    days: u32,
) -> Result<Vec<UncoveredProgram>, rusqlite::Error> {
    let start_ms = now_ms - i64::from(days) * DAY_MS;
    if start_ms >= now_ms {
        return Ok(Vec::new());
    }

    let mut rows: Vec<(String, i64, i64)> = Vec::new();
    for row in conn.prepare(
        "SELECT application, SUM(MIN(end_at, ?2) - MAX(start_at, ?1)) AS ms, COUNT(*) AS n
         FROM activities
         WHERE category = 'unknown'
           AND start_at < ?2 AND end_at > ?1
           AND application IS NOT NULL AND application != ''
         GROUP BY application
         ORDER BY ms DESC",
    )?
    .query_map(rusqlite::params![start_ms, now_ms], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })? {
        rows.push(row?);
    }

    let uncovered_total: i64 = rows.iter().map(|(_, ms, _)| *ms).sum();
    Ok(rows
        .into_iter()
        .map(|(application, total_ms, segment_count)| UncoveredProgram {
            application,
            total_ms,
            // 分母为 0 时全给 0，而不是 NaN —— 界面要能直接画进度条
            share_of_uncovered: if uncovered_total > 0 {
                total_ms as f64 / uncovered_total as f64
            } else {
                0.0
            },
            segment_count,
        })
        .collect())
}
