import DayPartChart from "./DayPartChart";
import DonutChart from "./DonutChart";
import { formatDuration } from "../lib/bucket";
import type { Summary } from "../types";

interface MetricRowProps {
  summary: Summary | null;
  loading: boolean;
}

function Card({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <div className="rounded-lg border border-line bg-surface-1 px-3 py-2">
      <span className="block text-[10.5px] text-ink-faint">{label}</span>
      <b className="tnum block text-[15px] leading-tight font-semibold text-ink">{value}</b>
      {sub && <i className="block text-[10px] text-ink-ghost not-italic">{sub}</i>}
    </div>
  );
}

/** 顶部指标横条。四张卡：总时长 / 活跃·空闲 / 圆环 / 段数·切换。 */
export default function MetricRow({ summary, loading }: MetricRowProps) {
  if (!summary) {
    return <p className="text-sm text-ink-faint">{loading ? "加载中…" : "暂无数据"}</p>;
  }
  const activePct =
    summary.totalMs > 0
      ? Math.round((Math.max(summary.activeMs, 0) / summary.totalMs) * 100)
      : 0;

  return (
    /* 24h 时段分布是**一张指标卡**，不是热力图下面的全宽条 */
    <div data-metric-row="" className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-6">
      <Card label="监控总时长" value={formatDuration(summary.totalMs)} />
      <Card
        label="活跃 / 空闲"
        value={`${formatDuration(summary.activeMs)} / ${formatDuration(summary.idleMs)}`}
        sub={`${activePct}% 活跃`}
      />
      {/* 圆环 + 四行图例是五张卡里最宽的，跨两列 */}
      <div className="rounded-lg border border-line bg-surface-1 px-3 py-2 lg:col-span-2">
        <span className="mb-1 block text-[10.5px] text-ink-faint">类别构成</span>
        <DonutChart donut={summary.donut} totalMs={summary.totalMs} />
      </div>
      <Card
        label="活动段"
        value={`${summary.segmentCount} 段`}
        sub={`切换 ${summary.switchCount} 次`}
      />
      <div className="rounded-lg border border-line bg-surface-1 px-3 py-2">
        <span className="block text-[10.5px] text-ink-faint">24h 时段分布</span>
        <DayPartChart hourlyMs={summary.hourlyMs} compact />
      </div>
    </div>
  );
}
