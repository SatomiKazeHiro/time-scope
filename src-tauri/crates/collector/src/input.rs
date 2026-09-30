//! 输入活动与空闲检测（spec §5.3）。
//!
//! 每 1s 轮询一次 `GetLastInputInfo`，**状态变化才发信号**——不轮询写库。
//!
//! Win32 两个坑（spec §15 风险表）：
//! 1. `LASTINPUTINFO.cbSize` 必须显式设置，否则 API 直接失败、`dwTime` 恒为 0，
//!    表现为 idle_seconds 永远是 0、永远不会触发 SystemIdle。
//! 2. `dwTime` 是 **u32** 的 `GetTickCount`，每 ~49.7 天回绕；拿它和 u64 的
//!    `GetTickCount64` 直接相减，回绕后会算出天文数字，等于"永不 idle"。

use crate::signals::RawSignal;
use activity_core::{Event, EventType, InputHeartbeatPayload};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

static RUNNING: AtomicBool = AtomicBool::new(false);

/// 距上次输入的秒数。纯函数，抽出以便单测。
///
/// 用 u32 的环绕减法：`(now - last) % 2^32` 正好是 Win32 自己计算 tick 差的
/// 方式，回绕后仍然正确。
pub fn idle_seconds(now_tick: u32, last_input_tick: u32) -> u32 {
    let delta_ms = now_tick.wrapping_sub(last_input_tick);
    (delta_ms / 1000) as u32
}

/// 是否应该进入 idle。阈值语义是 `>=`（spec §5.3）。
pub fn should_enter_idle(idle_s: u32, threshold_s: u32) -> bool {
    idle_s >= threshold_s
}

/// 是否应该发 SystemResume。只在"确实处于 idle 且检测到输入"时发，
/// 否则会产生成吨的假 resume（spec §5.4：状态变化才产 Event）。
pub fn should_exit_idle(was_idle: bool) -> bool {
    was_idle
}

/// 把一个窗口内的活跃秒数夹进 `[0, window]`，再转成 u8。
/// 防的是计数逻辑被改坏后 `as u8` 回绕成很大的值。
pub fn heartbeat_bucket(active_s: u32, window_s: u32) -> u8 {
    active_s.min(window_s) as u8
}

#[cfg(windows)]
fn last_input_tick() -> u32 {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, // 不设则恒返回 0
        dwTime: 0,
    };
    if unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        info.dwTime
    } else {
        0
    }
}

#[cfg(windows)]
fn now_tick() -> u32 {
    // 必须用 32 位的 GetTickCount（低 32 位），才能和 dwTime 同底数做环绕减法。
    unsafe { windows::Win32::System::SystemInformation::GetTickCount() }
}

/// 启动输入轮询线程。
/// * `idle_threshold_s` — 无输入多少秒后判定空闲（spec 默认 300）
/// * `heartbeat_every_s` — 心跳聚合窗口（spec 默认 10）
pub fn spawn_input_poller(sender: Sender<RawSignal>, idle_threshold_s: u32, heartbeat_every_s: u32) {
    RUNNING.store(true, Ordering::SeqCst);
    std::thread::spawn(move || {
        #[cfg(not(windows))]
        {
            let _ = (sender, idle_threshold_s, heartbeat_every_s);
            return;
        }
        #[cfg(windows)]
        {
            let mut was_idle = false;
            let mut active_in_window: u32 = 0;
            let mut last_tick = last_input_tick();
            let mut window_start_tick = now_tick();
            let heartbeat_ms = heartbeat_every_s.saturating_mul(1000);

            while RUNNING.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_secs(1));
                let now = now_tick();
                let last = last_input_tick();
                let had_input = last != last_tick;

                if had_input {
                    // dwTime 变化 => 这一秒内有输入
                    last_tick = last;
                    active_in_window += 1;
                    if should_exit_idle(was_idle) {
                        was_idle = false;
                        let _ = sender.send(RawSignal::InputActive);
                    }
                }

                if !was_idle && should_enter_idle(idle_seconds(now, last), idle_threshold_s) {
                    was_idle = true;
                    let _ = sender.send(RawSignal::IdleStart);
                }

                // 心跳窗口到了就发一条，idle 期间 active_seconds 为 0
                if now.wrapping_sub(window_start_tick) >= heartbeat_ms {
                    let _ = sender.send(RawSignal::Heartbeat(heartbeat_bucket(
                        active_in_window,
                        heartbeat_every_s,
                    )));
                    active_in_window = 0;
                    window_start_tick = now;
                }
            }
        }
    });
}

pub fn stop_input_poller() {
    RUNNING.store(false, Ordering::SeqCst);
}

/// 暴露给集成测试的包装：`(调用是否成功, dwTime)`。
/// 用来在运行时验证 cbSize 没踩坑（纯函数单测验证不了这一点）。
pub fn live_last_input_tick() -> (bool, u32) {
    #[cfg(windows)]
    {
        (true, last_input_tick())
    }
    #[cfg(not(windows))]
    {
        (false, 0)
    }
}

pub fn make_idle_event(ts: i64) -> Event {
    Event::new(EventType::SystemIdle, ts)
}

pub fn make_resume_event(ts: i64) -> Event {
    Event::new(EventType::SystemResume, ts)
}

pub fn make_heartbeat_event(active_seconds: u8, ts: i64) -> Event {
    Event::new(
        EventType::InputHeartbeat(InputHeartbeatPayload { active_seconds }),
        ts,
    )
}
