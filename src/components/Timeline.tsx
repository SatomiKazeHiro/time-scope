import { colorFor, type StoredEvent } from "../types";

const DAY_MS = 86_400_000;
const DEFAULT_WIDTH = 1000;
const DEFAULT_HEIGHT = 48;
const BLOCK_WIDTH = 3;

interface Props {
  events: StoredEvent[];
  /** 该日 00:00 的 Unix 毫秒（本地时区） */
  dayStartMs: number;
  width?: number;
  height?: number;
  onSelect: (e: StoredEvent) => void;
}

/**
 * 24h 横向时间线：每个 Event 一个窄色块。
 *
 * 骨架阶段按**事件类型**着色；分类色（work/browsing/...）属 engine task。
 * 用 viewBox + width:100%，容器再窄也不会把 x 算坏（高 DPI / 小窗口安全）。
 */
export default function Timeline({
  events,
  dayStartMs,
  width = DEFAULT_WIDTH,
  height = DEFAULT_HEIGHT,
  onSelect,
}: Props) {
  return (
    <svg
      width="100%"
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      role="img"
      aria-label="24h 活动时间线"
      style={{ display: "block", background: "#f1f3f5", borderRadius: 4 }}
    >
      {events.map((e) => {
        // 夹到 [0, DAY_MS-1]：跨时区/边界查询可能带进区间外的点，
        // 不夹会画出负 x 或超出 viewBox 的坐标。
        const offset = Math.min(Math.max(e.timestamp - dayStartMs, 0), DAY_MS - 1);
        const x = (offset / DAY_MS) * width;
        return (
          <rect
            key={e.id}
            x={x}
            y={4}
            width={BLOCK_WIDTH}
            height={height - 8}
            fill={colorFor(e.type)}
            style={{ cursor: "pointer" }}
            onClick={() => onSelect(e)}
          >
            <title>{`${new Date(e.timestamp).toLocaleTimeString()} · ${e.type}`}</title>
          </rect>
        );
      })}
    </svg>
  );
}
