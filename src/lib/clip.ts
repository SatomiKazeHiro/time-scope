/**
 * 重叠段的裁剪口径 —— 全仓唯一的一份（前端）。
 *
 * 规则：先开始的段拥有重叠部分（first-wins by startAt），与后端
 * `storage::summary::clip_to_uncovered` 同一条。以前前端抄了两遍、后端一遍、
 * 热力图一遍，2026-10-13 就因为漏抄一处让「当日汇总」与时间线指标对不上。
 *
 * 被藏起来的量必须由调用方报出来：静默兜底时数字仍然对，问题就隐形了。
 */

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
 * 首尾相接（`prev.endAt === next.startAt`）不是重叠，两段都计满。
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
