import { useState } from "react";
import { formatDuration } from "../lib/bucket";
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

/**
 * 极短段也要看得见：0 宽的 rect 等于没画，用户会以为数据丢了。
 *
 * 2.5 单位 ≈ 铺满时 3px。真实的采集结果一天有上百个 1–3 分钟的短段，
 * 1 单位（≈1.2px）在两个主题里都只是发丝线，读起来像渲染噪点而不是数据。
 * 代价是最短的那批段被画得比实际宽 —— 与其看不见，宁可略失真。
 */
const MIN_WIDTH = 2.5;
/**
 * 相邻填充之间留 1.5 单位底色缝，让两段不同类别的边界能读出来。
 * 碎片多的时候 2 单位的缝太宽，会把一段连续活动切成条。
 */
const GAP = 1.5;
/** 窄于此宽度不直标：文字放不下，硬塞会盖住相邻段。 */
const LABEL_MIN_W = 46;
/** 刻度尺：每 10 分钟一根小刻度，每 60 分钟一根大刻度。 */
const MINOR_STEP_MIN = 10;
const MAJOR_STEP_MIN = 60;
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
}

interface Placed {
  seg: Segment;
  /** viewBox 单位 */
  x: number;
  w: number;
  /** 占全天的百分比，HTML 覆盖层按它定位 */
  leftPct: number;
  widthPct: number;
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
  showNow = false,
}: Props) {
  const [hover, setHover] = useState<{ placed: Placed; xPct: number } | null>(null);

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
  const placed: Placed[] = segments.map((s) => {
    const start = Math.max(s.startAt, dayStartMs);
    const end = Math.min(s.endAt, dayEnd);
    const rawW = Math.max(((end - start) / DAY_MS) * WIDTH, MIN_WIDTH);
    const w = Math.max(rawW - GAP, MIN_WIDTH);
    return {
      seg: s,
      x: ((start - dayStartMs) / DAY_MS) * WIDTH,
      w,
      leftPct: ((start - dayStartMs) / DAY_MS) * 100,
      widthPct: (w / WIDTH) * 100,
    };
  });

  // 「此刻」：夹进 [0,1]，否则跨天/时区差会把它画到轨道外
  const nowPct = showNow
    ? Math.min(Math.max((Date.now() - dayStartMs) / DAY_MS, 0), 1) * 100
    : null;

  // 刻度全在轨道下方那条带上，不压数据 —— 之前那条 22% 透明竖线穿过了
  // 整个色块区，碎片一多就成了噪声，读时间反而要眯眼。
  const ticks: { x: number; major: boolean }[] = [];
  for (let m = 0; m < MINUTES_PER_DAY; m += MINOR_STEP_MIN) {
    ticks.push({ x: (m / MINUTES_PER_DAY) * WIDTH, major: m % MAJOR_STEP_MIN === 0 });
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
            const selected = selectedId === p.seg.id;
            return (
              <rect
                key={p.seg.id}
                data-id={p.seg.id}
                x={p.x}
                y={4}
                width={p.w}
                height={TRACK_H - 8}
                rx={1.5}
                fill={colorForCategory(p.seg.category)}
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
                onClick={() => onSelect(p.seg)}
                onMouseMove={(e) => {
                  const box = e.currentTarget.ownerSVGElement!.getBoundingClientRect();
                  const xPct = ((e.clientX - box.left) / box.width) * 100;
                  setHover({ placed: p, xPct });
                }}
                onMouseLeave={() => setHover(null)}
              >
                {/* 原生 title 兜底：浮层还没渲染出来前也有提示 */}
                <title>
                  {`${clockOf(p.seg.startAt)} – ${clockOf(p.seg.endAt)} · ${p.seg.category} · ${formatDuration(p.seg.endAt - p.seg.startAt)}`}
                </title>
              </rect>
            );
          })}

          {/* 刻度尺：10 分钟一根小刻度、60 分钟一根大刻度，底端对齐。
              一天 144 根，用 stroke 而非 DOM 节点，省得为装饰铺 144 个元素。 */}
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
            这层覆盖层只盖住轨道那 64px —— 直接相对整个容器 top-1/2 会把标签
            推到轨道底边去蹭刻度尺。 */}
        <div className="pointer-events-none absolute inset-x-0 top-0 h-16">
          {placed.map((p) => {
            const meta = metaForCategory(p.seg.category);
            if (!meta.inlineLabel || p.w < LABEL_MIN_W) return null;
            return (
              <span
                key={`l-${p.seg.id}`}
                aria-hidden
                className="absolute top-1/2 -translate-y-1/2 truncate text-[11px] leading-none font-medium"
                style={{
                  left: `${p.leftPct + (p.widthPct - (GAP / WIDTH) * 100) / 2}%`,
                  width: `${p.widthPct}%`,
                  textAlign: "center",
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

      {hover && (
        <div
          role="tooltip"
          className="pointer-events-none absolute z-20 -translate-x-1/2 -translate-y-full rounded-md border border-line-strong bg-surface-3 px-2.5 py-1.5 text-xs whitespace-nowrap shadow-pop"
          style={{ left: `${Math.min(Math.max(hover.xPct, 8), 92)}%`, top: -8 }}
        >
          <span
            className="mr-1.5 inline-block size-2 rounded-[2px] align-middle"
            style={{ background: colorForCategory(hover.placed.seg.category) }}
            aria-hidden
          />
          <span className="font-medium">{labelForCategory(hover.placed.seg.category)}</span>
          <span className="text-ink-muted"> · {hover.placed.seg.application ?? "未知应用"}</span>
          <div className="tnum mt-0.5 text-ink-muted">
            {clockOf(hover.placed.seg.startAt)} – {clockOf(hover.placed.seg.endAt)}
            <span className="mx-1 text-ink-ghost">|</span>
            {formatDuration(hover.placed.seg.endAt - hover.placed.seg.startAt)}
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
