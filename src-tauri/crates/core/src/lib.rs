#[cfg(test)]
pub mod tests;

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// 采集到的事实。不可变，永不修改。
///
/// spec §6：Event 是最底层事实，Activity 是可重算的解释。Event 里**没有** category /
/// confidence 之类的业务结论——那些属于 engine。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventType {
    /// 前台窗口切换
    WindowFocus(WindowFocusPayload),
    /// 同一窗口标题变化（浏览器切 Tab）
    WindowTitleChange(WindowTitleChangePayload),
    /// 无输入超过 idle_threshold
    SystemIdle,
    /// 从 idle 恢复
    SystemResume,
    /// 会话锁屏
    SessionLock,
    /// 会话解锁
    SessionUnlock,
    /// 一个心跳窗口内的活跃秒数
    InputHeartbeat(InputHeartbeatPayload),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowFocusPayload {
    pub process_name: String,
    /// 脱敏在写入前完成（spec §11）；骨架阶段原样存，后续 task 接入 `[[redact]]` 规则。
    pub window_title: Option<String>,
    pub exe_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowTitleChangePayload {
    pub process_name: String,
    pub window_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InputHeartbeatPayload {
    pub active_seconds: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub id: String,
    /// Unix 毫秒
    pub timestamp: i64,
    pub event_type: EventType,
}

impl Event {
    /// 构造一个带 id 的 Event。
    ///
    /// id 形如 `<16 hex timestamp><16 hex counter>`：等宽 32 字符、按字典序即时间序，
    /// 同一毫秒内靠进程级原子计数器区分。够用且零依赖；真 ULID 是后续 task 的事。
    pub fn new(event_type: EventType, timestamp: i64) -> Self {
        let c = COUNTER.fetch_add(1, Ordering::SeqCst);
        let id = format!("{:016x}{:016x}", timestamp as u64, c);
        Self {
            id,
            timestamp,
            event_type,
        }
    }
}

impl EventType {
    /// 与 serde 的 `rename_all = "snake_case"` 保持一致。
    ///
    /// storage 落库时用它填 `events.type` 列，查询侧就不必再解析一遍 JSON
    /// ——spec §8 的 `idx_events_type_timestamp` 索引建在这一列上。
    pub fn type_tag(&self) -> &'static str {
        match self {
            EventType::WindowFocus(_) => "window_focus",
            EventType::WindowTitleChange(_) => "window_title_change",
            EventType::SystemIdle => "system_idle",
            EventType::SystemResume => "system_resume",
            EventType::SessionLock => "session_lock",
            EventType::SessionUnlock => "session_unlock",
            EventType::InputHeartbeat(_) => "input_heartbeat",
        }
    }
}
