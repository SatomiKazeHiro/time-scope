//! 挂在前端轮询上的后台维护。
//!
//! 目前只有一件事：清理过期的原始事件（B3）。它**至多每 24 小时真的做一次**，
//! 所以不值得为它常驻一个线程 —— 这是最初把它挂在 `get_segments` 上的理由。
//!
//! 但它不该长在查询函数里：那样「前端多久轮询一次」就等于「多久检查一次清理」，
//! 而这件事和「取某一天的段」没有任何关系。改轮询节奏不该改变保留期的行为。
//! 所以它现在是一个具名的 tick，`get_segments` 开头叫它一声 ——
//! 依赖还在（这就是设计），但**看得见**。

use crate::retention;
use crate::state::AppState;

/// 每次轮询叫一次。真的清理按 `retention::prune_if_due` 内部的 24h 节流走。
pub fn tick(state: &AppState) {
    // 每次现读现用：用户在设置页改保留期后，下一次轮询就按新值清理。
    retention::prune_if_due(&state.conn, state.retention_days(), activity_storage::now_ms());
}
