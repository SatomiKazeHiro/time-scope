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
      <b className="tnum block text-[17px] leading-tight font-semibold text-ink">{value}</b>
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
    <div className="grid gap-3 lg:grid-cols-4">
      <Card label="监控总时长" value={formatDuration(summary.totalMs)} />
      <Card
        label="活跃 / 空闲"
        value={`${formatDuration(summary.activeMs)} / ${formatDuration(summary.idleMs)}`}
        sub={`${activePct}% 活跃`}
      />
      <div className="rounded-lg border border-line bg-surface-1 px-3 py-2">
        <span className="mb-1 block text-[10.5px] text-ink-faint">类别构成</span>
        <DonutChart donut={summary.donut} totalMs={summary.totalMs} />
      </div>
      <Card
        label="活动段"
        value={`${summary.segmentCount} 段`}
        sub={`切换 ${summary.switchCount} 次`}
      />
    </div>
  );
}
