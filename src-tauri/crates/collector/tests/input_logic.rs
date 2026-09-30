use activity_collector::input::{heartbeat_bucket, idle_seconds, should_enter_idle, should_exit_idle};

#[test]
fn idle_seconds_computes_gap_in_whole_seconds() {
    assert_eq!(idle_seconds(10_000, 4_000), 6);
    assert_eq!(idle_seconds(5_000, 5_000), 0);
    assert_eq!(idle_seconds(6_500, 4_000), 2, "不足一秒的余数应截断");
}

#[test]
fn idle_seconds_never_saturates_because_wrapping_is_real() {
    // `dwTime` 与 `now` 来自同一个 GetTickCount，last 永远不可能晚于 now，
    // 所以“last 在未来”不是一种需要饱和处理的输入，而是 u32 回绕的另一种写法。
    // 因此这里断言的是回绕语义，而不是饱和到 0。
    let huge = idle_seconds(4_000, 10_000);
    assert!(
        huge > 4_000_000,
        "回绕后应得到一个很大的秒数（约 49 天），实际={huge}"
    );
}

#[test]
fn idle_seconds_survives_tick_count_wraparound() {
    // dwTime 是 u32 GetTickCount，每 ~49.7 天回绕一次。
    // 直接用 u64 相减会在回绕后算出天文数字（相当于"永不 idle"）。
    let now: u32 = 500; // 已回绕
    let last: u32 = u32::MAX - 999; // 回绕前
    // 真实的空闲时长约 1500ms
    assert_eq!(idle_seconds(now, last), 1);
}

#[test]
fn idle_threshold_boundary_uses_gte() {
    assert!(should_enter_idle(300, 300), "恰好等于阈值应判定为 idle");
    assert!(!should_enter_idle(299, 300));
    assert!(should_enter_idle(1_000, 300));
}

#[test]
fn should_exit_idle_only_when_already_idle() {
    assert!(should_exit_idle(true), "已 idle 且检测到输入 => 退出 idle");
    assert!(!should_exit_idle(false), "本来就不 idle，不该发 resume");
}

#[test]
fn heartbeat_bucket_clamps_to_window() {
    // active_seconds 是 u8；窗口是 10s，但计数逻辑若被改坏可能越界
    assert_eq!(heartbeat_bucket(0, 10), 0);
    assert_eq!(heartbeat_bucket(7, 10), 7);
    assert_eq!(heartbeat_bucket(255, 10), 10, "必须被夹到窗口大小");
}

#[test]
fn heartbeat_bucket_zero_when_window_is_zero() {
    assert_eq!(heartbeat_bucket(5, 0), 0, "窗口为 0 时不能除零");
}

/// 真实调用 GetLastInputInfo，验证 `cbSize` 陷阱没有踩中。
///
/// 这是 Task 8 唯一能自动验证的运行时行为：cbSize 没设对时 API 会失败、
/// dwTime 恒为 0，idle_seconds 永远是 0，SystemIdle 永远不会触发——
/// 而这个 bug 用纯函数单测永远发现不了。
#[test]
fn get_last_input_info_reports_a_plausible_tick() {
    use activity_collector::input::live_last_input_tick;
    let (ok, tick) = live_last_input_tick();
    assert!(ok, "GetLastInputInfo 失败——检查 LASTINPUTINFO.cbSize 是否设置");
    assert!(tick > 0, "dwTime 为 0——cbSize 大概率没设对");
    // dwTime 与 GetTickCount 同底数，差值应在一个合理范围内（不会超过开机时长）
    let now = unsafe { windows::Win32::System::SystemInformation::GetTickCount() };
    let gap = idle_seconds(now, tick);
    assert!(gap < 60 * 60 * 24, "tick 差值荒谬地大：{gap} 秒");
}
