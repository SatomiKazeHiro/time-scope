import { useMemo, useState } from "react";
import { formatDuration } from "../lib/bucket";
import {
  bucketMetrics,
  focusStep,
  switchStep,
  scaleColor,
  type BucketMetric,
} from "../lib/metrics";
import { colorForCategory, labelForCategory, labelInkFor, metaForCategory } from "../design/categories";
import type { Category, Segment } from "../types";

const DAY_MS = 86_400_000;
const WIDTH = 1000;
const TRACK_H = 64;
/** 底部刻度尺高度。游标和刻度都长在这条带上，不压数据。
    8 单位（约 9px）已经够读出疏密，再高就成了和数据抢戏的装饰品。 */
const RULER_H = 8;
const SVG_H = TRACK_H + RULER_H;
const MINUTES_PER_DAY = 24 * 60;
const MINUTE_MS = 60_000;

/**
 * 极短段也要看得见：0 宽的 rect 等于没画，用户会以为数据丢了。
 *
 * 2.5 单位 ≈ 铺满时 3px。真实的采集结果一天有上百个 1–3 分钟的短段，
 * 1 单位（≈1.2px）在两个主题里都只是发丝线，读起来像渲染噪点而不是数据。
 * 代价是最短的那批段被画得比实际宽 —— 与其看不见，宁可略失真。
 */
const MIN_WIDTH = 2.5;
/**
 * 两种缝，分得很要紧：
 *
 *  - `GAP` 不同活动段之间。1.5 单位，让"这里结束、那里开始"读得出来。
 *  - `SPLIT_GAP` **首尾相接、又是同一类别**的相邻段之间。1 单位，更细 ——
 *    那不是活动边界，只是引擎把它们分成了两段（比如标题变了），
 *    视觉上应该读成"同一个活动的两截"而不是"两件事"。
 */
const GAP = 1.5;
const SPLIT_GAP = 1;
/** 窄于此宽度不直标：文字放不下，硬塞会盖住相邻段。 */
const LABEL_MIN_W = 46;
/** 小刻度固定 10 分钟（当参考尺）；大刻度跟随当前粒度。 */
const MINOR_STEP_MIN = 10;
const DEFAULT_MAJOR_STEP_MIN = 60;
/** 3 小时一格太密、6 小时一格对不齐小数；这里只标 0/6/12/18/24。 */
const AXIS_LABELS = [0, 6, 12, 18, 24];

/** 兼容旧引用：现在只映射到 CSS 变量，色值真相在 styles/theme.css。 */
export const CATEGORY_COLOR: Record<Category, string> = Object.fromEntries(
  (["work", "study", "entertainment", "communication", "browsing", "life", "idle", "unknown"] as Category[]).map(
    (c) => [c, colorForCategory(c)],
  ),
) as Record<Category, string>;

export { colorForCategory };

/** 时间线看什么。类别模式画段，另两种模式画桶。 */
export type MetricMode = "category" | "focus" | "switch";

export const METRIC_LABEL: Record<MetricMode, string> = {
  category: "类别",
  focus: "专注度",
  switch: "切换次数",
};

interface Props {
  segments: Segment[];
  /** 该日 00:00 的 Unix 毫秒（本地时区） */
  dayStartMs: number;
  /** 点色块。第二个参数是所属桶的下标 —— 指标模式下色块就是桶，得知道是哪一格。 */
  onSelect: (s: Segment, bucketIndex?: number) => void;
  /** 当前选中的段：给它加一圈亮环。选中用明度表达，不占用任何类别色相。 */
  selectedId?: string | null;
  /**
   * 指标模式下被点中的那一格。
   *
   * 类别模式下一段就是一个色块，选中它就够；指标模式下**一段横跨十几个桶**，
   * 把它们全圈上会画出一道白栅栏。指标模式的选中语义是「我在看哪一格」，
   * 所以只圈被点的那一格。
   */
  selectedBucket?: number | null;
  /** 正在看今天时画「此刻」游标。历史日期上没有"现在"，画了是错的。 */
  showNow?: boolean;
  /**
   * 粒度（毫秒）。**类别模式下它只管底部刻度尺的大刻度**，不碰色块；
   * 指标模式下它就是桶宽。spec §8.2：切换粒度只改前端参数，不重查后端。
   */
  intervalMs?: number;
  /** 看类别还是看指标。默认 category。 */
  metric?: MetricMode;
}

interface Placed {
  seg: Segment;
  /** 在 segments 里的下标 —— 悬停时用它回查 */
  index: number;
  /** viewBox 单位 */
  x: number;
  w: number;
  /** 占全天的百分比，HTML 覆盖层按它定位 */
  leftPct: number;
  widthPct: number;
  /** 这次扣掉的缝宽。标签定位不再用它（曾因补偿公式溢出 8%） */
  gap: number;
}

function clockOf(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/**
 * 24h 横向时间线：每个 ActivitySegment 一个矩形，按 category 着色，宽度正比于时长。
 *
 * 用 viewBox + width:100%，容器再窄（高 DPI、小窗口）也不会把 x/width 算坏。
 *
 * **所有文字都走 HTML 覆盖层，不放进 SVG。** `preserveAspectRatio="none"` 会把
 * viewBox 非等比拉伸，放进去的 <text> 会跟着横向变形。轨道本身仍留在 SVG 里，
 * 因为它就是需要铺满宽度的色块。
 *
 * **轨道是可见的。** 之前轨道背景与面板同色，浅色下三者是同一个白 ——
 * 于是"这段没活动"和"这里什么都没画"读起来一模一样，整条时间线没有边框可依。
 * 现在轨道用 surface-2 + 描边 + 圆角，空白时段明确读作"无活动"。
 */
export default function SegmentTimeline({
  segments,
  dayStartMs,
  onSelect,
  selectedId,
  selectedBucket = null,
  showNow = false,
  intervalMs = 0,
  metric = "category",
}: Props) {
  const [hoverX, setHoverX] = useState<number | null>(null);
  const [hoverIndex, setHoverIndex] = useState<number | null>(null);

  const buckets = useMemo(
    () => (metric === "category" ? [] : bucketMetrics(segments, dayStartMs, intervalMs)),
    [metric, segments, dayStartMs, intervalMs],
  );

  if (segments.length === 0) {
    return (
      <div
        className="flex items-center rounded-md border border-line bg-surface-2 px-3 text-sm text-ink-faint"
        style={{ height: "var(--track-h)" }}
        role="img"
        aria-label="24h 活动时间线"
      >
        这一天还没有活动段。
      </div>
    );
  }
  const dayEnd = dayStartMs + DAY_MS;
  const isMetric = metric !== "category";

  /**
   * 类别模式：一个段就是一个色块，**不按桶切**。
   *
   * 段是引擎判定的活动边界，它是什么就是什么 —— 按时间格切一刀只会把一段
   * 连续活动切碎，既不增加信息（宽度已经表示时长），又让"这段到底多长"
   * 变得要靠心算。粒度在类别模式下只管底部的刻度尺。
   */
  const placed: Placed[] = segments.map((s, i) => {
    const start = Math.max(s.startAt, dayStartMs);
    const end = Math.min(s.endAt, dayEnd);
    // **只挡倒挂段**（end < start）。负宽度会让 x 飞到视图外把轨道画歪。
    // 零长段（end === start）要走正常路径拿 MIN_WIDTH 缝隙 —— 那是一个真实事件，
    // 画成 0 宽用户会以为数据丢了，这正是最早那条测试守住的东西。
    if (end < start) {
      return {
        seg: s,
        index: i,
        x: ((start - dayStartMs) / DAY_MS) * WIDTH,
        w: 0,
        leftPct: ((start - dayStartMs) / DAY_MS) * 100,
        widthPct: 0,
        gap: 0,
      };
    }
    const next = segments[i + 1];
    const continues = next !== undefined && next.startAt === end && next.category === s.category;
    const gap = continues ? SPLIT_GAP : GAP;
    const rawW = Math.max(((end - start) / DAY_MS) * WIDTH, MIN_WIDTH);
    const w = Math.max(rawW - gap, MIN_WIDTH);
    return {
      seg: s,
      index: i,
      x: ((start - dayStartMs) / DAY_MS) * WIDTH,
      w,
      leftPct: ((start - dayStartMs) / DAY_MS) * 100,
      widthPct: (w / WIDTH) * 100,
      gap,
    };
  });

  /** 指标模式下，一个桶就是一个色块。 */
  const bucketCells = buckets.map((b, i) => {
    const rawW = WIDTH / buckets.length;
    // 专注度模式下全是空闲的桶不涂色：空闲是"没在工作"，不是"不专注"。
    // 涂成最低档会被读成"在做事但很分心"，那也是假的。
    const blank = b.segmentCount === 0 || (metric === "focus" && b.activeMs === 0);
    const step = metric === "focus" ? focusStep(b.focus) : switchStep(b.switches);
    return {
      metric: b,
      index: i,
      blank,
      step: blank ? 0 : step,
      x: i * rawW,
      w: Math.max(rawW - GAP, MIN_WIDTH),
    };
  });

  // 「此刻」：夹进 [0,1]，否则跨天/时区差会把它画到轨道外
  const nowPct = showNow
    ? Math.min(Math.max((Date.now() - dayStartMs) / DAY_MS, 0), 1) * 100
    : null;

  // 刻度全在轨道下方那条带上，不压数据 —— 之前那条 22% 透明竖线穿过了
  // 整个色块区，碎片一多就成了噪声，读时间反而要眯眼。
  // 大刻度跟随当前粒度，于是这个控件在刻度尺上也看得见反应。
  const majorStepMin = intervalMs >= MINUTE_MS ? intervalMs / MINUTE_MS : DEFAULT_MAJOR_STEP_MIN;
  const ticks: { x: number; major: boolean }[] = [];
  const stepMin = Math.max(1, Math.min(MINOR_STEP_MIN, majorStepMin));
  for (let m = 0; m < MINUTES_PER_DAY; m += stepMin) {
    const major = m % majorStepMin === 0;
    // 粒度比 10 分钟还细时，大刻度和这一步重合，只画一根
    if (!major && stepMin >= majorStepMin) continue;
    ticks.push({ x: (m / MINUTES_PER_DAY) * WIDTH, major });
  }

  const hoverBucket: BucketMetric | null =
    isMetric && hoverIndex !== null ? buckets[hoverIndex] : null;

  return (
    /* 外层只负责定位，不裁剪 —— 提示框在轨道**上方**，放进裁剪容器会被切掉 */
    <div className="relative">
      <div className="group overflow-hidden rounded-md border border-line bg-surface-2">
        <svg
          width="100%"
          height={SVG_H}
          viewBox={`0 0 ${WIDTH} ${SVG_H}`}
          preserveAspectRatio="none"
          role="img"
          aria-label="24h 活动时间线"
          className="block w-full"
          /* 高度由 --track-h 决定，矮窗口下压扁（见 theme.css） */
          style={{ height: "var(--track-h)" }}
        >
          {isMetric
            ? bucketCells.map((c) => {
                const selected = selectedBucket === c.index;
                return (
                  <rect
                    key={`b${c.index}`}
                    data-bucket={c.index}
                    x={c.x}
                    y={4}
                    width={c.w}
                    height={TRACK_H - 8}
                    rx={1.5}
                    /* 没数据的桶留成轨道色，别用最低档 —— 那会被读成"专注度极低" */
                    fill={c.blank ? "transparent" : scaleColor(c.step)}
                    vectorEffect="non-scaling-stroke"
                    className="cursor-pointer"
                    style={{
                      pointerEvents: "all",
                      stroke: selected ? "var(--color-ink)" : "transparent",
                      strokeWidth: selected ? 2 : 6,
                    }}
                    onClick={() => {
                      const s = c.metric.segments[0];
                      if (s) onSelect(s, c.index);
                    }}
                    onMouseMove={(e) => {
                      const box = e.currentTarget.ownerSVGElement!.getBoundingClientRect();
                      setHoverX(((e.clientX - box.left) / box.width) * 100);
                      setHoverIndex(c.index);
                    }}
                    onMouseLeave={() => {
                      setHoverX(null);
                      setHoverIndex(null);
                    }}
                  >
                    <title>
                      {`${clockOf(c.metric.start)} – ${clockOf(c.metric.end)}`}
                    </title>
                  </rect>
                );
              })
            : null}
          {isMetric
            ? null
            : placed.map((p) => {
                const seg = p.seg;
                const selected = selectedId === p.seg.id;
                const cellIndex = p.index;
                return (
                  <rect
                    key={p.seg.id}
                    data-id={p.seg.id}
                    x={p.x}
                    y={4}
                    width={p.w}
                    height={TRACK_H - 8}
                    rx={1.5}
                    fill={colorForCategory(seg.category)}
                    vectorEffect="non-scaling-stroke"
                    className="cursor-pointer"
                    /* 描边兼两职：选中时是 2px 亮环（选中用明度表达，不占用类别色相）；
                       未选中时是 8px 透明描边 —— 1 分钟的段只有约 3px 宽，
                       裸 rect 不好点。透明描边只扩大命中区，不改变观感。 */
                    style={{
                      pointerEvents: "all",
                      stroke: selected ? "var(--color-ink)" : "transparent",
                      strokeWidth: selected ? 2 : 8,
                    }}
                    /* 点任意一块都选中整段活动，不是只选中这一片 */
                    onClick={() => onSelect(seg)}
                    onMouseMove={(e) => {
                      const box = e.currentTarget.ownerSVGElement!.getBoundingClientRect();
                      setHoverX(((e.clientX - box.left) / box.width) * 100);
                      setHoverIndex(cellIndex);
                    }}
                    onMouseLeave={() => {
                      setHoverX(null);
                      setHoverIndex(null);
                    }}
                  >
                    {/* 原生 title 兜底：浮层还没渲染出来前也有提示 */}
                    <title>
                      {`${clockOf(seg.startAt)} – ${clockOf(seg.endAt)} · ${seg.category} · ${formatDuration(seg.endAt - seg.startAt)}`}
                    </title>
                  </rect>
                );
              })}

          {/* 刻度尺：小刻度固定 10 分钟作参考，大刻度跟随当前粒度。
              一天一百多根，用 stroke 而非 DOM 节点，省得为装饰铺这么多元素。 */}
          <g
            data-ruler="ticks"
            pointerEvents="none"
            stroke="var(--color-ink-ghost)"
            vectorEffect="non-scaling-stroke"
          >
            {ticks.map((t) => (
              <line
                key={t.x}
                x1={t.x}
                x2={t.x}
                y1={SVG_H - (t.major ? RULER_H : RULER_H / 2)}
                y2={SVG_H}
                strokeWidth={1}
                opacity={t.major ? 0.9 : 0.4}
              />
            ))}
          </g>

          {/* 「此刻」游标：一条线，不加顶部三角 —— 三角会盖住第一行色块。
              pointer-events-none 保证它绝不挡用户点色块；
              鼠标移进容器时降到 40%，查数据时不再抢眼，但还找得回来
              （再暗就等于没有这个参考点了）。 */}
          {nowPct !== null && (
            <line
              x1={(nowPct / 100) * WIDTH}
              x2={(nowPct / 100) * WIDTH}
              y1={0}
              y2={SVG_H}
              stroke="var(--color-cursor-now)"
              strokeWidth={1.5}
              vectorEffect="non-scaling-stroke"
              pointerEvents="none"
              className="transition-opacity duration-[--duration-fast] group-hover:opacity-40"
            />
          )}
        </svg>

        {/* 段内直标：能不能打是结构决策（inlineLabel），什么颜色由 CSS 随主题决定。
            **左对齐贴在块的开头**，不居中：时间从左往右流，标签在块开头读作
            "从这里开始是 X"；居中会飘在宽块中间，而且离左右边界都远。
            宽度不够（< 46 单位 ≈ 66 分钟）就不打，两个字塞不下会盖到邻居上。
            这层覆盖层只盖轨道那部分，直接相对整个容器 top-1/2 会把标签
            推到轨道底边去蹭刻度尺。 */}
        <div
          className="pointer-events-none absolute inset-x-0 top-0"
          /* 覆盖层只盖轨道那部分（viewBox 72 单位里的 64），
             所以跟着 --track-h 一起缩，否则矮窗口下标签会掉到刻度尺上 */
          style={{ height: "calc(var(--track-h) * 0.888)" }}
        >
          {isMetric
            ? null
            : placed.map((p) => {
            const meta = metaForCategory(p.seg.category);
            if (!meta.inlineLabel || p.w < LABEL_MIN_W) return null;
            return (
              <span
                key={`l-${p.seg.id}`}
                aria-hidden
                /* 左对齐加一点内边距，不居中：时间从左往右流，标签贴在块的
                   开头读起来是"从这里开始是 X"；居中会飘在宽块中间。
                   左对齐也不会碰到轨道右沿 —— 贴着 24:00 的段也不会被裁。 */
                className="absolute top-1/2 -translate-y-1/2 truncate pl-1.5 text-[11px] leading-none font-medium"
                style={{
                  /* 标签正好盖住 rect：left 用 leftPct，width 用 rect 实际宽度
                     （已经扣过缝）。之前这里沿用切片时代的补偿公式，
                     left + width 会算出 108%，标签被 overflow-hidden 切掉一截。 */
                  left: `${p.leftPct}%`,
                  width: `${(p.w / WIDTH) * 100}%`,
                  color: labelInkFor(p.seg.category),
                  opacity: 0.85,
                }}
              >
                {meta.label}
              </span>
            );
          })}
        </div>
      </div>

      {hoverX !== null && (
        <div
          role="tooltip"
          className="pointer-events-none absolute z-20 -translate-x-1/2 -translate-y-full rounded-md border border-line-strong bg-surface-3 px-2.5 py-1.5 text-xs whitespace-nowrap shadow-pop"
          style={{ left: `${Math.min(Math.max(hoverX, 8), 92)}%`, top: -8 }}
        >
          {hoverBucket && hoverIndex !== null ? (
            bucketCells[hoverIndex].blank ? (
              /* 没涂色的格子不能报读数 —— 报了就是在编一个不存在的数值 */
              <>
                <span className="font-medium text-ink-muted">
                  {hoverBucket.segmentCount === 0
                    ? "无活动"
                    : metric === "focus"
                      ? "这段时间是空闲"
                      : METRIC_LABEL[metric]}
                </span>
                <div className="tnum mt-0.5 text-ink-muted">
                  {clockOf(hoverBucket.start)} – {clockOf(hoverBucket.end)}
                </div>
                {hoverBucket.segmentCount > 0 && metric === "focus" && (
                  <div className="mt-0.5 text-ink-faint">没在用电脑，不计入专注度</div>
                )}
              </>
            ) : (
              <>
                <span
                  className="mr-1.5 inline-block size-2 rounded-[2px] align-middle"
                  style={{ background: scaleColor(bucketCells[hoverIndex].step) }}
                  aria-hidden
                />
                <span className="font-medium">{METRIC_LABEL[metric]}</span>
                <span className="text-ink-muted">
                  {metric === "focus"
                    ? ` ${Math.round(hoverBucket.focus * 100)}%`
                    : ` ${hoverBucket.switches} 次`}
                </span>
                <div className="tnum mt-0.5 text-ink-muted">
                  {clockOf(hoverBucket.start)} – {clockOf(hoverBucket.end)}
                  <span className="mx-1 text-ink-ghost">|</span>
                  {`${hoverBucket.appCount} 个应用 · ${hoverBucket.segmentCount} 段`}
                </div>
                {hoverBucket.dominantCategory && (
                  <div className="mt-0.5 text-ink-faint">
                    主要在做 {labelForCategory(hoverBucket.dominantCategory)}
                  </div>
                )}
              </>
            )
          ) : hoverIndex !== null ? (
            (() => {
              const p = placed[hoverIndex];
              const seg = p?.seg;
              if (!seg) return null;
              return (
                <>
                  <span
                    className="mr-1.5 inline-block size-2 rounded-[2px] align-middle"
                    style={{ background: colorForCategory(seg.category) }}
                    aria-hidden
                  />
                  <span className="font-medium">{labelForCategory(seg.category)}</span>
                  <span className="text-ink-muted"> · {seg.application ?? "未知应用"}</span>
                  <div className="tnum mt-0.5 text-ink-muted">
                    {clockOf(seg.startAt)} – {clockOf(seg.endAt)}
                    <span className="mx-1 text-ink-ghost">|</span>
                    {formatDuration(seg.endAt - seg.startAt)}
                  </div>
                </>
              );
            })()
          ) : null}
        </div>
      )}

      {/* 刻度按真实位置绝对定位：0:00 贴左、24:00 贴右，中间等分。
          用 flex justify-between 会让最后一个标签的右边而不是左边对齐 24:00。 */}
      <div className="relative mt-1.5 h-4 text-micro text-ink-faint" aria-hidden>
        {AXIS_LABELS.map((h) => (
          <span
            key={h}
            /* 首尾两个标签贴边对齐：居中会让 24:00 有一半跑到容器外面被切掉 */
            className={`tnum absolute top-0 ${
              h === 0 ? "" : h === 24 ? "-translate-x-full" : "-translate-x-1/2"
            }`}
            style={{ left: `${(h / 24) * 100}%` }}
          >
            {String(h).padStart(2, "0")}:00
          </span>
        ))}
      </div>
    </div>
  );
}
