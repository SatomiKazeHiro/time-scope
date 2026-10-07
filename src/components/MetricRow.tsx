import type { ReactNode } from "react";
import DayPartChart from "./DayPartChart";
import DonutChart from "./DonutChart";
import { formatDuration } from "../lib/bucket";
import type { DonutSlice, Summary } from "../types";

interface MetricRowProps {
  summary: Summary | null;
  loading: boolean;
  /** 当前范围的名字，落在英雄数字下面当上下文（"这 1094 小时是多长时间的"）。 */
  rangeLabel?: string;
  /** 范围跨多少天。用来算日均 —— 没有它，1094 小时是个没有参照的数。 */
  rangeDays?: number;
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

function Label({ children }: { children: ReactNode }) {
  return <span className="block text-micro text-ink-faint">{children}</span>;
}

/**
 * 顶部指标区。
 *
 * 之前是**五个一模一样的圆角盒子**，里面塞三种不同性质的东西：一个累计读数、
 * 一个比例构成、一条时间分布。行高被环形卡撑起来，另外四张各空掉近一半 ——
 * 空白不是留白，是没安排内容的余地。
 *
 * 现在按**信息性质**排，不按数量排：
 *
 * | 位置 | 内容 | 读法 |
 * |---|---|---|
 * | 英雄数字 | 监控总时长 + 范围 | 「我记了多久」——整页唯一的大数 |
 * | 主体 | 活跃占比（含两段时长） | 「多少是我真的在用」 |
 * | 主体 | 类别构成 | 「花在什么上」 |
 * | 主体 | 24h 时段分布（撑满卡高） | 「什么时段在动」 |
 * | 诊断行 | 平均段长 + 峰值 + 段数 | 引擎口径，**不占大字** |
 *
 * 段数与切换次数是**引擎诊断量**（「引擎切成了 458 段」）。问「时间去哪了」的
 * 人不需要它当主读数，所以降到脚注行；原先算得出却没显示的平均段长一并补上。
 */
export default function MetricRow({ summary, loading, rangeLabel, rangeDays }: MetricRowProps) {
  const has = summary !== null;
  const s = summary;
  const activePct = s && s.totalMs > 0 ? Math.round((Math.max(s.activeMs, 0) / s.totalMs) * 100) : 0;
  // 段数为 0 时不写 0 —— 那会读成「每段 0 分钟」
  const avgMs = s && s.segmentCount > 0 ? Math.round(s.activeMs / s.segmentCount) : 0;
  // 日均：把总量锚到一个可比的尺度上。「1094 小时」本身没有参照，
  // 「日均 3 小时」才有 —— 它让人判断「这算多还是少」。
  const perDayMs = s && rangeDays && rangeDays > 0 ? Math.round(s.totalMs / rangeDays) : 0;

  return (
    <div
      data-metric-row=""
      className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-12"
      aria-busy={loading || undefined}
    >
      {/* ① 英雄数字：整页唯一的大数。下面两行是它的**参照**——
             只有范围，1094 小时读不出多少；有了日均，它才变成「一天 3 小时」。 */}
      <div data-metric-card="total" className="panel flex flex-col p-3 sm:col-span-1 lg:col-span-3">
        <Label>监控总时长</Label>
        <b
          className={`tnum mt-0.5 block text-lg leading-tight font-semibold tracking-tight ${
            has ? "text-ink" : "text-ink-faint"
          }`}
        >
          {has ? formatDuration(s!.totalMs) : PLACEHOLDER}
        </b>
        <span className="mt-auto block pt-2 text-micro text-ink-ghost">
          {rangeLabel ?? " "}
        </span>
        <span className="tnum block text-micro text-ink-ghost">
          {/* 单日范围下「日均」等于总量，是句废话 —— 只在跨多天时才有意义 */}
          {has && rangeDays && rangeDays > 1 && perDayMs > 0
            ? `日均 ${formatDuration(perDayMs)}`
            : " "}
        </span>
      </div>

      {/* ② 活跃占比。主读数是百分比 ——「多少是我真的在用」；
             两段时长退到下面。比例条与监控页「当日汇总」同款。 */}
      <div data-metric-card="active" className="panel p-3 lg:col-span-2">
        <Label>活跃占比</Label>
        <b
          className={`tnum block text-md leading-tight font-semibold ${
            has ? "text-ink" : "text-ink-faint"
          }`}
        >
          {has ? `${activePct}%` : PLACEHOLDER}
        </b>
        <div className="mt-1.5 flex h-2 overflow-hidden rounded-full bg-surface-2">
          <span
            className="h-full bg-mark-fill"
            style={{ width: `${Math.max(activePct, activePct > 0 ? 1 : 0)}%` }}
          />
        </div>
        <span className="tnum mt-1 block text-micro text-ink-ghost">
          {has ? `活跃 ${formatDuration(s!.activeMs)}` : " "}
        </span>
        <span className="tnum block text-micro text-ink-ghost">
          {has ? `空闲 ${formatDuration(s!.idleMs)}` : " "}
        </span>
      </div>

      {/* ③ 类别构成。环 + 四行图例。 */}
      <div data-metric-card="mix" className="panel p-3 lg:col-span-4">
        <Label>类别构成</Label>
        <DonutChart donut={s?.donut ?? EMPTY_DONUT} totalMs={s?.totalMs ?? 0} />
      </div>

      {/* ④ 24h 分布。图表撑满卡高 —— 原来 74px 的图浮在 150px 盒子顶上，下面一半是空的。 */}
      <div data-metric-card="hours" className="panel flex flex-col p-3 lg:col-span-3">
        <Label>24h 时段分布</Label>
        <div className="mt-1 min-h-0 flex-1">
          <DayPartChart hourlyMs={s?.hourlyMs ?? EMPTY_24H} stretch />
        </div>
        <span className="tnum mt-1 block text-micro text-ink-ghost">
          {has ? `最忙 ${peakHour(s!.hourlyMs)}` : " "}
        </span>
      </div>

      {/* ⑤ 诊断行：段越短注意力切得越碎。原来「458 段 / 切换 12 次」占着一张卡，
             而平均段长这个一眼能看懂的量反而没显示。 */}
      <div
        data-metric-card="segments"
        className="panel flex flex-wrap items-baseline gap-x-2 gap-y-1 p-3 sm:col-span-2 lg:col-span-12"
      >
        <span className="text-micro text-ink-faint">平均每段</span>
        <b className={`tnum text-sm font-semibold ${has ? "text-ink" : "text-ink-faint"}`}>
          {has && avgMs > 0 ? formatDuration(avgMs) : PLACEHOLDER}
        </b>
        <span className="tnum text-micro text-ink-ghost">
          {has ? `· ${s!.segmentCount} 段 · 切换 ${s!.switchCount} 次` : " "}
        </span>
        <span className="text-micro text-ink-ghost">
          —— 段越短，注意力切得越碎
        </span>
      </div>
    </div>
  );
}

/** 最忙时段：一天里非空闲时长最高的那一小时。 */
function peakHour(hourlyMs: number[]): string {
  const h = hourlyMs.length === 24 ? hourlyMs : new Array(24).fill(0);
  const max = Math.max(...h);
  if (max <= 0) return "无峰值";
  return `${String(h.indexOf(max)).padStart(2, "0")}:00`;
}