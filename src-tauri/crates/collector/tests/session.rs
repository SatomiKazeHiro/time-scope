//! session 监听的纯逻辑部分。真实的 WTS/Power 注册与窗口消息必须人工验证，
//! 但"消息怎么翻译成信号"和"注册失败时会怎样"是行为，必须能自动验证。

use activity_collector::session::{
    registration_outcome, translate, RegistrationOutcome, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE,
};
use activity_collector::signals::RawSignal;

#[test]
fn a_successful_registration_reports_ok() {
    assert_eq!(registration_outcome(Ok(())), RegistrationOutcome::Ok);
}

#[test]
fn a_failed_registration_degrades_instead_of_panicking() {
    // Review Focus #4：常驻增强功能失败不能让主链路不可用
    let out = registration_outcome(Err("no window handle"));
    assert!(matches!(out, RegistrationOutcome::Failed { .. }));
}

#[test]
fn a_failed_registration_carries_a_reason_for_the_stderr_line() {
    match registration_outcome(Err("boom")) {
        RegistrationOutcome::Failed { reason } => assert!(!reason.is_empty()),
        other => panic!("不该成功: {other:?}"),
    }
}

#[test]
fn outcome_is_comparable_for_diagnostics() {
    assert_ne!(
        registration_outcome(Ok(())),
        registration_outcome(Err("x"))
    );
}

// --- 窗口消息 -> RawSignal（spec §3.1）---

#[test]
fn the_message_constants_match_win32() {
    // 自己写的常量必须和 Win32 的定义一致，否则注册了也收不到消息
    use windows::Win32::UI::WindowsAndMessaging::{WM_POWERBROADCAST as W, WM_WTSSESSION_CHANGE as S};
    assert_eq!(WM_WTSSESSION_CHANGE, S);
    assert_eq!(WM_POWERBROADCAST, W);
}

#[test]
fn a_lock_notification_becomes_a_lock_signal() {
    assert!(matches!(
        translate(WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK),
        Some(RawSignal::SessionLock)
    ));
    assert!(matches!(
        translate(WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK),
        Some(RawSignal::SessionUnlock)
    ));
}

#[test]
fn suspending_and_resuming_map_to_the_same_signals() {
    // 合盖 = 睡眠，对用户而言和锁屏是同一件事（spec §3.3）
    assert!(matches!(
        translate(WM_POWERBROADCAST, PBT_APMSUSPEND),
        Some(RawSignal::SessionLock)
    ));
    assert!(matches!(
        translate(WM_POWERBROADCAST, PBT_APMRESUME),
        Some(RawSignal::SessionUnlock)
    ));
}

#[test]
fn unrelated_session_notifications_are_ignored() {
    // WTS_SESSION_LOGON / LOGOFF / CONSENT 等都不该影响记账
    assert!(translate(WM_WTSSESSION_CHANGE, 0x5).is_none());
    // 电池变化、电源设置变化同理
    assert!(translate(WM_POWERBROADCAST, PBT_APMPOWERSTATUSCHANGE).is_none());
}

#[test]
fn other_window_messages_are_ignored() {
    assert!(translate(0x0001 /* WM_CREATE */, 0).is_none());
    assert!(translate(0x0002 /* WM_DESTROY */, 0).is_none());
}

const WTS_SESSION_LOCK: usize = 0x7;
const WTS_SESSION_UNLOCK: usize = 0x8;
const PBT_APMSUSPEND: usize = 0x0004;
const PBT_APMRESUME: usize = 0x0007;
const PBT_APMPOWERSTATUSCHANGE: usize = 0x000A;

// --- 两条注册通路必须互相独立（2026-10-13）---
//
// 真实事故：用户终端里打的是
//   [time-scope] PowerRegisterSuspendResumeNotification 失败：合盖/唤醒将不被记录
//   [time-scope] 锁屏/睡眠监听 不可用
//
// 源码里 `WTSRegisterSessionNotification`（锁屏/解锁）其实成功了，
// 但紧接着电源注册失败就 `return`，函数提前退出 → 消息泵从未启动 →
// 那个已经注册成功的 HWND 根本没人收消息。于是「电源失败」连坐「锁屏失效」。
//
// 两条通路是独立能力：合盖/唤醒没记录，不该让锁屏/解锁也没记录。

use activity_collector::session::SessionRegistration;

fn reg(wts_ok: bool, power_ok: bool) -> SessionRegistration {
    SessionRegistration { wts_ok, power_ok }
}

#[test]
fn wts_working_and_power_failing_still_records_lock_unlock() {
    let r = reg(true, false);
    assert!(
        r.lock_unlock_works(),
        "电源注册失败不该连坐锁屏 —— 那一路注册成功了且消息泵必须照常跑"
    );
    assert!(!r.power_works(), "电源确实没注册上");
    assert!(r.should_pump(), "有一路可用就必须跑消息泵");
    assert!(!r.is_fully_broken());
}

#[test]
fn wts_failing_and_power_working_still_records_lid_close_open() {
    let r = reg(false, true);
    assert!(r.power_works());
    assert!(!r.lock_unlock_works());
    assert!(r.should_pump());
    assert!(!r.is_fully_broken());
}

#[test]
fn both_failing_means_nothing_is_recorded_and_no_pump() {
    let r = reg(false, false);
    assert!(r.is_fully_broken(), "两路都失败才算完全不可用");
    assert!(!r.should_pump(), "两路都不可用时才不启动消息泵");
}

#[test]
fn both_working_is_the_normal_case() {
    let r = reg(true, true);
    assert!(r.lock_unlock_works() && r.power_works());
    assert!(r.should_pump());
    assert!(!r.is_fully_broken());
}
