import type { Category, Segment } from "../types";
import { clipToUncovered } from "./clip";

/** 可选粒度（分钟）。spec §10：10 / 30（默认）/ 60 / 120。 */
export const GRANULARITIES = [10, 30, 60, 120] as const;
export const DEFAULT_GRANULARITY = 30;

const MINUTE = 60_000;
const DAY_MS = 86_400_000;

/**
 * 把时间戳向下取整到区间起点（spec §8.2）。
 *
 * `intervalMs` 为 0 时直接返回原值：除零会得到 NaN/Infinity，
 * 静默传下去会让整条时间线变成空白。
 */
export function bucketStart(ts: number, intervalMs: number): number {
  if (!intervalMs || !Number.isFinite(intervalMs)) return ts;
  return Math.floor(ts / intervalMs) * intervalMs;
}

export interface SummaryRow {
  category: Category;
  durationMs: number;
  ratio: number;
}

/**
 * 当日汇总。spec §9：后端不提供 summary 接口，由前端从 segments 聚合。
 *
 * 每段先被截到 `[dayStartMs, dayStartMs+24h)`，所以跨零点的段不会把时长算爆。
 *
 * 重叠去重走 `clip.ts` 的 `clipToUncovered` —— 不再在这里内联一遍。
 * 这条规则原来也被 `metrics.ts` 内联了一次，两处金额一致靠人看着；2026-10-13
 * 那次 `summarize` 漏掉去重，正好证明「同一条口径抄两遍」迟早分叉
 * （STATUS 第 22 条「三处必须同源」）。后端的对等实现是
 * `storage::summary::clip_to_uncovered`。
 */
export function summarize(segments: Segment[], dayStartMs: number): SummaryRow[] {
  const dayEnd = dayStartMs + DAY_MS;
  const { kept } = clipToUncovered(
    segments.map((s) => ({
      ...s,
      startAt: Math.max(s.startAt, dayStartMs),
      endAt: Math.min(s.endAt, dayEnd),
    })),
  );

  const totals = new Map<Category, number>();
  for (const { seg, start, end } of kept) {
    totals.set(seg.category, (totals.get(seg.category) ?? 0) + (end - start));
  }

  const total = Array.from(totals.values()).reduce((a, b) => a + b, 0);
  return Array.from(totals.entries())
    .map(([category, durationMs]) => ({
      category,
      durationMs,
      ratio: total > 0 ? durationMs / total : 0,
    }))
    .sort((a, b) => b.durationMs - a.durationMs);
}

/** 人类可读的时长。放在这里是因为汇总和详情都要用。 */
export function formatDuration(ms: number): string {
  const m = Math.round(ms / MINUTE);
  if (m < 60) return `${m} 分`;
  const h = Math.floor(m / 60);
  const rem = m % 60;
  return rem ? `${h} 时 ${rem} 分` : `${h} 时`;
}
