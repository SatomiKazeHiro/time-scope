import { formatDuration } from "../lib/bucket";
import type { Category, Segment } from "../types";

const DAY_MS = 86_400_000;
const WIDTH = 1000;
const HEIGHT = 56;
/** 极短段也要看得见：0 宽的 rect 等于没画，用户会以为数据丢了。 */
const MIN_WIDTH = 1;

export const CATEGORY_COLOR: Record<Category, string> = {
  work: "#4c8dff",
  study: "#7c6cff",
  entertainment: "#f0605f",
  communication: "#26a69a",
  browsing: "#ffb74d",
  life: "#8d9e6c",
  idle: "#b0b6bd",
  unknown: "#e0e0e0",
};

export function colorForCategory(c: string): string {
  return CATEGORY_COLOR[c as Category] ?? CATEGORY_COLOR.unknown;
}

interface Props {
  segments: Segment[];
  /** 该日 00:00 的 Unix 毫秒（本地时区） */
  dayStartMs: number;
  onSelect: (s: Segment) => void;
}

/**
 * 24h 横向时间线：每个 ActivitySegment 一个矩形，按 category 着色，宽度正比于时长。
 *
 * 用 viewBox + width:100%，容器再窄（高 DPI、小窗口）也不会把 x/width 算坏。
 */
export default function SegmentTimeline({ segments, dayStartMs, onSelect }: Props) {
  if (segments.length === 0) {
    return <p style={{ color: "#666" }}>这一天还没有活动段。</p>;
  }
  const dayEnd = dayStartMs + DAY_MS;

  return (
    <svg
      width="100%"
      height={HEIGHT}
      viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
      preserveAspectRatio="none"
      role="img"
      aria-label="24h 活动时间线"
      style={{ display: "block", background: "#f1f3f5", borderRadius: 4 }}
    >
      {segments.map((s) => {
        // 跨零点或时区差可能让段越出当天，不夹会画出负 x 或超 viewBox
        const start = Math.max(s.startAt, dayStartMs);
        const end = Math.min(s.endAt, dayEnd);
        const w = Math.max(((end - start) / DAY_MS) * WIDTH, MIN_WIDTH);
        return (
          <rect
            key={s.id}
            data-id={s.id}
            x={((start - dayStartMs) / DAY_MS) * WIDTH}
            y={4}
            width={w}
            height={HEIGHT - 8}
            fill={colorForCategory(s.category)}
            style={{ cursor: "pointer" }}
            onClick={() => onSelect(s)}
          >
            <title>
              {`${new Date(s.startAt).toLocaleTimeString()} – ${new Date(s.endAt).toLocaleTimeString()} · ${s.category} · ${formatDuration(s.endAt - s.startAt)}`}
            </title>
          </rect>
        );
      })}
    </svg>
  );
}
