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

/**
 * 还没被规则覆盖的程序，按累计时长排序。
 *
 * 存在的理由：默认 15 条规则覆盖的是通用软件（IDE / 终端 / Office / 浏览器 /
 * 微信 QQ / Slack / Steam…），一个装了自己那套工具的人会有**很大一部分时间落进
 * unknown**。界面上只看到「未分类 33%」是没法行动的 —— 得直接告诉用户
 * 该往 `rules.toml` 里加哪几个 `process`，否则这个数字只是个让人沮丧的总数。
 */
export interface UnclassifiedApp {
  /** 进程文件名，可直接抄进 `[[rule]] process = [...]` */
  application: string;
  durationMs: number;
  /** 占未分类总时长的比例 0..1 */
  ratio: number;
}

export function unclassifiedApps(
  segments: Segment[],
  dayStartMs: number,
  limit = 5,
): UnclassifiedApp[] {
  const dayEnd = dayStartMs + DAY_MS;
  const totals = new Map<string, number>();
  let total = 0;

  for (const s of segments) {
    if (s.category !== "unknown") continue;
    const start = Math.max(s.startAt, dayStartMs);
    const end = Math.min(s.endAt, dayEnd);
    const d = end - start;
    if (d <= 0) continue;
    const key = s.application ?? "（未知应用）";
    totals.set(key, (totals.get(key) ?? 0) + d);
    total += d;
  }
  if (total === 0) return [];

  return Array.from(totals.entries())
    .map(([application, durationMs]) => ({
      application,
      durationMs,
      ratio: durationMs / total,
    }))
    .sort((a, b) => b.durationMs - a.durationMs)
    .slice(0, limit);
}

/** 人类可读的时长。放在这里是因为汇总和详情都要用。 */export function formatDuration(ms: number): string {
  const m = Math.round(ms / MINUTE);
  if (m < 60) return `${m} 分`;
  const h = Math.floor(m / 60);
  const rem = m % 60;
  return rem ? `${h} 时 ${rem} 分` : `${h} 时`;
}
