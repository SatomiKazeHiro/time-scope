use crate::classifier::RuleSet;
use crate::config::EngineConfig;
use crate::context::ContextBuilder;
use crate::normalize;
use crate::{ActivityContext, ActivitySegment, Category, OpenSegment, CLASSIFIER_RULE};
use activity_core::{Event, EventType};

/// 引擎的完整状态。**只有这个结构体是可变的**，其余全是纯函数。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EngineState {
    pub current_segment: Option<OpenSegment>,
    pub current_context: Option<ActivityContext>,
    pub is_idle: bool,
    /// 已关闭、但**还可能被下一个段并掉**的那一段。
    ///
    /// 归一化只需要一段前瞻：短段要并进前驱（前驱已知），或并进后继（等下一条）。
    /// 所以这里最多一个段 —— 不再是旧版那个需要保持时间序的 `pending` 列表，
    /// 也就没有「顺序被打乱 → 复活错误的那一段」（A1）那类问题。
    pub tail: Option<ActivitySegment>,
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
            active_ms: open.active_ms,
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

    // 0. 心跳的「有输入秒数」记到**当前正在生长的段**上（2026-10-13）。
    //    必须在下面切段逻辑**之前**做 —— 心跳不携带 application，
    //    它不会自己开段，但它是「用户确实在动」这件事唯一的直接证据。
    //    这段时长只作附加量，不碰任何边界（见 OpenSegment::active_ms）。
    if let EventType::InputHeartbeat(p) = &event.event_type {
        if p.active_seconds > 0 {
            if let Some(cur) = st.current_segment.as_mut() {
                // **按窗口与段的重叠算**（2026-10-13 修）。
                // `active_seconds` 说的是「心跳之前那个窗口里有几秒在动」，
                // 窗口起点由 `window_start_ms` 给出。只拿 `ts - start_at` 当上限
                // 会在段刚开时丢掉窗口前半段（落在段外）—— 实测心跳共报 16 秒、
                // 两个段总共只收到 9 秒，另一个拿到 0。
                //
                // 有输入的秒可以看成落在窗口里的任意位置，**保守取交集上界**：
                // 窗口 `[w0, ts]` 与段 `[start_at, …]` 重叠多少，
                // 报告的 `active_seconds` 最多也超不过这个重叠。
                //
                // **不要再拿 `end_at` 当上限**（2026-10-13 踩过两次）：
                // 心跳到来时 `extend` 还没跑，`end_at` 仍等于 `start_at`，
                // 用它一夹就把这一笔全丢了。段本来就会延长到 `ts`，
                // 算进去是对的。
                let w0 = if p.window_start_ms > 0 {
                    p.window_start_ms
                } else {
                    // 旧数据（2026-10-13 之前）没有窗口起点：
                    // 退化成「整段都算有输入」，宁可高估也别把已有数据说成没有。
                    ts - i64::from(p.active_seconds) * 1000
                };
                let overlap = (ts - w0.max(cur.start_at)).max(0);
                let claimed = i64::from(p.active_seconds) * 1000;
                cur.active_ms += claimed.min(overlap);
                // **总量不许超过段自身的墙钟时长**。每笔都夹了还不够：
                // 窗口之间可以重叠（两个心跳报的是同一段时间），
                // 累加起来会超过段有多长 —— 分母比它衡量的东西还大是荒谬的。
                // 2026-10-13 实测：四个各 10 秒的窗口累出 35 秒，而段只有 20 秒。
                cur.active_ms = cur.active_ms.min((ts - cur.start_at).max(0));
            }
        }
    }

    // 1. idle 状态翻转
    match event.event_type {
        // 锁屏/睡眠与"无输入"等价：那段时间用户不在，不该记成活跃（spec §3.1）
        EventType::SystemIdle | EventType::SessionLock => st.is_idle = true,
        EventType::SystemResume | EventType::SessionUnlock => st.is_idle = false,
        _ => {}
    }

    // 2. 新上下文：**沿用上一次的**再应用这个 Event。
    //    不能每次从空 context 起算——心跳事件不携带 application，
    //    重建会把 application 冲成 None，导致"同一应用"判定失败、每来一个心跳就切一段。
    let mut new_ctx = st.current_context.clone().unwrap_or_default();
    new_ctx.is_idle = st.is_idle;
    ContextBuilder::apply(&mut new_ctx, event);

    // 本次 reduce 里刚关闭的段：交给下面的归一化步骤，不直接落库
    let mut just_closed: Vec<ActivitySegment> = Vec::new();

    match event.event_type {
        // --- idle 分支 ---
        EventType::SystemIdle | EventType::SessionLock => {
            // 已经有一段 idle 时**不重开**：锁屏通知可能重复到达
            // （锁屏与合盖各发一次），每次都开新段会把一段连续的空闲切碎。
            match st.current_segment.clone() {
                Some(cur) if cur.category == Category::Idle => {
                    extend(&mut st, cur, ts, event.id.clone(), config);
                }
                _ => {
                    if let Some(seg) = close_current(&mut st, rules, ts, config) {
                        just_closed.push(seg);
                    }
                    let mut open = new_open_segment(ts, Category::Idle, None, 0.0);
                    open.evidence_event_ids.push(event.id.clone());
                    st.current_segment = Some(open);
                    st.current_context = Some(new_ctx);
                }
            }
        }
        EventType::SystemResume | EventType::SessionUnlock => {
            // idle 段在 resume 时收尾；下一条事件会开新的 context 段
            if let Some(seg) = close_current(&mut st, rules, ts, config) {
                just_closed.push(seg);
            }
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
                    // 没有已知应用的事件（心跳、会话事件）无法归因，不开段。
                    // 否则应用一启动就会被心跳堆出一个 "unknown / 无应用" 的幽灵段。
                    if new_ctx.application.is_none() {
                        // 什么都不开，只更新上下文
                    } else {
                        let mut open = new_open_segment(
                            ts,
                            category,
                            new_ctx.application.clone(),
                            cls.confidence,
                        );
                        open.evidence_event_ids.push(event.id.clone());
                        st.current_segment = Some(open);
                    }
                }
                Some(cur) => {
                    // 已经在 idle 段里时，心跳/焦点变化**只延长**它：
                    // 锁屏期间"正在用哪个程序"没有意义（spec §3.1），
                    // 让 application 参与判定会把一段连续的空闲切成碎片。
                    let same = (st.is_idle && cur.category == Category::Idle)
                        || (cur.application == new_ctx.application && cur.category == category);

                    // **空档守卫在这里生效**：跨过观测空档时，
                    // 当前段在自己的最后一条证据处收尾，这条事件另开一段。
                    // 放在 extend 之前判定，因为 extend 会把段用 `max(ts)`
                    // 拉到下一条事件 —— 那正是凭空造时间的入口。
                    let crossed_gap = crossed_observation_gap(cur.last_evidence_at, ts, config);
                    if same && !crossed_gap {
                        extend(&mut st, cur, ts, event.id.clone(), config);
                    } else {
                        if let Some(seg) = close_current(&mut st, rules, ts, config) {
                            just_closed.push(seg);
                        }
                        let mut open = new_open_segment(
                            ts,
                            category,
                            new_ctx.application.clone(),
                            cls.confidence,
                        );
                        open.evidence_event_ids.push(event.id.clone());
                        st.current_segment = Some(open);
                    }
                }
            }
            st.current_context = Some(new_ctx);
        }
    }

    // 3. 归一化：短段并进邻居、相邻同类段合并，攒够的段定稿放行。
    //
    // 这一步取代了旧版「等 hold_ms 的 pending 队列 + fold 短段合并」两段流水。
    // 现在只有一条缓冲（`st.tail`）与两条规则，规则本身住在 `normalize.rs`。
    let min_ms = (config.min_segment_duration_s as i64) * 1000;
    let mut closed: Vec<ActivitySegment> = Vec::new();
    if min_ms == 0 {
        // 0 = 关掉归一化（与旧语义一致：段一关闭就立即可落库）
        closed.append(&mut just_closed);
        if let Some(t) = st.tail.take() {
            closed.push(t);
        }
    } else {
        for seg in just_closed {
            normalize::settle(&mut st.tail, &mut closed, seg, min_ms);
        }
        // 把缓冲里的段并进正在生长的当前段：切回来（同应用）或前驱太短
        // 又没有更前面的段可并（并进后继）。必须在「超时释放」之前做。
        if let Some(open) = st.current_segment.as_mut() {
            normalize::absorb_tail_into_open(&mut st.tail, open, min_ms);
        }
        // 等不到后继的尾巴也要定稿，否则数据永远不落库
        normalize::release_if_stale(&mut st.tail, &mut closed, ts, min_ms);
    }
    closed.sort_by_key(|s| s.start_at);

    EngineOutput {
        state: st,
        closed_segments: closed,
    }
}

// ===========================================================================
// 段的时间边界：唯一的所有者
//
// 2026-10-13。空档守卫、重叠去重、切换次数同名那三轮修下来，根因是同一个：
// 段的 start_at / end_at 在本文件里有 6 处独立地写，每处自己决定往哪边取
// min/max。代码里早就写着「不可凭空造时间」，但守卫只加在 close_current 一处，
// 另外 5 处照样能违反它 —— 所以重叠查了三轮都查不完。
//
// **下面这两个函数是唯一允许改边界的地方。** 别的代码只能读 start_at/end_at。
// 要新增一条写边界的路径，先想清楚它属于哪一种。
//
// 两条不变式：
//   1. 段之间不得重叠（合并/复活都会撑大时间跨度，可能越过邻居）
//   2. end_at 不得超过「最后一条证据」所在的时间 —— 没有观测就没有时间
// ===========================================================================

/// 空档判定：`ts` 与最后一条证据相距超过阈值 ⇒ 这段时间**没有观测**。
///
/// 与 `max_observation_gap_s` 的语义是「没有观测」，不是「没有输入」
/// —— `idle_threshold_s` 管的是后者，两者不要合并。
pub fn crossed_observation_gap(last_evidence_at: i64, ts: i64, config: &EngineConfig) -> bool {
    ts.saturating_sub(last_evidence_at) > (config.max_observation_gap_s as i64) * 1000
}

/// 把段的 `end_at` 推到 `ts` —— **唯一**允许往后推边界的地方。
///
/// 跨过观测空档时不推，停在最后一条证据：那段时间没有事件，就没有时间。
/// 这就是「不可凭空造时间」的实现。
pub fn advance_end(end_at: &mut i64, last_evidence_at: i64, ts: i64, config: &EngineConfig) {
    let cap = if crossed_observation_gap(last_evidence_at, ts, config) {
        last_evidence_at
    } else {
        ts
    };
    *end_at = (*end_at).max(cap);
}

/// `[start, end)` 与 `[other_start, other_end)` 是否重叠。首尾相接不算。
pub fn ranges_overlap(start: i64, end: i64, other_start: i64, other_end: i64) -> bool {
    other_start < end && start < other_end
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
        active_ms: 0,
    }
}

/**
 * 观测空档守卫（2026-10-12 事故）。
 *
 * `ts` 与「上一条证据」相距超过 `max_observation_gap_s` 时，
 * **空档本身不产生任何段**：当前段在它自己的 `last_evidence_at` 处收尾，
 * 调用方（`reduce`）随后按新事件另开一段。
 *
 * 为什么必须在这里：原来的 `end_at = end_at.max(ts)` 把段一路拉到
 * 下一条事件的���间戳。应用退出 / 机器睡眠的整段时间里**一条事件都没有**，
 * 于是空白被当成同一应用的连续工作记下来 —— 一次停机凭空造出 11 小时。
 * 2026-10-12 那天因此被记成 18.29 小时 unknown，界面随之说谎
 * （24h 时段分布 18/24 根柱子满格、专注度大片 100%）。
 *
 * 与 `idle_threshold_s` 的区别：那条是「**没有输入**」（heartbeat 说用户没动，
 * 记成 idle）；这条是「**没有观测**」（压根没事件，可能是进程没跑），
 * 正确处理是**什么都不记**，而不是补一段 idle 或 unknown。
 *
 * 这个原则现在有两处守卫：这里（收尾）与 `normalize.rs`（合并只认首尾相接）。
 */
fn extend(
    st: &mut EngineState,
    mut cur: OpenSegment,
    ts: i64,
    evidence_id: String,
    config: &EngineConfig,
) {
    // 走唯一的收尾入口：跨过观测空档时**不推** end。
    // 原来这里是裸的 `max(ts)`，空档守卫只加在 close_current ——
    // 两条路径口径不一致，就是 6 处写边界各自为政的典型后果。
    advance_end(&mut cur.end_at, cur.last_evidence_at, ts, config);
    cur.last_evidence_at = ts;
    cur.evidence_event_ids.push(evidence_id);
    st.current_segment = Some(cur);
}

/// 收尾当前段，返回它（交给归一化器）；没有当前段时返回 `None`。
fn close_current(
    st: &mut EngineState,
    rules: &RuleSet,
    ts: i64,
    config: &EngineConfig,
) -> Option<ActivitySegment> {
    let mut open = st.current_segment.take()?;
    // 收尾以**最后一条证据**为准，而不是「这段一直活到 ts」。
    // `reduce` 里那些直接 `close_current` 的分支（切应用、resume）都过这里。
    advance_end(&mut open.end_at, open.last_evidence_at, ts, config);
    Some(ActivitySegment {
        id: format!("seg-{}", open.start_at),
        start_at: open.start_at,
        end_at: open.end_at,
        category: open.category,
        application: open.application,
        confidence: open.confidence,
        classifier: CLASSIFIER_RULE.to_string(),
        classifier_version: rules.version.clone(),
        evidence_event_ids: open.evidence_event_ids,
        active_ms: open.active_ms,
    })
}
