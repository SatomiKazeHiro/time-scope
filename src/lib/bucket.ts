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

export interface Bucket {
  start: number;
  end: number;
  segments: Segment[];
}

/**
 * 把段切进它跨越的各个桶。跨桶的段会在多个桶里各出现一次。
 *
 * 桶的下界取到 [0, DAY_MS)：跨零点的段、或时区差导致的越界，
 * 不夹的话会算出负下标的桶。
 */
export function bucketSegments(segments: Segment[], intervalMs: number): Bucket[] {
  const byStart = new Map<number, Segment[]>();
  for (const s of segments) {
    const clampedStart = Math.max(s.startAt, 0);
    const clampedEnd = Math.min(s.endAt, DAY_MS);
    if (clampedEnd <= clampedStart) {
      // 零长或完全在一天之外的段：仍归入起点所在桶，免得静默丢失
      const only = bucketStart(clampedStart, intervalMs);
      const arr = byStart.get(only);
      if (arr) arr.push(s);
      else byStart.set(only, [s]);
      continue;
    }
    const first = bucketStart(clampedStart, intervalMs);
    const last = bucketStart(clampedEnd - 1, intervalMs);
    for (let b = first; b <= last; b += intervalMs) {
      const arr = byStart.get(b);
      if (arr) arr.push(s);
      else byStart.set(b, [s]);
    }
  }
  return Array.from(byStart.entries())
    .sort((a, b) => a[0] - b[0])
    .map(([start, segs]) => ({ start, end: start + intervalMs, segments: segs }));
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
 */
export function summarize(segments: Segment[], dayStartMs: number): SummaryRow[] {
  const dayEnd = dayStartMs + DAY_MS;
  const totals = new Map<Category, number>();
  for (const s of segments) {
    const start = Math.max(s.startAt, dayStartMs);
    const end = Math.min(s.endAt, dayEnd);
    const d = end - start;
    if (d <= 0) continue;
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
