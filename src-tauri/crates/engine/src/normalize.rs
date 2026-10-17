//! 段边界归一化 —— 「一个段至少多长」的唯一实现。
//!
//! 2026-10-17。这里取代了**两套**机制：grace 抖动吸收（`absorb` 系列）与
//! `fold_short_segments`。它们做的是同一件事（一闪而过不该成为一个段），只是
//! 作用时刻不同，各自维护「哪些段还能改」的状态 —— A1 / B12 那两次重叠事故
//! 都出在这套状态上（`pending` 的顺序、`previous_application` 的语义）。
//!
//! 现在只有一条缓冲（`tail`：**已关闭、但还可能在下一个段到来时被并掉的那一段**）
//! 和两条规则，N = `min_segment_duration_s`（默认 30 秒）：
//!
//! 1. **相邻且同应用同类**的段合成一段；
//! 2. **短于 N** 的段并进紧邻的**前驱**；没有前驱就并进紧邻的**后继**。
//!
//! 两条都必须「首尾相接」，所以永远不会凭空造出时间；两条都不跨
//! idle / 非 idle 的边界（把空闲并进工作就是说谎）。
//!
//! **「并进前驱」意味着那一小段时间归给前一段的类别与应用。** 这是有意的取舍：
//! 一个 2 秒的往返不足以代表一段活动，把它算进它打断的那一段，比在时间线上留
//! 一根 2px 的刺更接近当事人的记忆。代价是归属会挪动 —— 真实数据上约 1%。
//!
//! 还有第三条动作 `reopen`：切回来时**把 tail 重新长回当前段**。没有它，
//! `Code → chrome(2s) → Code` 会留下「Code(0..4s) 闭合段 + Code(4s..) 开段」
//! 两根紧挨着的同应用段 —— 那正是旧代码用 absorb 想避免的形态。

use crate::{ActivitySegment, Category, OpenSegment};

fn same_idleness(a: Category, b: Category) -> bool {
    (a == Category::Idle) == (b == Category::Idle)
}

fn span_ms(s: &ActivitySegment) -> i64 {
    s.end_at - s.start_at
}

/// 把 `extra` 的时间与证据并进 `target`（`target` 的身份不变）。
fn absorb_into(target: &mut ActivitySegment, extra: &ActivitySegment) {
    target.end_at = target.end_at.max(extra.end_at);
    target.start_at = target.start_at.min(extra.start_at);
    target
        .evidence_event_ids
        .extend(extra.evidence_event_ids.iter().cloned());
    // 两段的有输入时长相加；上限是合并后的墙钟跨度（分母不许比它衡量的东西大）
    target.active_ms = target
        .active_ms
        .saturating_add(extra.active_ms)
        .min(span_ms(target).max(0));
}

/// 把刚关闭的段 `c` 交给归一化器。
///
/// `tail` 是唯一还能被改的段；`out` 收的是**定稿**的段（可以落库）。
///
/// 只管「并进**前驱**」这一半。并进**后继**要等后继出现，而那时当前段已经在
/// 生长了 —— 那一步在 [`absorb_tail_into_open`] 里做。分成两处是必须的：
/// 缓冲会在「满 N 秒没有新段」时被释放，若把「后继」那半边留到这里，
/// 一个持续几十分钟的后继会让缓冲先被超时放走（2026-10-17 实测：每天几十个
/// 短段就是这么漏下来的）。
pub(crate) fn settle(
    tail: &mut Option<ActivitySegment>,
    out: &mut Vec<ActivitySegment>,
    c: ActivitySegment,
    min_ms: i64,
) {
    let Some(t) = tail.as_mut() else {
        *tail = Some(c);
        return;
    };

    let touching = c.start_at <= t.end_at;
    let compatible = same_idleness(t.category, c.category);
    let same_key = t.application == c.application && t.category == c.category;
    let c_is_short = span_ms(&c) < min_ms;
    // 时钟回退的守卫：新段的结束**早于**前一段的结束 → 不许并。
    // 那说明时间戳倒着走（NTP 跳变、手工改系统时间），并了就会用一小段
    // 替换掉一段真实时间，被替换的部分从账本上整个消失（B10 的教训）。
    let grows = c.end_at >= t.end_at;

    // 规则 ①：相邻且同应用同类 —— 同一次活动被切成了两段（规则换了/标题变了）
    // 规则 ②：新段太短 —— 并进前驱。前驱就是「它打断的那一段」。
    if touching && compatible && grows && (same_key || c_is_short) {
        absorb_into(t, &c);
        return;
    }

    // 两条都不成立：前一段定稿，新段进缓冲等它的后继。
    out.push(tail.take().expect("上面已确认 tail 存在"));
    *tail = Some(c);
}

/// 等不到后继就别一直压着：`tail` 结束之后满 N 秒没有新段，它就该定稿。
///
/// 这条替代了旧代码里 `hold_ms = max(min, grace)` 与「等三轮就认了」的
/// `give_up_ms` 两个魔法数 —— 现在只有一个数，就是 N 本身。
pub(crate) fn release_if_stale(
    tail: &mut Option<ActivitySegment>,
    out: &mut Vec<ActivitySegment>,
    now: i64,
    min_ms: i64,
) {
    if let Some(t) = tail.as_ref() {
        if now.saturating_sub(t.end_at) >= min_ms {
            out.push(tail.take().expect("上面已确认 tail 存在"));
        }
    }
}

/// 把 `tail` 并进正在生长的当前段 —— 「并进后继」那一半，外加「切回来」。
///
/// 两种情形会并：
/// - **同应用同类且相接**：`Code → chrome(2s) → Code` 要长成一段，而不是
///   两根紧挨着的同应用段；
/// - **`tail` 短于 N 且相接且 idleness 相同**：前驱短、又没有更前面的段可并，
///   那这段时间归给后继（规则 ② 的后半）。
///
/// 必须在这里做而不是等当前段关闭：缓冲会在「满 N 秒没有新段」时被释放，
/// 而一个正在生长的段可能持续几十分钟 —— 等到它关闭，`tail` 早被放走了。
pub(crate) fn absorb_tail_into_open(
    tail: &mut Option<ActivitySegment>,
    open: &mut OpenSegment,
    min_ms: i64,
) {
    let Some(t) = tail.as_ref() else {
        return;
    };
    let touching = t.end_at >= open.start_at;
    let compatible = same_idleness(t.category, open.category);
    let same_key = t.application == open.application && t.category == open.category;
    let t_is_short = span_ms(t) < min_ms;
    if !(touching && compatible && (same_key || t_is_short)) {
        return;
    }

    let t = tail.take().expect("上面已确认 tail 存在");
    open.start_at = open.start_at.min(t.start_at);
    open.evidence_event_ids.splice(0..0, t.evidence_event_ids);
    open.active_ms = open.active_ms.saturating_add(t.active_ms);
}
