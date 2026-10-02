//! 常驻能力的装配点（spec §4）。
//!
//! 把 config、托盘、关窗、自启串在一起。
//! 决策逻辑都在可测的纯函数里；平台交互集中在 `install()`。

use crate::close_behavior::CloseDecision;
use crate::config::CloseBehavior;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use tauri::menu::CheckMenuItem;
use tauri::{AppHandle, Manager, WindowEvent};

/// 托管状态：托盘回调与关窗回调都要读它。
pub struct Residency {
    pub config_path: std::path::PathBuf,
    pub close_behavior: Mutex<CloseBehavior>,
    pub autostart: AtomicBool,
    pub tray_ok: AtomicBool,
    /// 自启勾选项，切换后要同步勾选态
    autostart_item: Mutex<Option<CheckMenuItem<tauri::Wry>>>,
}

/// 托盘不可用时必须能通过关窗退出（Review Focus #4），
/// 否则用户会卡在一个打不开、也关不掉的界面。
pub fn close_interception_when(behavior: CloseBehavior, tray_ok: bool) -> CloseDecision {
    if tray_ok {
        crate::close_behavior::decide(behavior, true)
    } else {
        CloseDecision::Quit
    }
}

/// 建托盘 + 装关窗拦截。返回托盘是否可用。
pub fn install(app: &mut tauri::App, cfg: crate::config::AppConfig) -> bool {
    let handle = app.handle().clone();
    let cfg = crate::autostart::setup(&handle, cfg);

    let state = Residency {
        config_path: crate::config::config_path(),
        close_behavior: Mutex::new(cfg.close_behavior),
        autostart: AtomicBool::new(cfg.autostart),
        tray_ok: AtomicBool::new(false),
        autostart_item: Mutex::new(None),
    };
    app.manage(state);

    let menu = build_tray_menu(&handle, cfg.autostart);
    let (menu, check_item) = match menu {
        Ok(m) => m,
        Err(e) => {
            // 托盘菜单都建不出来 = 托盘不可用。退回"关窗即退出"，
            // 保证用户至少还能用这个应用（spec §7）。
            eprintln!("[time-scope] 托盘菜单构建失败（{e}），退回「关窗即退出」");
            install_close_handler(&handle);
            return false;
        }
    };
    app.state::<Residency>()
        .autostart_item
        .lock()
        .unwrap()
        .replace(check_item);

    let tray_ok = tauri::tray::TrayIconBuilder::with_id("main")
        .tooltip("Time Scope")
        .icon(default_icon(&handle))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::DoubleClick { .. } = event {
                focus_window(tray.app_handle());
            }
        })
        .build(&handle)
        .is_ok();
    if !tray_ok {
        eprintln!("[time-scope] 托盘创建失败，退回「关窗即退出」以保证可用");
    }
    // 即使建图标失败，菜单事件也可能已经注册在上面；状态以实际结果为准
    app.state::<Residency>()
        .tray_ok
        .store(tray_ok, std::sync::atomic::Ordering::Relaxed);

    install_close_handler(&handle);
    tray_ok
}

fn default_icon(handle: &AppHandle) -> tauri::image::Image<'static> {
    // `default_window_icon()` 的借用 tied to &self，所以拷贝一份像素而不是 clone 引用
    match handle.default_window_icon() {
        Some(i) => tauri::image::Image::new_owned(i.rgba().to_vec(), i.width(), i.height()),
        None => tauri::image::Image::new_owned(vec![0u8; 32 * 32 * 4], 32, 32),
    }
}

type BuiltMenu = (
    tauri::menu::Menu<tauri::Wry>,
    CheckMenuItem<tauri::Wry>,
);

/// 按 `crate::tray::build_menu` 的结构建真菜单：id、顺序、勾选态都来自那份
/// 被单测钉住的数据，而不是在这里再写一遍。
fn build_tray_menu(handle: &AppHandle, autostart_on: bool) -> Result<BuiltMenu, String> {
    use crate::tray::{menu_item_id, TrayMenuItem};
    use tauri::menu::{CheckMenuItem, Menu, MenuItem};

    let (mut open_id, mut auto, mut quit_id) = (None, None, None);
    for item in crate::tray::build_menu(autostart_on) {
        match item {
            TrayMenuItem::Open => open_id = Some(menu_item_id(&item)),
            TrayMenuItem::Quit => quit_id = Some(menu_item_id(&item)),
            TrayMenuItem::ToggleAutostart { enabled } => auto = Some((menu_item_id(&item), enabled)),
        }
    }
    let (Some(open_id), Some((auto_id, auto_on)), Some(quit_id)) = (open_id, auto, quit_id) else {
        return Err("托盘菜单结构不完整".to_string());
    };

    let build = || -> tauri::Result<BuiltMenu> {
        let open = MenuItem::with_id(handle, open_id, "打开时间线", true, None::<&str>)?;
        let autostart = CheckMenuItem::with_id(
            handle,
            auto_id,
            "开机自启",
            true,
            auto_on,
            None::<&str>,
        )?;
        // 用普通 MenuItem 而不是 PredefinedMenuItem::quit：预定义项的行为由 Tauri
        // 决定，而我们要的是"显式 flush 后退出"这一条确定路径（spec §4.2）。
        let quit = MenuItem::with_id(handle, quit_id, "退出", true, None::<&str>)?;
        let menu = Menu::with_items(handle, &[&open, &autostart, &quit])?;
        Ok((menu, autostart))
    };
    build().map_err(|e| e.to_string())
}

fn on_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    use crate::tray::{on_menu_event as interpret, MenuOutcome};
    let autostart_now = app.state::<Residency>().autostart.load(std::sync::atomic::Ordering::Relaxed);
    match interpret(autostart_now, event.id().as_ref()) {
        MenuOutcome::FocusWindow => focus_window(app),
        MenuOutcome::SetAutostart(on) => {
            let ok = if on {
                crate::autostart::enable(app)
            } else {
                crate::autostart::disable(app)
            };
            if !ok {
                // 失败时**不改**内存与勾选态：菜单不能显示一个没生效的状态
                eprintln!("[time-scope] 修改开机自启失败，状态未变");
                return;
            }
            let state = app.state::<Residency>();
            state.autostart.store(on, std::sync::atomic::Ordering::Relaxed);
            if let Some(item) = state.autostart_item.lock().unwrap().as_ref() {
                let _ = item.set_checked(on);
            }
            // 只改 config 里那一行；失败只提示——系统状态已经生效，不该因此回滚。
            // 不整份重写：用户的注释与其他设置不该被我们抹掉。
            if let Err(e) = crate::config::patch_line(&state.config_path, "autostart", &on.to_string()) {
                eprintln!("[time-scope] {e}");
            }
        }
        MenuOutcome::Quit => app.exit(0),
        MenuOutcome::Ignored => {}
    }
}

fn install_close_handler(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let handle = app.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = handle.state::<Residency>();
                let behavior = *state.close_behavior.lock().unwrap();
                let tray_ok = state.tray_ok.load(std::sync::atomic::Ordering::Relaxed);
                match close_interception_when(behavior, tray_ok) {
                    CloseDecision::HideToTray => {
                        api.prevent_close();
                        if let Some(w) = handle.get_webview_window("main") {
                            let _ = w.hide();
                        }
                    }
                    CloseDecision::Ask => {
                        // 问一次：答案覆写 config，之后不再问（spec §4.2）
                        match ask_close_behavior(&handle) {
                            Some(CloseBehavior::Quit) => { /* 放行默认关闭 */ }
                            Some(other) => {
                                api.prevent_close();
                                remember_answer(&handle, other);
                            }
                            None => { /* 用户取消：什么都不做 */ }
                        }
                    }
                    CloseDecision::Quit => { /* 放行默认关闭 */ }
                }
            }
        });
    }
}

/// 记下用户的答案并写回 config。**写回失败不影响本次行为**（Review Focus #2）。
fn remember_answer(app: &AppHandle, answered: CloseBehavior) {
    let state = app.state::<Residency>();
    let next = crate::close_behavior::remember(answered);
    *state.close_behavior.lock().unwrap() = next;
    if next == CloseBehavior::Minimize {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.hide();
        }
    }
    // 写回只动一行；失败不阻止本次行为（Review Focus #2）
    if let Err(e) = crate::config::patch_line(
        &state.config_path,
        "close_behavior",
        &format!("\"{}\"", next.as_str()),
    ) {
        eprintln!("[time-scope] {e}");
    }
}

/// 关窗确认框的样式。抽成常量是为了能单测。
///
/// **故意不加 `MB_DEFBUTTON2`**（clippy 会因此报 `bad_bit_mask`）：
/// button 1「是」本来就是默认项，一旦加上 `MB_DEFBUTTON2`，回车就会变成
/// 「退出」——用户随手一按就把常驻应用关了。
#[cfg(windows)]
const CLOSE_PROMPT_STYLE: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE =
    windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE(
        windows::Win32::UI::WindowsAndMessaging::MB_YESNO.0
            | windows::Win32::UI::WindowsAndMessaging::MB_ICONQUESTION.0,
    );

#[cfg(windows)]
fn ask_close_behavior(_app: &AppHandle) -> Option<CloseBehavior> {
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDNO, IDYES};
    use windows::core::w;
    let r = unsafe {
        MessageBoxW(
            None,
            w!("关闭窗口后 Time Scope 会继续在后台记录。\n\n要最小化到托盘吗？\n选择「否」将退出程序。"),
            w!("Time Scope"),
            CLOSE_PROMPT_STYLE,
        )
    };
    // IDYES = 最小化；IDNO = 退出；其余（关掉对话框）= 什么都不做
    match r {
        IDYES => Some(CloseBehavior::Minimize),
        IDNO => Some(CloseBehavior::Quit),
        _ => None,
    }
}

#[cfg(not(windows))]
fn ask_close_behavior(_app: &AppHandle) -> Option<CloseBehavior> {
    None
}

pub fn focus_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failing_tray_must_not_leave_the_app_unclosable() {
        // Review Focus #4：托盘建不起来时若仍拦截关窗，用户就再也打不开界面
        assert_eq!(
            close_interception_when(CloseBehavior::Ask, false),
            CloseDecision::Quit,
            "托盘不可用时关窗必须直接退出"
        );
        assert_eq!(
            close_interception_when(CloseBehavior::Minimize, false),
            CloseDecision::Quit,
            "连「最小化到托盘」也不能用——没有托盘可停"
        );
    }

    #[test]
    fn a_working_tray_follows_the_configured_close_behavior() {
        assert_eq!(
            close_interception_when(CloseBehavior::Ask, true),
            CloseDecision::Ask
        );
        assert_eq!(
            close_interception_when(CloseBehavior::Minimize, true),
            CloseDecision::HideToTray
        );
        assert_eq!(
            close_interception_when(CloseBehavior::Quit, true),
            CloseDecision::Quit
        );
    }

    /// 防回归：确认框的回车默认项必须是「最小化」而不是「退出」——
    /// 随手一按回车就把常驻应用关掉，是最不该发生的那种误操作。
    ///
    /// 注意 `MB_DEFBUTTON1 == 0`（它是"什么都不加"的默认），所以**不能**断言
    /// "含有 MB_DEFBUTTON1"——那是个恒真断言，测不出任何东西。真正的判据是
    /// 样式里**没有** `MB_DEFBUTTON2`。
    #[test]
    fn the_close_prompt_defaults_to_minimizing_not_to_quitting() {
        use windows::Win32::UI::WindowsAndMessaging::{
            MB_DEFBUTTON2, MB_DEFBUTTON3, MB_DEFBUTTON4, MB_YESNO,
        };
        assert_eq!(CLOSE_PROMPT_STYLE.0 & MB_YESNO.0, MB_YESNO.0);
        assert_eq!(
            CLOSE_PROMPT_STYLE.0 & MB_DEFBUTTON2.0,
            0,
            "回车必须触发「是」= 最小化到托盘；加上 MB_DEFBUTTON2 就变成退出"
        );
        assert_eq!(CLOSE_PROMPT_STYLE.0 & MB_DEFBUTTON3.0, 0);
        assert_eq!(CLOSE_PROMPT_STYLE.0 & MB_DEFBUTTON4.0, 0);
    }

    /// 防回归：写进 config.toml 的值必须原样到达各层，而不是又回到硬编码默认。
    #[test]
    fn collector_gets_the_configured_thresholds() {
        use crate::config::AppConfig;
        // idle_threshold < MIN 时应被夹住，而不是原样传下去
        let cfg = AppConfig {
            idle_threshold_s: 0,
            heartbeat_every_s: 1,
            ..AppConfig::default()
        };
        let parsed = crate::config::parse(&cfg.to_toml());
        assert!(parsed.idle_threshold_s >= crate::config::MIN_IDLE_THRESHOLD_S);
        assert_eq!(parsed.heartbeat_every_s, 1);
        // 引擎侧拿到的是同一组值
        assert_eq!(
            parsed.to_engine_config().idle_threshold_s,
            parsed.idle_threshold_s as u64
        );
    }
}
