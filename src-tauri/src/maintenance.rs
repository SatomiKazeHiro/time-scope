//! 挂在前端轮询上的后台维护：目前只有清理过期原始事件。
//!
//! 至多每 24 小时真做一次，不值得常驻线程，所以蹭轮询。但别让它在查询函数里
//! 悄悄发生 —— 那样改轮询节奏就等于改保留期行为；这里是个具名的 tick。

use crate::retention;
use crate::state::AppState;

/// 每次轮询叫一次。真的清理按 `retention::prune_if_due` 内部的 24h 节流走。
pub fn tick(state: &AppState) {
    // 每次现读现用：用户在设置页改保留期后，下一次轮询就按新值清理。
    retention::prune_if_due(&state.conn, state.retention_days(), activity_storage::now_ms());
}
