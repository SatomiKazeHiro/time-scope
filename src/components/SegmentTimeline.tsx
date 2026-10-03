import { useState } from "react";
import { formatDuration } from "../lib/bucket";
import { colorForCategory, labelForCategory, labelInkFor, metaForCategory } from "../design/categories";
import type { Category, Segment } from "../types";

const DAY_MS = 86_400_000;
const WIDTH = 1000;
const TRACK_H = 56;
/** 极短段也要看得见：0 宽的 rect 等于没画，用户会以为数据丢了。 */
const MIN_WIDTH = 1;
/** 相邻填充之间留 2 单位底色缝。viewBox 1000 铺满约 1168px，2 单位 ≈ 2.3px。
    没有这道缝，两段不同类别贴在一起时边界只能靠色差硬读。 */
const GAP = 2;
/** 窄于此宽度不直标：文字放不下，硬塞会盖住相邻段。 */
const LABEL_MIN_W = 46;
/** 3 小时一格太密、6 小时一格对不齐小数；这里只标 0/6/12/18/24。 */
const AXIS_LABELS = [0, 6, 12, 18, 24];
const AXIS_TICKS = [0, 3, 6, 9, 12, 15, 18, 21];

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
 */
export default function SegmentTimeline({
  segments,
  dayStartMs,
  onSelect,
  selectedId,
}: Props) {
  const [hover, setHover] = useState<{ placed: Placed; xPct: number } | null>(null);

  if (segments.length === 0) {
    return (
      <div
        className="flex h-14 items-center px-3 text-sm text-ink-faint"
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

  return (
    <div>
      <div className="relative">
        <svg
          width="100%"
          height={TRACK_H}
          viewBox={`0 0 ${WIDTH} ${TRACK_H}`}
          preserveAspectRatio="none"
          role="img"
          aria-label="24h 活动时间线"
          className="block h-14 w-full rounded-sm bg-surface-1"
        >
          {/* 刻度线用 <line> 而不是 <rect>：测试用 rect 数量断言段数，
              多画一个 rect 就会把"渲染了一个段"读成两个。 */}
          {AXIS_TICKS.map((h) => (
            <line
              key={h}
              x1={(h / 24) * WIDTH}
              x2={(h / 24) * WIDTH}
              y1={0}
              y2={TRACK_H}
              stroke="var(--color-ink-ghost)"
              /* 非等比拉伸下不指定的话，竖线的描边会被横向放大 */
              vectorEffect="non-scaling-stroke"
              opacity={0.28}
            />
          ))}

          {placed.map((p) => {
            const selected = selectedId === p.seg.id;
            return (
              <rect
                key={p.seg.id}
                data-id={p.seg.id}
                x={p.x}
                y={selected ? 0 : 3}
                width={p.w}
                height={selected ? TRACK_H : TRACK_H - 6}
                rx={1}
                fill={colorForCategory(p.seg.category)}
                vectorEffect="non-scaling-stroke"
                className="cursor-pointer"
                /* 描边兼两职：选中时是 2px 亮环（选中用明度表达，不占用类别色相）；
                   未选中时是 8px 透明描边 —— 1 分钟的段在 24h 里只有约 4px 宽，
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
        </svg>

        {/* 段内直标：能不能打是结构决策（inlineLabel），什么颜色由 CSS 随主题决定 */}
        {placed.map((p) => {
          const meta = metaForCategory(p.seg.category);
          if (!meta.inlineLabel || p.w < LABEL_MIN_W) return null;
          return (
            <span
              key={`l-${p.seg.id}`}
              aria-hidden
              className="pointer-events-none absolute top-1/2 -translate-y-1/2 truncate text-[11px] leading-none font-medium"
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

        {hover && (
          <div
            role="tooltip"
            className="pointer-events-none absolute z-20 -translate-x-1/2 -translate-y-full rounded-md border border-line-strong bg-surface-3 px-2.5 py-1.5 text-xs whitespace-nowrap shadow-pop"
            style={{ left: `${Math.min(Math.max(hover.xPct, 8), 92)}%`, top: -6 }}
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
      </div>

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
