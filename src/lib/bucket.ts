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

/**
 * 一段被桶边界切出来的碎片。
 *
 * 与 `bucketSegments` 的区别：那个把**整段**塞进它跨越的每个桶（聚合用），
 * 这个按 spec §8.2 的原话「一个跨桶的 segment 按桶边界切成多段，按落入时长分配」
 * 真正把时间切开 —— 每块只覆盖自己那段时间。
 */
export interface SegmentSlice {
  /** 切片后唯一。`${segmentId}#${序号}` */
  id: string;
  /** 原段 id。点任意一块都要选中整段活动，不是选一片。 */
  segmentId: string;
  startAt: number;
  endAt: number;
  /** 原始段对象，悬停详情直接用它的字段 */
  segment: Segment;
  /** 是不是原段的第一块 —— 段内直标只标第一块，否则一段连着切三刀会标三次 */
  isFirst: boolean;
}

/**
 * 按桶边界把段切成碎片。
 *
 * `intervalMs` 非法（0 / NaN）时不切，原样返回 —— 静默返回空数组会让整条
 * 时间线消失，而不是"退回不切"。
 */
export function sliceSegments(segments: Segment[], intervalMs: number): SegmentSlice[] {
  const unsplit = (): SegmentSlice[] =>
    segments.map((s) => ({
      id: `${s.id}#0`,
      segmentId: s.id,
      startAt: s.startAt,
      endAt: s.endAt,
      segment: s,
      isFirst: true,
    }));

  if (!intervalMs || !Number.isFinite(intervalMs) || intervalMs < 0) return unsplit();

  const out: SegmentSlice[] = [];
  for (const s of segments) {
    // 零长或倒挂的段：切不出碎片，原样保留一块，否则它会在界面上彻底消失
    if (s.endAt <= s.startAt) {
      out.push({
        id: `${s.id}#0`,
        segmentId: s.id,
        startAt: s.startAt,
        endAt: s.endAt,
        segment: s,
        isFirst: true,
      });
      continue;
    }

    let cursor = s.startAt;
    let n = 0;
    while (cursor < s.endAt) {
      // 用 ceil 且从 cursor+1 起算：正好落在边界上的点不能再切一刀，
      // 否则每段末尾都会多出一个 0 宽的碎片。
      const boundary = Math.ceil((cursor + 1) / intervalMs) * intervalMs;
      const pieceEnd = boundary > cursor && boundary < s.endAt ? boundary : s.endAt;
      out.push({
        id: `${s.id}#${n}`,
        segmentId: s.id,
        startAt: cursor,
        endAt: pieceEnd,
        segment: s,
        isFirst: n === 0,
      });
      cursor = pieceEnd;
      n++;
    }
  }
  return out;
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
