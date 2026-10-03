import { useMemo, useState } from "react";
import { formatDuration, sliceSegments, type SegmentSlice } from "../lib/bucket";
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
 *  - `SPLIT_GAP` 同一段被桶边界切开之间。1 单位，更细 —— 它只是时间网格线，
 *    不是活动边界。如果两者一样粗，30 分钟一切会把一段连续工作切成条形码，
 *    正好把碎片可读性那轮修掉的问题又请回来。
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

interface Props {
  segments: Segment[];
  /** 该日 00:00 的 Unix 毫秒（本地时区） */
  dayStartMs: number;
  onSelect: (s: Segment) => void;
  /** 当前选中的段：给它加一圈亮环。选中用明度表达，不占用任何类别色相。 */
  selectedId?: string | null;
  /** 正在看今天时画「此刻」游标。历史日期上没有"现在"，画了是错的。 */
  showNow?: boolean;
  /**
   * 粒度（毫秒）。段按这个间隔切分，刻度尺的大刻度也跟着它走。
   * 传 0 / 不传 = 不切。spec §8.2：切换粒度只改前端参数，不重查后端。
   */
  intervalMs?: number;
}

interface Placed {
  slice: SegmentSlice;
  /** viewBox 单位 */
  x: number;
  w: number;
  /** 占全天的百分比，HTML 覆盖层按它定位 */
  leftPct: number;
  widthPct: number;
  /** 这次扣掉的缝宽，直标的居中要用它回补 */
  gap: number;
  /** 整段（未切片）在轨道上的横向范围。直标按整段判宽、贴整段开头。 */
  segLeftPct: number;
  segWidthPct: number;
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
 *
 * **粒度是真的在切。** spec §8.2 要求"跨桶的 segment 按桶边界切成多段"，
 * 但这个控件之前只改了个桶计数，视图纹丝不动。
 */
export default function SegmentTimeline({
  segments,
  dayStartMs,
  onSelect,
  selectedId,
  showNow = false,
  intervalMs = 0,
}: Props) {
  const [hover, setHover] = useState<{ placed: Placed; xPct: number } | null>(null);

  const slices = useMemo(() => sliceSegments(segments, intervalMs), [segments, intervalMs]);

  if (segments.length === 0) {
    return (
      <div
        className="flex h-16 items-center rounded-md border border-line bg-surface-2 px-3 text-sm text-ink-faint"
        role="img"
        aria-label="24h 活动时间线"
      >
        这一天还没有活动段。
      </div>
    );
  }
  const dayEnd = dayStartMs + DAY_MS;

  // 跨零点或时区差可能让段越出当天，不夹会画出负 x 或超 viewBox
  const placed: Placed[] = slices.map((sl, i) => {
    const start = Math.max(sl.startAt, dayStartMs);
    const end = Math.min(sl.endAt, dayEnd);
    // 下一块是不是同一段的延续 —— 是的话缝细一档，那是时间网格不是活动边界
    const next = slices[i + 1];
    const continues = next !== undefined && next.segmentId === sl.segmentId;
    const gap = continues ? SPLIT_GAP : GAP;
    const rawW = Math.max(((end - start) / DAY_MS) * WIDTH, MIN_WIDTH);
    const segStart = Math.max(sl.segment.startAt, dayStartMs);
    const segEnd = Math.min(sl.segment.endAt, dayEnd);
    return {
      slice: sl,
      x: ((start - dayStartMs) / DAY_MS) * WIDTH,
      w: Math.max(rawW - gap, MIN_WIDTH),
      leftPct: ((start - dayStartMs) / DAY_MS) * 100,
      widthPct: (Math.max(rawW - gap, MIN_WIDTH) / WIDTH) * 100,
      gap,
      segLeftPct: ((segStart - dayStartMs) / DAY_MS) * 100,
      segWidthPct: (((segEnd - segStart) / DAY_MS) * 100),
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
  const step = Math.max(1, Math.min(MINOR_STEP_MIN, majorStepMin));
  for (let m = 0; m < MINUTES_PER_DAY; m += step) {
    const major = m % majorStepMin === 0;
    // 粒度比 10 分钟还细时，大刻度和这一步重合，只画一根
    if (!major && step >= majorStepMin) continue;
    ticks.push({ x: (m / MINUTES_PER_DAY) * WIDTH, major });
  }

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
          className="block h-18 w-full"
        >
          {placed.map((p) => {
            const seg = p.slice.segment;
            const selected = selectedId === p.slice.segmentId;
            return (
              <rect
                key={p.slice.id}
                data-id={p.slice.segmentId}
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
                  const xPct = ((e.clientX - box.left) / box.width) * 100;
                  setHover({ placed: p, xPct });
                }}
                onMouseLeave={() => setHover(null)}
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
            **按整段判宽、贴整段开头** —— 30 分粒度下一段连切六刀，每块只有约 21 单位，
            按块判宽等于默认视图下一个标签都打不出来，而直标是 §2.3 的无障碍兜底通道。
            只在第一块上标一次，否则一段会连着标六次。
            这层覆盖层只盖轨道那 64px，直接相对整个容器 top-1/2 会把标签
            推到轨道底边去蹭刻度尺。 */}
        <div className="pointer-events-none absolute inset-x-0 top-0 h-16">
          {placed.map((p) => {
            const meta = metaForCategory(p.slice.segment.category);
            if (!p.slice.isFirst || !meta.inlineLabel) return null;
            const segW = (p.segWidthPct / 100) * WIDTH;
            if (segW < LABEL_MIN_W) return null;
            return (
              <span
                key={`l-${p.slice.segmentId}`}
                aria-hidden
                className="absolute top-1/2 max-w-full -translate-y-1/2 truncate pr-1 text-[11px] leading-none font-medium"
                style={{
                  left: `${p.segLeftPct}%`,
                  width: `${p.segWidthPct}%`,
                  color: labelInkFor(p.slice.segment.category),
                  opacity: 0.85,
                }}
              >
                {meta.label}
              </span>
            );
          })}
        </div>
      </div>

      {hover && (
        <div
          role="tooltip"
          className="pointer-events-none absolute z-20 -translate-x-1/2 -translate-y-full rounded-md border border-line-strong bg-surface-3 px-2.5 py-1.5 text-xs whitespace-nowrap shadow-pop"
          style={{ left: `${Math.min(Math.max(hover.xPct, 8), 92)}%`, top: -8 }}
        >
          <span
            className="mr-1.5 inline-block size-2 rounded-[2px] align-middle"
            style={{ background: colorForCategory(hover.placed.slice.segment.category) }}
            aria-hidden
          />
          <span className="font-medium">
            {labelForCategory(hover.placed.slice.segment.category)}
          </span>
          <span className="text-ink-muted">
            {" "}
            · {hover.placed.slice.segment.application ?? "未知应用"}
          </span>
          <div className="tnum mt-0.5 text-ink-muted">
            {clockOf(hover.placed.slice.segment.startAt)} –{" "}
            {clockOf(hover.placed.slice.segment.endAt)}
            <span className="mx-1 text-ink-ghost">|</span>
            {formatDuration(
              hover.placed.slice.segment.endAt - hover.placed.slice.segment.startAt,
            )}
          </div>
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
