use crate::ActivityContext;
use activity_core::{Event, EventType};

/// 把一个 Event 应用到上下文上。
///
/// 纯函数集合，**无状态**（spec §7.1：Context 是临时状态，不落库）。
pub struct ContextBuilder;

impl ContextBuilder {
    pub fn apply(ctx: &mut ActivityContext, event: &Event) {
        match &event.event_type {
            // 两个 payload 是不同类型，不能用 or-pattern 合并，只能分开处理
            EventType::WindowFocus(p) => {
                ctx.application = Some(p.process_name.clone());
                ctx.window_title = p.window_title.clone();
            }
            EventType::WindowTitleChange(p) => {
                ctx.application = Some(p.process_name.clone());
                ctx.window_title = p.window_title.clone();
            }
            EventType::InputHeartbeat(p) => {
                ctx.input_active = p.active_seconds > 0;
            }
            // idle 状态由 reduce 翻转后作为参数传进来；会话事件 Phase 1 不采集
            EventType::SystemIdle
            | EventType::SystemResume
            | EventType::SessionLock
            | EventType::SessionUnlock => {}
        }
    }
}

/// 由一个 Event 加上当前 idle/输入状态，算出"处理完这个 Event 之后"的完整上下文。
pub fn context_of(event: &Event, is_idle: bool, input_active: bool) -> ActivityContext {
    let mut ctx = ActivityContext {
        application: None,
        window_title: None,
        input_active,
        is_idle,
    };
    ContextBuilder::apply(&mut ctx, event);
    ctx
}
