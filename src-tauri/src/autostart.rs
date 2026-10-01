//! 开机自启（spec §4.3）。
//!
//! **默认关闭**。一个会自动启动、自动读取窗口标题的程序，不该替用户做这个决定。
//!
//! 同步方向不对称是有意的：config 说要开/关，就照做（用户的意图优先）；
//! 系统状态与 config 不一致但 config 没要求变更时，把**系统状态写回 config**，
//! 这样托盘菜单的勾选态显示的是真实情况，而不是我们自己以为的情况。

use crate::config::AppConfig;
use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDirection {
    /// 让系统去匹配 config
    ToSystem,
    /// 让 config 去匹配系统
    ToConfig,
}

/// 纯函数：启动时该往哪边同步。
pub fn sync(enabled_in_config: bool, enabled_in_system: bool) -> SyncDirection {
    if enabled_in_config == enabled_in_system {
        SyncDirection::ToConfig
    } else {
        SyncDirection::ToSystem
    }
}

/// 系统里的自启状态。读不到就当"关"——宁可少一个功能。
pub fn is_enabled(app: &AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

/// 启用自启。返回是否成功（失败时调用方要提示，不要假装成功）。
pub fn enable(app: &AppHandle) -> bool {
    app.autolaunch().enable().is_ok()
}

pub fn disable(app: &AppHandle) -> bool {
    app.autolaunch().disable().is_ok()
}

/// 启动时同步一次，返回**同步后**的配置（自启状态已按系统实际值修正）。
pub fn setup(app: &AppHandle, mut cfg: AppConfig) -> AppConfig {
    match sync(cfg.autostart, is_enabled(app)) {
        SyncDirection::ToSystem => {
            let ok = if cfg.autostart { enable(app) } else { disable(app) };
            if !ok {
                eprintln!(
                    "[time-scope] 未能按配置{}开机自启，本次以系统实际状态为准",
                    if cfg.autostart { "启用" } else { "关闭" }
                );
                cfg.autostart = is_enabled(app);
            }
        }
        SyncDirection::ToConfig => {
            cfg.autostart = is_enabled(app);
        }
    }
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mismatch_pushes_the_system_towards_the_config() {
        // Review Focus #5：用户可能在 Windows「启动应用」里手动改过
        assert_eq!(sync(false, true), SyncDirection::ToSystem, "系统有、配置无 -> 关掉系统的");
        assert_eq!(sync(true, false), SyncDirection::ToSystem, "配置有、系统无 -> 打开系统的");
    }

    #[test]
    fn a_match_writes_the_system_state_into_the_config() {
        // 托盘勾选态要反映真实系统状态
        assert_eq!(sync(true, true), SyncDirection::ToConfig);
        assert_eq!(sync(false, false), SyncDirection::ToConfig);
    }

    #[test]
    fn default_is_off_so_nothing_starts_without_being_asked() {
        // 用户没主动开过自启，系统里就不该有我们的条目
        assert_eq!(sync(false, true), SyncDirection::ToSystem);
        assert!(!AppConfig::default().autostart);
    }
}
