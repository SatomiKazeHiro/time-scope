/**
 * 重叠段的裁剪口径 —— **全仓唯一的一份**（前端）。
 *
 * 规则只有一条：**先开始的段拥有重叠部分**（first-wins by startAt）。
 * 选它的理由与后端 `storage::summary::clip_to_uncovered` 相同：确定、不需要
 * 新数据、与时间线的阅读顺序一致（时间线也从左往右读）。
 *
 * 为什么必须有这个文件：同一条规则原来在前端写了两遍（`metrics.ts` 的桶内
 * 累加、`bucket.ts` 的当日汇总），加上后端一份、热力图一份一共四处。
 * 2026-10-13 的「三处同源」就是这么丢了一处 —— `summarize` 漏了去重，
 * 于是「当日汇总」与「时间线指标」在重叠出现时会对不上。
 *
 * **被藏起来的量必须可见**：`hiddenCount` / `hiddenMs` 由调用方报出来。
 * 静默兜底比不兜底更危险：数字仍然对 → 「引擎又产出坏数据」完全隐形。
 */

/** 只需要时间边界的任何东西。 */
export interface Span {
  startAt: number;
  endAt: number;
}

export interface ClippedSpan<T extends Span> {
  seg: T;
  /** 裁剪后这段真正占用的区间 `[start, end)` */
  start: number;
  end: number;
}

export interface ClipResult<T extends Span> {
  /** 只含真正占用了时间的段，`start`/`end` 已避让更早的段 */
  kept: ClippedSpan<T>[];
  /** 被更早的段整段盖住（或零长 / 倒挂）的段数 */
  hiddenCount: number;
  /** 这些段一共被藏起来多少毫秒 */
  hiddenMs: number;
}

/**
 * 裁剪掉与更早段重叠的部分。
 *
 * 入参不必有序：内部按 `startAt` 排序（同起点保持入参顺序，让结果可预测）。
 * 首尾相接（`prev.endAt === next.startAt`）**不是**重叠，两段都计满。
 */
export function clipToUncovered<T extends Span>(spans: readonly T[]): ClipResult<T> {
  const ordered = spans
    .map((seg, i) => ({ seg, i }))
    .sort((a, b) => a.seg.startAt - b.seg.startAt || a.i - b.i);

  const kept: ClippedSpan<T>[] = [];
  let coveredUntil = Number.NEGATIVE_INFINITY;
  let hiddenCount = 0;
  let hiddenMs = 0;

  for (const { seg } of ordered) {
    const start = Math.max(seg.startAt, coveredUntil);
    if (start < seg.endAt) {
      kept.push({ seg, start, end: seg.endAt });
      coveredUntil = Math.max(coveredUntil, seg.endAt);
    } else {
      // 整段被更早的段盖住 —— 只可能是重叠造成的数据缺陷（或零长 / 倒挂）
      hiddenCount += 1;
      hiddenMs += Math.max(0, seg.endAt - seg.startAt);
    }
  }

  return { kept, hiddenCount, hiddenMs };
}
