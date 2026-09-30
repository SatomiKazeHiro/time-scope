use time::macros::format_description;
use time::{Date, PrimitiveDateTime, Time, UtcOffset};

/// `YYYY-MM-DD`。time 0.3.55 的 `well_known` 里没有 Date（只有 Rfc3339/Rfc2822/
/// Iso8601），所以用宏构造一个编译期常量描述符。
const YMD: &[time::format_description::BorrowedFormatItem<'_>] =
    format_description!("[year]-[month]-[day]");

/// 把 `YYYY-MM-DD` 转成本地时区下该日的半开区间 `[00:00, 次日00:00)` 的 Unix 毫秒。
///
/// Review Focus #2：应用 24h 常驻，跨零点后查询"今天"必须落在新日期内，
/// 而昨天已落库的数据仍应能查出来——这要求端点用本地时区且是半开区间。
///
/// `offset` 由调用方传入而不是内部读取：单测要能固定时区，不依赖运行机器的设置。
pub fn day_range_ms(date: &str, offset: UtcOffset) -> Result<(i64, i64), String> {
    let d = Date::parse(date, YMD).map_err(|e| format!("bad date {date}: {e}"))?;
    // time 0.3 的 `next_day` 返回 Option（None = 撞上 Date::MAX）
    let next = d
        .next_day()
        .ok_or_else(|| format!("date out of range: {date}"))?;
    let start = PrimitiveDateTime::new(d, Time::MIDNIGHT).assume_offset(offset);
    let end = PrimitiveDateTime::new(next, Time::MIDNIGHT).assume_offset(offset);
    Ok((start.unix_timestamp() * 1000, end.unix_timestamp() * 1000))
}

/// 取当前本地 UTC 偏移。失败（跨 DST 边界的歧义时间等）时向上抛。
pub fn local_offset() -> Result<UtcOffset, String> {
    UtcOffset::current_local_offset().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::day_range_ms;
    use time::macros::offset;
    use time::UtcOffset;

    // time_macros 的 offset! 不接受补零的 "+08:00"（会报 unexpected token）
    const CST: UtcOffset = offset!(+8);
    const EST: UtcOffset = offset!(-5);

    #[test]
    fn range_is_24h_in_fixed_offset() {
        let (s, e) = day_range_ms("2026-10-01", CST).unwrap();
        assert_eq!(e - s, 86_400_000);
    }

    #[test]
    fn start_matches_local_midnight() {
        let (s, _e) = day_range_ms("2026-10-01", CST).unwrap();
        // 2026-10-01T00:00+08:00 == 2026-09-30T16:00:00Z == 1790784000 秒
        assert_eq!(s, 1_790_784_000_000);
    }

    #[test]
    fn end_is_next_day_midnight_not_inclusive() {
        let (s, e) = day_range_ms("2026-10-01", CST).unwrap();
        let (s2, _e2) = day_range_ms("2026-10-02", CST).unwrap();
        assert_eq!(e, s2, "一天的终点必须等于下一天的起点");
        assert!(e > s);
    }

    #[test]
    fn offset_shifts_window_without_changing_length() {
        let (s8, e8) = day_range_ms("2026-10-01", CST).unwrap();
        let (s0, e0) = day_range_ms("2026-10-01", UtcOffset::UTC).unwrap();
        let (s_5, e_5) = day_range_ms("2026-10-01", EST).unwrap();
        // 固定偏移下每天都是 24h
        assert_eq!(e8 - s8, e0 - s0);
        assert_eq!(e_5 - s_5, e0 - s0);
        // 东八区的一天比 UTC 的一天早 8h 开始（本地 00:00 == 前一天 16:00Z）
        assert_eq!(s8 - s0, -8 * 3_600_000);
        // 西五区则晚 5h 开始（本地 00:00 == 当天 05:00Z）
        assert_eq!(s_5 - s0, 5 * 3_600_000);
    }

    #[test]
    fn leap_day_is_accepted() {
        let (s, e) = day_range_ms("2028-02-29", CST).unwrap();
        assert_eq!(e - s, 86_400_000, "闰日本身也是 24h");
        // 闰日之后直接接到 3-01，不多不少一天
        let (s3, _e3) = day_range_ms("2028-03-01", CST).unwrap();
        assert_eq!(e, s3);
    }

    #[test]
    fn year_boundary_rolls_over() {
        let (_s, e) = day_range_ms("2026-12-31", CST).unwrap();
        let (s, _e2) = day_range_ms("2027-01-01", CST).unwrap();
        assert_eq!(e, s);
    }

    #[test]
    fn invalid_dates_are_rejected() {
        assert!(day_range_ms("2026-13-01", CST).is_err());
        assert!(day_range_ms("2026-02-30", CST).is_err());
        assert!(day_range_ms("2026-00-10", CST).is_err());
        assert!(day_range_ms("2027-02-29", CST).is_err(), "2027 不是闰年");
        assert!(day_range_ms("not-a-date", CST).is_err());
        assert!(day_range_ms("2026-1-1", CST).is_err(), "必须是补零的 YYYY-MM-DD");
        assert!(day_range_ms("", CST).is_err());
    }

    #[test]
    fn max_date_is_rejected_rather_than_panicking() {
        // next_day() 会失败，必须变成 Err 而不是 panic
        assert!(day_range_ms("+999999-12-31", CST).is_err());
    }
}
