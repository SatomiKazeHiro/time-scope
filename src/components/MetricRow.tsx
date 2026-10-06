import DayPartChart from "./DayPartChart";
import DonutChart from "./DonutChart";
import { formatDuration } from "../lib/bucket";
import type { DonutSlice, Summary } from "../types";

interface MetricRowProps {
  summary: Summary | null;
  loading: boolean;
}

/** 没数据时的占位符。用「—」而不是 0，0 会被读成「这天真的没活动」。 */
const PLACEHOLDER = "—";

/** 空圆环：四档都是 0，弧长为 0，**几何与有数据时完全一致**（高度不变）。 */
const EMPTY_DONUT: DonutSlice[] = [
  { key: "work", ms: 0 },
  { key: "browsing", ms: 0 },
  { key: "idle", ms: 0 },
  { key: "unknown", ms: 0 },
];

/** 空 24h：24 根 0 高柱，viewBox 高度固定。 */
const EMPTY_24H: number[] = new Array(24).fill(0);

function Card({
  label, value, sub, muted,
}: {
  label: string;
  value: string;
  sub?: string;
  /** 还没数据：文字压淡，但**占位照样在**，高度不变 */
  muted?: boolean;
}) {
  return (
    <div
      data-metric-card=""
      className="rounded-lg border border-line bg-surface-1 px-3 py-2"
    >
      <span className="block text-[10.5px] text-ink-faint">{label}</span>
      <b
        className={`tnum block text-[15px] leading-tight font-semibold ${
          muted ? "text-ink-faint" : "text-ink"
        }`}
      >
        {value}
      </b>
      {/* 副标题**永远占一格**。之前没数据时传空串，<i> 元素在、没内容，
          行高由 CSS 撑着 —— 两态差 16px，页面还是会「弹一下」。 */}
      <i className="block h-[14px] text-[10px] leading-[14px] text-ink-ghost not-italic">
        {sub ?? ""}
      </i>
    </div>
  );
}

/**
 * 顶部指标横条。五张卡：总时长 / 活跃·空闲 / 圆环 / 段数·切换 / 24h 分布。
 *
 * **外壳永远渲染，数据没到时显示占位符。**
 * 之前没数据就整行 return 一行「加载中…」，5 张卡全消失 —— 布局塌陷把
 * 下面的内容顶上移，数据到了再展开推下来，看着像「页面内容弹了一下」。
 * 现在卡片数量、列数、圆环与图表的几何都不随数据有无变化。
 */
export default function MetricRow({ summary, loading }: MetricRowProps) {
  const has = summary !== null;
  const s = summary;
  const activePct =
    s && s.totalMs > 0 ? Math.round((Math.max(s.activeMs, 0) / s.totalMs) * 100) : 0;

  return (
    <div
      data-metric-row=""
      className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-6"
      aria-busy={loading || undefined}
    >
      <Card
        label="监控总时长"
        muted={!has}
        value={s ? formatDuration(s.totalMs) : PLACEHOLDER}
      />
      <Card
        label="活跃 / 空闲"
        muted={!has}
        value={
          s
            ? `${formatDuration(s.activeMs)} / ${formatDuration(s.idleMs)}`
            : PLACEHOLDER
        }
        sub={s ? `${activePct}% 活跃` : undefined}
      />
      {/* 圆环 + 四行图例是五张卡里最宽的，跨两列 */}
      <div
        data-metric-card=""
        className="rounded-lg border border-line bg-surface-1 px-3 py-2 lg:col-span-2"
      >
        <span className="mb-1 block text-[10.5px] text-ink-faint">类别构成</span>
        <DonutChart donut={s?.donut ?? EMPTY_DONUT} totalMs={s?.totalMs ?? 0} />
      </div>
      <Card
        label="活动段"
        muted={!has}
        value={s ? `${s.segmentCount} 段` : PLACEHOLDER}
        sub={s ? `切换 ${s.switchCount} 次` : undefined}
      />
      <div
        data-metric-card=""
        className="rounded-lg border border-line bg-surface-1 px-3 py-2"
      >
        <span className="block text-[10.5px] text-ink-faint">24h 时段分布</span>
        <DayPartChart hourlyMs={s?.hourlyMs ?? EMPTY_24H} compact />
      </div>
    </div>
  );
}
