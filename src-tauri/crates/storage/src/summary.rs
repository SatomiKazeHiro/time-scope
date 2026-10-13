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
use crate::activity::SegmentSpan;

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
/// **跨零点的段按本地午夜切分到两天**（A2）。按 `start_at` 分组的话，
/// 23:57 起的段整段落给第一天，第二天那格是空的——于是「昨天最后一格」
/// 和「今天第一格」对不上，与时间线/汇总页看到的事实矛盾。
/// 切分保证：某一天的格子 ≡ `range_totals` 对那一天算出的 `total_ms`。
///
/// 两天用固定 86,400,000ms 递推，是跟随全项目既有的单固定 offset 假设
/// （前端 `+86_400_000`、`date_range::day_range_ms`、本文件的 `localtime` 同款）。
/// 夏令时切换日会差一小时，见 STATUS #16——本项目登记为已知取舍。
///
/// 库为空时返回 `Ok(None)`：那是「刚装完还没跑满一天」的真实状态，
/// 调用方据此显示空状态，而不是拿到一个 first/last 为空串的半成品。
pub fn daily_calendar(conn: &Connection) -> Result<Option<DailyCalendar>, rusqlite::Error> {
    // 递归 CTE 把「数据跨了哪几天」列出来，再把每个段与每一天求交集。
    // 锚点是最早那个段所在**本地日**的真零点：`date(...,'localtime')` 拿到本地
    // 日期标签，拼上 ` 00:00:00` 得到本地墙上时间，再用 `'utc'` 修饰符换算回
    // 真正的 Unix 时刻（少写 `'utc'` 会差一个 UTC offset）。
    // **不能用** `datetime(...,'localtime','start of day')` 再 `strftime('%s',...)`：
    // 那是把本地零点当 UTC 读，得到的不是零点而是「零点 + UTC offset」，
    // 与下面 `+86400000` 的递推对不齐，会让每天 00:00–offset 那段落不进任何格子。
    // 库为空时 span 的 lo/hi 都是 NULL，days 只产出一行 NULL，
    // JOIN 不上任何段，于是 days 为空 → 走下面的 `Ok(None)` 分支。
    let mut stmt = conn.prepare(
        "WITH RECURSIVE
             span AS (SELECT MIN(start_at) AS lo, MAX(end_at) AS hi FROM activities),
             days(day_ms) AS (
                 SELECT CAST(strftime('%s', date(lo/1000,'unixepoch','localtime')||' 00:00:00','utc') AS INTEGER) * 1000
                 FROM span
                 UNION ALL
                 SELECT day_ms + 86400000 FROM days, span WHERE day_ms + 86400000 < hi
             )
         SELECT date(days.day_ms/1000,'unixepoch','localtime') AS d,
                SUM(MIN(a.end_at, days.day_ms + 86400000) - MAX(a.start_at, days.day_ms)) AS ms
         FROM days
         JOIN activities a
           ON a.start_at < days.day_ms + 86400000
          AND a.end_at  > days.day_ms
         GROUP BY days.day_ms
         HAVING ms > 0
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

const TOP_APPS_LIMIT: usize = 5;

/// **一段只被计入它未被更早的段占用的时间。**
///
/// 输入必须已按 `start_at` 升序（`CLIPPED_SPANS_FROM` 保证）。
/// 维护一个「已占用的右边界」`covered_until`：本段只取 `[max(start, covered_until), end)`，
/// 非空时才算入，并把 `covered_until` 推到 `max(covered_until, end)`。
///
/// **为什么需要**：库里存在真实重叠的段（引擎侧成因另查，
/// 10-06 的 fc1c1bc 修过一条路径但没堵完）。逐段独立累加会把同一段时间
/// 算两次，于是 2026-10-12 的全天总时长是 **24.25 小时**。
///
/// **归属规则：先开始的段拥有重叠部分**（first-wins by start_at）。
/// 确定、不需要新数据、与时间线的阅读顺序一致（时间线也从左往右读）。
/// 实测重叠的两段**类别总是不同**（work×browsing 最多，17 处），
/// 所以这条规则是必须的 —— 不选就会把同一段时间记给两个类别。
///
/// 首尾相接（`prev.end_at == next.start_at`）**不是**重叠：
/// `covered_until` 取的是 `<=`，相接时 `max(start, covered) == start`，仍计满。
///
/// **被去重掉的量必须可见**（2026-10-13）。这里曾经只去重、不留痕 ——
/// 于是「库里又出现重叠」这件事是静默的：数字仍然对（去重了），但没人知道
/// 引擎又产出了坏数据，排查时看不出是「读侧在擦屁股」还是「引擎本来就这样」。
/// 那个静默让重叠问题被反复当成新 bug 查了三轮。
///
/// 现在每次真的发生重叠都往 stderr 打一行，带**被藏掉的毫秒数**。
/// 这是兜底的记账：读侧负责让数字正确，引擎负责不再产生，
/// 两者任一失效都会立刻在日志里显形。
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
        eprintln!(
            "[time-scope] activities 表存在重叠段：本次查询藏起 {hidden_pairs} 段 / {hidden_ms} ms。\
             时长已按「先开始的段拥有重叠部分」去重，数字是对的；\
             但这说明引擎产出了本不该有的重叠段（2026-10-08 修过一次，\
             见 segmenter 的 fold_short_segments open 分支的相邻检查）。\
             历史数据需重放才能修掉。"
        );
    }
    out
}

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

    // 按区间并集累加，不用 `SUM(MIN(end,?2)-MAX(start,?1)) GROUP BY category`：
    // 那个写法逐段独立相加，重叠的时间会被数两遍（全库 84 处重叠）。
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

    // top_apps 与 by_category 用**同一个去重结果**（`clip_to_uncovered` 已按
    // start_at 升序切好），否则两处会因重叠而对不上：类别按 first-wins 归属，
    // 应用也必须同样，否则「某应用时长」会与「它出现的那些档位之和」矛盾。
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

const HOUR_MS: i64 = 3_600_000;

/// 24h 活跃分布。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RangePacing {
    pub hourly_ms: [i64; 24],
}

///
/// `switch_count` 已删除（2026-10-13）：它数的是「相邻段之间空了 ≥5 分钟」，
/// 实测全库 0–4 次（2026-10-12 是 1 次），几乎没有信息量，且与紧邻的「N 段」
/// 说的是同一件事。它与时间线指标模式的「专注切换」（数相邻窗口变化，
/// 同日 48 次）**定义不同却同名**，用户会以为两边矛盾 —— 保留后者。
///
/// `offset_secs` 是本地 UTC 偏移的**秒数**，由 app 层从
/// `UtcOffset::whole_seconds()` 传进来 —— storage crate 不依赖 `time`。
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

    // 已按 start_at 升序（共用的时间线口径保证）。
    // 走专用的三列查询而不是 `get_segments_in_range`：后者对每个段都要再跑一次
    // evidence 查询（N+1），而这里一个字节都用不到（B1）。
    let segs = crate::activity::get_segment_spans_in_range(conn, start_ms, end_ms)?;

    // 24h 分布：逐段按本地小时边界劈开，跨小时的段分给两桶。
    //
    // **先去重再劈桶**（`clip_to_uncovered`）：逐段独立劈完累加会把重叠的时间
    // 劈进两次，那 18 小时的伪造段就是被这样撒成 18/24 根满格的柱子。
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

/// 范围内按**原始**字符串分组的窗口标题及其次数。
///
/// 只返回原始计数，**不做归一化** —— 归一化在 app 层（`title_norm.rs`），
/// 因为它要 `regex` 和脱敏占位符，两者都不属于 storage 的职责。
///
/// 两段式的原因：先在 SQL 里把 3.7M 行压成约 1000 行，再在 Rust 里归一化
/// 合并。反过来（先取全部行到内存再归一化）会把整库拉进进程。
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
    /// 占**未覆盖总时长**的比例，0–1。
    ///
    /// 分母刻意不是全天时长：用户打开这个列表要回答的是
    /// 「我漏配的时间里有多少给了它」，不是「我一天有多少给了它」。
    /// 用全天当分母的话，列表里每行都是个位数百分比，
    /// 「32% 未分类」的大头就消失在了分母里。
    pub share_of_uncovered: f64,
    pub segment_count: i64,
}

const DAY_MS: i64 = 86_400_000;

/// 最近 `days` 天里落进 `unknown` 的程序，按耗时降序。
///
/// 这份列表回答的是「**你的配置缺了什么**」，与汇总面板的「今天干了什么」
/// 是两件事 —— 曾在当日汇总里做过并被撤回，因为那会把一张配置表
/// 混进一个描述时间去向的面板。设置页是它的家。
///
/// **命中口径与 `range_totals` 同源**：`start_at < end AND end_at > start`
/// 的区间相交，起止**裁剪到窗口**。改这里必须同时改那里
/// （STATUS「别动这里」第 22 条）—— 不然同一天在时间线和这里会算出不同时长。
///
/// 没有未覆盖段时返回**空列表而不是 Err**：「全都覆盖了」是一个好状态，
/// 调用方据此显示空状态；返回 Err 会让界面把它当成出错来提示。
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
