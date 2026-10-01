use crate::classifier::RuleSet;
use crate::config::EngineConfig;
use crate::context::ContextBuilder;
use crate::{ActivityContext, ActivitySegment, Category, OpenSegment, CLASSIFIER_RULE};
use activity_core::{Event, EventType};

/// 引擎的完整状态。**只有这个结构体是可变的**，其余全是纯函数。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EngineState {
    pub current_segment: Option<OpenSegment>,
    pub current_context: Option<ActivityContext>,
    pub is_idle: bool,
    /// 已切掉但还没到"可落库年龄"的段。
    /// 同时是 grace 抖动时**可回溯复活**的来源——已落库的段无法复活。
    pub pending: Vec<ActivitySegment>,
    /// 上一次**真正**切段时离开的那个应用。spec §7.3 的"再前一个"就是它。
    ///
    /// 必须独立于 `pending`：`min_segment_duration = 0` 时 pending 会被立刻清空，
    /// 若拿它当"再前一个"，grace 判定就永远失效。
    pub previous_application: Option<String>,
    /// 上一次**真正**切段的时刻。被判为抖动的吸收不更新它。
    pub last_switch_at: Option<i64>,
}

impl EngineState {
    pub fn initial() -> Self {
        Self::default()
    }

    /// 把正在生长的段投影成一个可展示的 ActivitySegment。
    ///
    /// **它还没落库** —— `get_segments` 用它把"当前这一段"附在结果末尾，
    /// 时间线因此是实时的。
    pub fn open_segment_snapshot(
        &self,
        rules: &RuleSet,
        _config: &EngineConfig,
    ) -> Option<ActivitySegment> {
        let open = self.current_segment.as_ref()?;
        Some(ActivitySegment {
            id: format!("open-{}", open.start_at),
            start_at: open.start_at,
            end_at: open.end_at,
            category: open.category,
            application: open.application.clone(),
            confidence: open.confidence,
            classifier: CLASSIFIER_RULE.to_string(),
            classifier_version: rules.version.clone(),
            evidence_event_ids: open.evidence_event_ids.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EngineOutput {
    pub state: EngineState,
    /// 已够老、可以落库的段
    pub closed_segments: Vec<ActivitySegment>,
}

/// 逐个吞 Event，吐出可落库的 Segment。**无 IO、无副作用**（spec §7.4）。
pub fn reduce(
    state: &EngineState,
    event: &Event,
    rules: &RuleSet,
    config: &EngineConfig,
) -> EngineOutput {
    let mut st = state.clone();
    let ts = event.timestamp;

    // 1. idle 状态翻转
    match event.event_type {
        EventType::SystemIdle => st.is_idle = true,
        EventType::SystemResume => st.is_idle = false,
        _ => {}
    }

    // 2. 新上下文：**沿用上一次的**再应用这个 Event。
    //    不能每次从空 context 起算——心跳事件不携带 application，
    //    重建会把 application 冲成 None，导致"同一应用"判定失败、每来一个心跳就切一段。
    let mut new_ctx = st.current_context.clone().unwrap_or_default();
    new_ctx.is_idle = st.is_idle;
    ContextBuilder::apply(&mut new_ctx, event);

    match event.event_type {
        // --- idle 分支 ---
        EventType::SystemIdle => {
            close_current(&mut st, rules, ts);
            let mut open = new_open_segment(ts, Category::Idle, None, 0.0);
            open.evidence_event_ids.push(event.id.clone());
            st.current_segment = Some(open);
            st.current_context = Some(new_ctx);
            st.last_switch_at = Some(ts);
        }
        EventType::SystemResume => {
            // idle 段在 resume 时收尾；下一条事件会开新的 context 段
            close_current(&mut st, rules, ts);
            st.current_context = Some(new_ctx);
        }
        // --- context 分支 ---
        _ => {
            let cls = rules.classify(new_ctx.application.as_deref());
            let category = if st.is_idle {
                Category::Idle
            } else {
                cls.category
            };

            match st.current_segment.clone() {
                None => {
                    let mut open = new_open_segment(
                        ts,
                        category,
                        new_ctx.application.clone(),
                        cls.confidence,
                    );
                    open.evidence_event_ids.push(event.id.clone());
                    st.current_segment = Some(open);
                    st.last_switch_at = Some(ts);
                }
                Some(cur) => {
                    let same = cur.application == new_ctx.application
                        && cur.category == category;
                    if same {
                        extend(&mut st, cur, ts, event.id.clone());
                    } else if is_absorption(&st, &new_ctx, ts, config) {
                        absorb(&mut st, ts, event.id.clone());
                    } else {
                        close_current(&mut st, rules, ts);
                        st.previous_application = cur.application.clone();
                        let mut open = new_open_segment(
                            ts,
                            category,
                            new_ctx.application.clone(),
                            cls.confidence,
                        );
                        open.evidence_event_ids.push(event.id.clone());
                        st.current_segment = Some(open);
                        st.last_switch_at = Some(ts);
                    }
                }
            }
            st.current_context = Some(new_ctx);
        }
    }

    // 3. 释放够老的 pending 段
    let hold_ms = (config
        .min_segment_duration_s
        .max(config.grace_period_s) as i64)
        * 1000;
    let mut closed = Vec::new();
    let mut still = Vec::new();
    for seg in st.pending.drain(..) {
        if ts - seg.end_at >= hold_ms {
            closed.push(seg);
        } else {
            still.push(seg);
        }
    }
    st.pending = still;

    EngineOutput {
        state: st,
        closed_segments: closed,
    }
}

/// spec §7.3 的 grace 判定：距上次切换在宽限窗口内，**且新 context 与"再前一个"相同**。
///
/// 附加条件：前一段还留在 `pending` 里（没落库）。已落库的段无法复活，
/// 那时再判定为抖动也来不及了——落库门槛是 `max(min_segment_duration, grace_period)`，
/// 正好覆盖整个宽限窗口，所以正常情况下这个条件恒成立。
fn is_absorption(
    st: &EngineState,
    new_ctx: &ActivityContext,
    ts: i64,
    config: &EngineConfig,
) -> bool {
    if st.previous_application.as_deref() != new_ctx.application.as_deref() {
        return false;
    }
    let revisitable = st
        .pending
        .last()
        .map(|p| p.application == st.previous_application)
        .unwrap_or(false);
    if !revisitable {
        return false;
    }
    st.last_switch_at
        .map(|t| (ts - t).abs() < (config.grace_period_s as i64) * 1000)
        .unwrap_or(false)
}

/// 抖动吸收：丢弃刚开的那段（错误的 context），**复活** pending 末尾的前一段并延长。
///
/// 关键：如果只是"不切段"而把当前段留着，活下来的会是那个一闪而过的错误 context
/// （Code -> chrome(2s) -> Code 会留下一个 chrome 段），与用户实际经历不符。
fn absorb(st: &mut EngineState, ts: i64, evidence_id: String) {
    let Some(mut prev) = st.pending.pop() else {
        return;
    };
    let dropped = st.current_segment.take();

    prev.end_at = prev.end_at.max(ts);
    prev.evidence_event_ids.push(evidence_id);
    if let Some(d) = dropped {
        // 被丢弃段的证据并回来，别凭空消失
        prev.evidence_event_ids.extend(d.evidence_event_ids);
    }
    st.current_segment = Some(OpenSegment {
        start_at: prev.start_at,
        end_at: prev.end_at,
        category: prev.category,
        application: prev.application,
        confidence: prev.confidence,
        evidence_event_ids: prev.evidence_event_ids,
        last_evidence_at: ts,
    });
    // 抖动不是真切换：last_switch_at 与 previous_application 都保持不动
}

fn new_open_segment(
    ts: i64,
    category: Category,
    application: Option<String>,
    confidence: f32,
) -> OpenSegment {
    OpenSegment {
        start_at: ts,
        end_at: ts,
        category,
        application,
        confidence,
        evidence_event_ids: Vec::new(),
        last_evidence_at: ts,
    }
}

fn extend(st: &mut EngineState, mut cur: OpenSegment, ts: i64, evidence_id: String) {
    cur.end_at = cur.end_at.max(ts);
    cur.last_evidence_at = ts;
    cur.evidence_event_ids.push(evidence_id);
    st.current_segment = Some(cur);
}

fn close_current(st: &mut EngineState, rules: &RuleSet, ts: i64) {
    let Some(mut open) = st.current_segment.take() else {
        return;
    };
    open.end_at = open.end_at.max(ts);
    st.pending.push(ActivitySegment {
        id: format!("seg-{}", open.start_at),
        start_at: open.start_at,
        end_at: open.end_at,
        category: open.category,
        application: open.application,
        confidence: open.confidence,
        classifier: CLASSIFIER_RULE.to_string(),
        classifier_version: rules.version.clone(),
        evidence_event_ids: open.evidence_event_ids,
    });
}
