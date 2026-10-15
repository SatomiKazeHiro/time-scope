import type { Category, Segment } from "../types";

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
 * **重叠段去重**（2026-10-13 补上）。这里原来逐段独立相加，而
 * `metrics.ts` 与后端 `clip_to_uncovered` 都做了去重 —— 三处口径不一致，
 * 违反 STATUS §1 第 22 条「三处必须同源」。当时没出事只是因为引擎刚修完、
 * 数据里没有重叠段；一旦重叠再现，「当日汇总」就会与「时间线指标」对不上。
 *
 * 规则与另两处**完全相同**：**先开始的段拥有重叠部分**（first-wins by startAt）。
 */
export function summarize(segments: Segment[], dayStartMs: number): SummaryRow[] {
  const dayEnd = dayStartMs + DAY_MS;
  const clipped = segments
    .map((s) => ({ ...s, startAt: Math.max(s.startAt, dayStartMs), endAt: Math.min(s.endAt, dayEnd) }))
    .filter((s) => s.endAt > s.startAt)
    .sort((a, z) => a.startAt - z.startAt);

  const totals = new Map<Category, number>();
  let coveredUntil = Number.NEGATIVE_INFINITY;
  for (const s of clipped) {
    const from = Math.max(s.startAt, coveredUntil);
    const d = s.endAt - from;
    if (d <= 0) continue;
    coveredUntil = s.endAt;
    totals.set(s.category, (totals.get(s.category) ?? 0) + d);
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
