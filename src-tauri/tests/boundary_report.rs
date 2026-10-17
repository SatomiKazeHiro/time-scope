//! 真实库的「段边界体检」。
//!
//! **不进 CI**：需要一台有真实数据的机器。它回答的是「改完引擎之后，真实数据
//! 变成什么样了」—— 用数字，不用眼睛：
//!
//! ```text
//! $env:TIME_SCOPE_DB   = "$env:APPDATA\time-scope\time-scope.db"
//! $env:TIME_SCOPE_DAYS = "2026-10-10,2026-10-04"   # 可选，默认全库
//! cargo test -p time-scope --test boundary_report -- --ignored --nocapture
//! ```
//!
//! 报告里三列是**断言**（红了就是引擎坏了），其余是读数：
//! - `可留短段`：短于 N 的段只允许「没有可并的邻居」——相接的邻居必须与它
//!   idleness 不同（idle 与非 idle 按设计不许互并，见 `normalize.rs`）
//! - `重叠对`：段两两不得相交（「一段时间只能有一个答案」）
//! - `位移m`：归一化前后**各分类**时长的最大变化（分钟）。它是归一化的代价，
//!   不是错误 —— 短段的时间归给了前一段
//!
//! 为什么要这个量具：2026-10-13 那一轮「读代码推断连错三次」，最后是靠参数
//! 扫描 + 真实数据重放定住的（STATUS §4.2）。以后动段边界的人先用它。

use activity_core::{Event, EventType};
use activity_engine::{reduce, EngineConfig, EngineState, RuleSet, DEFAULT_RULES_TOML};
use std::collections::BTreeMap;

/// 默认 N，与 `config.toml` 一致。测试里显式写出来，免得改默认值悄悄改了体检口径。
const N_MS: i64 = 30_000;

/// `(开始, 结束, 类别)`。类别只用来判 idle / 非 idle 与归类位移。
type Span = (i64, i64, &'static str);

fn db_path() -> Option<std::path::PathBuf> {
    std::env::var_os("TIME_SCOPE_DB").map(std::path::PathBuf::from)
}

/// 某个 Unix 毫秒所在**本地日**的零点。借 SQLite 换算，不在测试里复刻时区逻辑。
fn local_midnight(conn: &rusqlite::Connection, ms: i64) -> i64 {
    conn.query_row(
        "SELECT CAST(strftime('%s', date(?/1000, 'unixepoch', 'localtime') || ' 00:00:00', 'utc') AS INTEGER) * 1000",
        rusqlite::params![ms],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
}

fn days_in_db(conn: &rusqlite::Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT date(timestamp/1000,'unixepoch','localtime') AS d
             FROM events ORDER BY d ASC",
        )
        .expect("events 表");
    stmt.query_map([], |r| r.get::<_, String>(0))
        .expect("查询日期")
        .collect::<Result<Vec<_>, _>>()
        .expect("读日期")
}

fn events_of(conn: &rusqlite::Connection, start: i64, end: i64) -> Vec<Event> {
    activity_storage::get_events_in_range(conn, start, end)
        .expect("读事件")
        .iter()
        .map(|s| {
            let et: EventType = serde_json::from_str(&s.payload).expect("payload 必须可解析");
            Event::new(et, s.timestamp)
        })
        .collect()
}

/// 重放一天，返回 `(已定稿的段, 还在生长的当前段)`。
///
/// **当前段不算「已定稿」**：它还没关闭，归一化还没机会处理它 —— 拿它去做
/// 「短段必须被并」的断言会误报。但它要算进总时长，否则对不上。
fn replay(events: &[Event], cfg: &EngineConfig) -> (Vec<Span>, Option<(i64, i64)>) {
    let rules = RuleSet::from_toml(DEFAULT_RULES_TOML).expect("默认规则");
    let mut st = EngineState::initial();
    let mut out: Vec<Span> = Vec::new();
    for e in events {
        let o = reduce(&st, e, &rules, cfg);
        st = o.state;
        for s in o.closed_segments {
            out.push((s.start_at, s.end_at, s.category.as_str()));
        }
    }
    let open = st.current_segment.as_ref().map(|o| (o.start_at, o.end_at));
    // 缓冲里那一段也还没定稿（下一段到来时可能被并掉），但判断「谁挨着谁」必须
    // 算上它，否则最后一个定稿段会被误判成孤立的短段。
    if let Some(t) = st.tail.as_ref() {
        out.push((t.start_at, t.end_at, t.category.as_str()));
    }
    out.sort_unstable_by_key(|(s, _, _)| *s);
    (out, open)
}

fn total(segs: &[Span]) -> i64 {
    segs.iter().map(|(s, e, _)| e - s).sum()
}

fn median_ms(segs: &[Span]) -> i64 {
    let mut v: Vec<i64> = segs.iter().map(|(s, e, _)| e - s).collect();
    v.sort_unstable();
    v.get(v.len() / 2).copied().unwrap_or(0)
}

fn by_category(segs: &[Span]) -> BTreeMap<&'static str, i64> {
    let mut m: BTreeMap<&'static str, i64> = BTreeMap::new();
    for (s, e, c) in segs {
        *m.entry(*c).or_insert(0) += e - s;
    }
    m
}

fn is_idle(cat: &str) -> bool {
    cat == "idle"
}

#[test]
#[ignore]
fn boundary_report() {
    let Some(path) = db_path() else {
        eprintln!("跳过：设置 TIME_SCOPE_DB 指向 time-scope.db（应用运行中请连同 -wal/-shm 复制出来）");
        return;
    };
    let conn = activity_storage::open_file(&path).expect("打开数据库");
    let days: Vec<String> = match std::env::var("TIME_SCOPE_DAYS") {
        Ok(v) if !v.trim().is_empty() => v.split(',').map(|s| s.trim().to_string()).collect(),
        _ => days_in_db(&conn),
    };

    println!(
        "\n{:<12} {:>7} {:>7} {:>7} {:>7} {:>8} {:>7} {:>6}",
        "日期", "事件", "段数", "中位s", "<30s", "可留短段", "重叠对", "位移m"
    );

    let (mut bad_slivers, mut bad_overlap, mut bad_total) = (0, 0, 0);
    let mut max_shift_min = 0i64;

    for day in &days {
        let Ok(day_start) = conn.query_row(
            "SELECT CAST(strftime('%s', ? || ' 00:00:00','utc') AS INTEGER) * 1000",
            rusqlite::params![day],
            |r| r.get::<_, i64>(0),
        ) else {
            continue;
        };
        let midnight = local_midnight(&conn, day_start);
        let events = events_of(&conn, midnight, midnight + 86_400_000);
        if events.is_empty() {
            continue;
        }

        let (segs, open) = replay(&events, &EngineConfig::default());
        // 归一化关闭 = 原始分段，用来对总时长、算归类位移
        let (raw, raw_open) = replay(
            &events,
            &EngineConfig::default().with_min_segment_duration_s(0),
        );

        let total_diff = total(&segs) + open.map_or(0, |(s, e)| e - s)
            - total(&raw)
            - raw_open.map_or(0, |(s, e)| e - s);

        let slivers: Vec<Span> = segs.iter().copied().filter(|(s, e, _)| e - s < N_MS).collect();
        // 短段允许留下的唯一情形：**没有可并的邻居** —— 两侧都不相接，
        // 或者相接的邻居与它 idleness 不同（idle 与非 idle 按设计不许互并）。
        let acceptable = slivers
            .iter()
            .filter(|(s, e, cat)| {
                !segs.iter().any(|(s2, e2, cat2)| {
                    (e2 == s || s2 == e)
                        && !(s2 == s && e2 == e)
                        && is_idle(cat) == is_idle(cat2)
                })
            })
            .count();
        let overlap = segs.windows(2).filter(|w| w[0].1 > w[1].0).count();

        // 归类位移：归一化前后各分类时长的最大变化（分钟）
        let now = by_category(&segs);
        let before = by_category(&raw);
        let shift = now
            .keys()
            .chain(before.keys())
            .map(|c| {
                (now.get(c).copied().unwrap_or(0) - before.get(c).copied().unwrap_or(0)).abs()
            })
            .max()
            .unwrap_or(0);

        println!(
            "{:<12} {:>7} {:>7} {:>7} {:>7} {:>8} {:>7} {:>6}",
            day,
            events.len(),
            segs.len(),
            median_ms(&segs) / 1000,
            slivers.len(),
            acceptable,
            overlap,
            shift / 60_000
        );

        max_shift_min = max_shift_min.max(shift / 60_000);
        bad_slivers += slivers.len() - acceptable;
        bad_overlap += overlap;
        bad_total += i64::from(total_diff != 0);
    }

    println!("\n最大归类位移: {max_shift_min} 分钟/天 —— 归一化的代价，不是错误（见 normalize.rs）");
    assert_eq!(
        bad_slivers, 0,
        "有 {bad_slivers} 个短于 N 的段紧挨着可并的邻居却没被并进去 —— 归一化漏了"
    );
    assert_eq!(bad_overlap, 0, "有 {bad_overlap} 对重叠段 —— 引擎不该产出重叠");
    assert_eq!(
        bad_total, 0,
        "有 {bad_total} 天归一化前后的总时长不相等 —— 合并凭空造/丢了时间"
    );
}
