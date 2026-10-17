import { DEFAULT_GRANULARITY } from "./bucket";
import { clipToUncovered } from "./clip";
import type { Category, Segment } from "../types";

const MINUTE = 60_000;
const DAY_MS = 86_400_000;

/** 一格有没有可读的数。见 `BucketMetric.readout`。 */
export type BucketReadout = "empty" | "idle" | "no-input" | "value";

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
   * 桶内的**非空闲**时长（段的时间跨度）。
   *
   * **不是专注度的分母了**（2026-10-13）。分母是 `inputMs`。这个留着是因为
   * 覆盖率、切换、段数还要用。
   */
  activeMs: number;
  /**
   * 桶内**用户确实有输入**的时长（毫秒）。
   *
   * `null` = 桶里的段都是 2026-10-13 之前的历史数据，没有这个量。
   * 这种情况**退回旧口径**（见 focus），而不是显示 0% ——
   * 把「没数据」说成「完全分心」是在编造。
   */
  inputMs: number | null;
  /**
   * **这一格有没有可读的数** —— 数据层面的结论，不是「该不该涂色」。
   *
   * 为什么它必须由 metrics 给出：同一个判断原来在 `SegmentTimeline` 里写了四遍，
   * 而且四处的判据互不相同（导航用 `segmentCount > 0`、涂色用
   * `segmentCount === 0 || inputMs === 0`、无障碍名用 `activeMs === 0`、
   * 浮层又拼一次）—— 于是「这一格是空的吗」在不同交互里答案不一样。
   * 判据一旦分散，就没人能回答「为什么这一格点得动、却画不出颜色」。
   *
   * 四种状态，边界互斥且齐全：
   * - `empty`：桶里一段都没有（无人监测 / 空白区）
   * - `idle`：有段但**全是空闲**（人不在，不是"零分"）
   * - `no-input`：**有活动但没有任何输入**（inputMs 恰好为 0）
   * - `value`：有活动且有输入，可以报读数
   *
   * 注意 `inputMs === null`（历史数据）**不算 `no-input`**，仍然落到 `value` ——
   * 「没有这个量」和「这个量是 0」是两件事（STATUS 第 37 条）。
   */
  readout: BucketReadout;
  /**
   * 专注度 = 主导类别时长 / **分母**，0..1。
   *
   * **口径（2026-10-13，用户拍板：笼统，边界清晰，不做细分）**：
   *
   * ```
   * 分母 = 桶内「用户确实有输入」的时长
   * 分子 = 主导类别的时长
   * ```
   *
   * 规则只有一条：**没在动的时间不进分母。** 于是
   * - 人不在（锁屏 / 长时间无输入）→ 那一段被摘出去，不会显示成满档
   * - 全程在动 → 100%
   *
   * **明确不做的细分**：「边做边走神」与「看视频」当前**同分**（都是单一类别、
   * 都在动）。分档是未来的方向，现在不要为了「更准」把公式搞复杂 ——
   * 一个说不清口径的复合公式比粗糙的公式更没用（读者无法反推某个格子
   * 为什么是这个颜色）。
   *
   * `inputMs === null`（2026-10-13 之前的历史数据）→ 退回旧口径
   * （分母用 `activeMs`）。宁可粗，也不要假装知道。
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
/**
 * 重叠情况的**节流汇报**（2026-10-13）。
 *
 * 为什么不能每桶每刷都打：时间线每 5 秒轮询一次，一次刷新要重算全部桶，
 * 于是同一批重叠会刷出几十行 —— 控制台被自己的告警淹没之后，
 * 真正该看的那一行也就没人看了。
 *
 * 与后端 `summary.rs` 的 `OverlapReporter` 同款：累积 + 每分钟最多一行。
 * 后端那边已经有汇总行，前端这行是同一件事在浏览器侧的第二次计数，
 * 保留它是为了「是谁造成的」在 devtools 里也一眼可见。
 */
let overlapAccum = { pairs: 0, ms: 0, lastAt: 0 };

function reportOverlap(pairs: number, ms: number): void {
  overlapAccum.pairs += pairs;
  overlapAccum.ms += ms;
  const now = Date.now();
  if (now - overlapAccum.lastAt < 60_000) return;
  overlapAccum.lastAt = now;
  console.error(
    `[metrics] 段被重叠覆盖：累计 ${overlapAccum.pairs} 段 / ${overlapAccum.ms}ms。` +
      `时长已去重（数字是对的），但这说明 activities 表里存在重叠段 ——` +
      `引擎侧已于 2026-10-08 修过（见 segmenter 的 fold_short_segments open 分支）。` +
      `历史数据需重放才能修掉。同一情况每分钟最多报一次。`,
  );
}

/** **仅测试用**：清掉节流状态，否则「一小时内只报一次」会让测试互相干扰。 */
export function __resetOverlapReportForTest(): void {
  overlapAccum = { pairs: 0, ms: 0, lastAt: 0 };
}

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
    inputMs: null,
    readout: "empty",
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
      buckets[i].segments.push(seg);
    }
  }

  for (const b of buckets) {
    b.segmentCount = b.segments.length;
    if (b.segmentCount === 0) continue;

    // 覆盖时长与逐类别时长都从**同一个裁剪结果**来：
    // 段先裁到桶内，再走 `clipToUncovered`（全仓唯一的重叠口径）。
    //
    // 原来这里是「逐段独立累加 → 夹到桶宽」，那个夹是去重之前的兜底：
    // 它只保证数字不超过 1，重叠时是**多算**。而同一个函数里另有一段
    // first-wins 的去重循环在算 `activeMs` —— 于是同一个桶有两种覆盖口径，
    // 覆盖率与专注度在重叠数据上互相矛盾（STATUS 第 22 条）。
    //
    // **去掉的量必须可见**（2026-10-13）：静默去重会让「引擎又产出坏数据」
    // 完全隐形 —— 数字仍然对，排查时看不出问题出在读侧还是引擎。宁可吵，不可静。
    b.segments.sort((a, z) => a.startAt - z.startAt);
    const apps = new Set<string>();
    const byCategory = new Map<Category, number>();

    const { kept, hiddenCount, hiddenMs } = clipToUncovered(
      // 段被桶边界切开时只算落在本桶的那一段
      b.segments.map((s) => ({
        ...s,
        startAt: Math.max(s.startAt, b.start),
        endAt: Math.min(s.endAt, b.end),
      })),
    );

    // `inputMs` 从 null 起：**一个段都没有这个量的桶保持 null**，
    // 表示「这是 2026-10-13 之前的数据」，而不是「这段时间没人动」。
    let sawInputField = false;
    for (const { seg: s, start: segStart, end: segEnd } of kept) {
      apps.add(s.application ?? "（未知）");
      const inside = segEnd - segStart;
      b.coveredMs += inside;
      if (s.category !== "idle") {
        b.activeMs += inside;
        byCategory.set(s.category, (byCategory.get(s.category) ?? 0) + inside);
      }
      // 桶内「有输入时长」：按段在桶内占据的比例摊分。
      // 段跨桶时不能把整段算进某一个桶。
      if (typeof s.activeMs === "number") {
        sawInputField = true;
        const span = Math.max(1, s.endAt - s.startAt);
        b.inputMs = (b.inputMs ?? 0) + (s.activeMs * inside) / span;
      }
    }
    b.coverage = b.coveredMs / width;

    // 切换次数按**原始段序**算，不受去重影响 —— 时间线上看得见两条就数两次
    for (let i = 1; i < b.segments.length; i++) {
      const prev = b.segments[i - 1].application ?? "（未知）";
      const cur = b.segments[i].application ?? "（未知）";
      if (prev !== cur) b.switches++;
    }

    if (!sawInputField) b.inputMs = null;
    if (hiddenCount > 0) {
      reportOverlap(hiddenCount, hiddenMs);
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
    // 专注度：主导类别占比，**分母是「有输入时长」**。
    // 边界只有一条：没在动的时间不进分母。人不在（锁屏/长时间无输入）因此
    // 不会被读成「高度专注」；全程在动就是 100%。
    // 「边做边走神」与「看视频」当前同分 —— 细分是未来的方向，现在不做
    // （见 BucketMetric.focus 的注释）。
    // `inputMs === null`（历史数据）→ 退回旧口径，别假装知道。
    // `inputMs === 0`（这一桶确实没动）→ focus = 0，**不能退回旧口径**：
    // 那正是「人不在」被读成「高度专注」的那个坑。
    const denom =
      b.inputMs === null
        ? b.activeMs
        : b.inputMs;
    b.focus = denom > 0 && bestMs > 0 ? Math.min(1, bestMs / denom) : 0;
    // 「这一格有没有可读的数」在这里算一次，别让每个调用方各自重推
    // （判据分散时，同一个「空」在导航、涂色、读屏、浮层里会给出四种答案）。
    b.readout =
      b.segmentCount === 0
        ? "empty"
        : b.activeMs === 0
          ? "idle"
          : b.inputMs === 0
            ? "no-input"
            : "value";
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

/**
 * 图例两端的中文描述，给色阶图例和无障碍标签用。
 *
 * 「分心/专注」那一侧的措辞是**按主导类别占比**说的（笼统口径，
 * 2026-10-13 用户拍板不做细分）—— 见 `BucketMetric.focus`。
 * 「完全分心」现在的意思是「这一桶的主导类别只占很小一部分」，
 * 而不是「一直在切换程序」。
 */
export const FOCUS_LEGEND = ["完全分心", "较碎", "一半一半", "较专注", "高度专注"];
export const SWITCH_LEGEND = ["没有切换", "1–2 次", "3–5 次", "6–10 次", "10 次以上"];
