import type { Category, Segment } from "../types";
import { DEFAULT_GRANULARITY } from "./bucket";

const MINUTE = 60_000;
const DAY_MS = 86_400_000;

/**
 * 一个时间桶上的聚合指标。
 *
 * 全部由 `get_segments` 的结果算出，**不需要新的后端接口** ——
 * 段里已经有 startAt / endAt / category / application，够算。
 */
export interface BucketMetric {
  start: number;
  end: number;
  /** 落在桶内的段（按起点排序，段本身按当天裁剪过） */
  segments: Segment[];
  /** 被段覆盖的时长，已截到桶宽 */
  coveredMs: number;
  /** 覆盖率 0..1 */
  coverage: number;
  /** 落在桶内的段数 */
  segmentCount: number;
  /**
   * 专注切换 = 相邻段之间**应用不同**的次数（界面上的模式名，见 SegmentTimeline 的 METRIC_LABEL）。
   *
   * 不用「段数 − 1」：引擎会把同一次活动切成多段（标题变化、窗口切换），
   * 那些不是用户自己在跳。数应用变化才是"我换了个程序"。
   */
  switches: number;
  /**
   * 桶内的**非空闲**时长。专注度的分母 —— 见下面 focus 的说明。
   */
  activeMs: number;
  /**
   * 专注度 = 主导类别时长 / **非空闲**时长，0..1。
   *
   * **分母必须排除 idle。** idle 是一个类别，如果把它算进来，挂机两小时的桶
   * 主导类别就是 idle、专注度算出来 100% —— "完全没在用电脑"被读成"高度专注"。
   * 用户第一次看到的就是这个。空闲不是"不专注"，是"没在工作"，两回事。
   *
   * 桶内全是空闲时 activeMs = 0，focus = 0，且界面上**不涂色** ——
   * 涂成最低档会被读成"在做事但很分心"，同样是假的。
   */
  focus: number;
  /** 主导类别（只统计非空闲段）；桶内无活动时为 null */
  dominantCategory: Category | null;
  /** 桶内是否只有空闲（"没在工作"，不是"不专注"） */
  allIdle: boolean;
  /** 去重应用数 */
  appCount: number;
}

/**
 * 把一天按 `intervalMs` 切桶，逐桶算指标。
 *
 * 段按当天裁剪（和 `summarize` 同一个口径），跨零点的段不会被算进两天。
 * `intervalMs` 非法时退回默认粒度，而不是静默返回空数组 ——
 * 返回空数组会让整条时间线消失，比粒度不对更难排查。
 */
export function bucketMetrics(
  segments: Segment[],
  dayStartMs: number,
  intervalMs: number,
): BucketMetric[] {
  const step =
    intervalMs > 0 && Number.isFinite(intervalMs) ? intervalMs : DEFAULT_GRANULARITY * MINUTE;
  const count = Math.max(1, Math.round(DAY_MS / step));
  const width = DAY_MS / count;

  const buckets: BucketMetric[] = Array.from({ length: count }, (_, i) => ({
    start: dayStartMs + i * width,
    end: dayStartMs + (i + 1) * width,
    segments: [],
    coveredMs: 0,
    coverage: 0,
    segmentCount: 0,
    switches: 0,
    activeMs: 0,
    focus: 0,
    dominantCategory: null,
    allIdle: false,
    appCount: 0,
  }));

  for (const seg of segments) {
    // 跨零点的段按当天裁剪，否则它会把两天的覆盖率都算爆
    const start = Math.max(seg.startAt, dayStartMs);
    const end = Math.min(seg.endAt, dayStartMs + DAY_MS);
    if (end <= start) continue; // 零长 / 倒挂的段不参与统计

    const from = Math.max(0, Math.floor((start - dayStartMs) / width));
    const to = Math.min(count - 1, Math.floor((end - 1 - dayStartMs) / width));
    for (let i = from; i <= to; i++) {
      const b = buckets[i];
      const bStart = dayStartMs + i * width;
      const bEnd = bStart + width;
      b.segments.push(seg);
      // 段被桶边界切开时只算落在本桶的那一段
      b.coveredMs += Math.min(end, bEnd) - Math.max(start, bStart);
    }
  }

  for (const b of buckets) {
    b.segmentCount = b.segments.length;
    if (b.segmentCount === 0) continue;

    // 覆盖时长截到桶宽：兜底（去重之后不该再超 1，见下）
    b.coveredMs = Math.min(b.coveredMs, width);
    b.coverage = b.coveredMs / width;

    b.segments.sort((a, z) => a.startAt - z.startAt);
    const apps = new Set<string>();
    const byCategory = new Map<Category, number>();

    // **重叠去重**：逐段独立累加会把同一段时间数两遍，`activeMs` 被抬高、
    // 于是 `focus` 这个「主导类别占比」的分母虚高、专注度被稀释。
    //
    // 规则与后端 `clip_to_uncovered` 一致：**先开始的段拥有重叠部分**
    // （first-wins by startAt）。两处必须同规则，否则汇总页与时间线
    // 对同一个重叠会给出不同的解释（STATUS 第 22 条「三处同源」）。
    //
    // **去掉的量必须可见**，与后端同理（2026-10-13）：静默去重会让
    // 「引擎又产出坏数据」这件事完全隐形 —— 数字仍然对，但排查时
    // 根本看不出问题出在读侧还是引擎。宁可吵，不可静。
    let coveredUntil = Number.NEGATIVE_INFINITY;
    let hiddenMs = 0;
    let hiddenCount = 0;
    for (let i = 0; i < b.segments.length; i++) {
      const s = b.segments[i];
      apps.add(s.application ?? "（未知）");
      const from = Math.max(s.startAt, coveredUntil);
      const clamped = Math.max(0, Math.min(s.endAt, b.end) - from);
      if (clamped > 0) {
        coveredUntil = Math.min(s.endAt, b.end);
      } else {
        // 整段被更早的段盖住 —— 只可能是重叠造成的数据缺陷
        hiddenMs += Math.min(s.endAt, b.end) - Math.max(s.startAt, b.start);
        hiddenCount += 1;
      }
      if (s.category !== "idle") {
        b.activeMs += clamped;
        byCategory.set(s.category, (byCategory.get(s.category) ?? 0) + clamped);
      }
      if (i > 0 && (b.segments[i - 1].application ?? "（未知）") !== (s.application ?? "（未知）")) {
        b.switches++;
      }
    }
    if (hiddenCount > 0) {
      console.error(
        `[metrics] 桶 #${buckets.indexOf(b)} 有 ${hiddenCount} 段被重叠覆盖，共 ${hiddenMs}ms。` +
          `时长已去重（数字是对的），但这说明 activities 表里存在重叠段 ——` +
          `引擎侧已于 2026-10-08 修过（见 segmenter 的 fold_short_segments open 分支）。` +
          `历史数据需重放才能修掉。`,
      );
    }
    b.appCount = apps.size;

    // 主导类别 = 非空闲段里时长最长的那个。
    let best: Category | null = null;
    let bestMs = -1;
    for (const [cat, ms] of byCategory) {
      if (ms > bestMs) {
        bestMs = ms;
        best = cat;
      }
    }
    b.dominantCategory = best;
    // 分母是非空闲时长：挂机两小时的桶是"没在工作"，不是"高度专注"
    b.focus = b.activeMs > 0 && bestMs > 0 ? Math.min(1, bestMs / b.activeMs) : 0;
    // 「只有空闲」必须在循环外判定：桶里既有 idle 也有 work 时它不是全空闲
    b.allIdle = b.coveredMs > 0 && b.activeMs === 0;
  }

  return buckets;
}

/* ── 顺序色阶分档 ──────────────────────────────────────────────
   两套都用**绝对阈值**，不用「按当天最大值归一化」——
   相对刻度会让一个安静的日子看起来像"到处都很碎"，
   而「5 次切换」每天都是同一个意思。 */

export const SCALE_STEPS = 5;

/** 专注度 → 1..5 */
export function focusStep(focus: number): number {
  if (focus >= 0.9) return 5;
  if (focus >= 0.7) return 4;
  if (focus >= 0.5) return 3;
  if (focus >= 0.3) return 2;
  return 1;
}

/** 专注切换数 → 1..5 */
export function switchStep(switches: number): number {
  if (switches <= 0) return 1;
  if (switches <= 2) return 2;
  if (switches <= 5) return 3;
  if (switches <= 10) return 4;
  return 5;
}

export function scaleColor(step: number): string {
  const s = Math.max(1, Math.min(SCALE_STEPS, Math.round(step)));
  return `var(--color-scale-${s})`;
}

/** 图例两端的中文描述，给色阶图例和无障碍标签用。 */
export const FOCUS_LEGEND = ["完全分心", "较碎", "一半一半", "较专注", "高度专注"];
export const SWITCH_LEGEND = ["没有切换", "1–2 次", "3–5 次", "6–10 次", "10 次以上"];
