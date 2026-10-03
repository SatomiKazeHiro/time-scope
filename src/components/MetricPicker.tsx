import type { MetricMode } from "./SegmentTimeline";
import { METRIC_LABEL } from "./SegmentTimeline";
import { FOCUS_LEGEND, SWITCH_LEGEND, scaleColor, SCALE_STEPS } from "../lib/metrics";

const MODES: MetricMode[] = ["category", "focus", "switch"];

/**
 * 时间线看什么：类别 / 专注度 / 切换次数。
 *
 * 三态都是**互斥**的，不会同时出现在一个视图里 —— 这也是紫色顺序色阶
 * 可以离类别色那么近的原因（蓝 ↔ 紫在红色盲下会塌陷，见 MASTER §2.5）。
 * 所以模式切换必须在界面上一眼可辨：这个控件、色阶图例、还有时间线
 * 上方那句「当前指标」都在回答同一个问题。
 */
export default function MetricPicker({
  value,
  onChange,
}: {
  value: MetricMode;
  onChange: (m: MetricMode) => void;
}) {
  return (
    <div
      role="group"
      aria-label="时间线指标"
      className="inline-flex gap-0.5 rounded-md border border-line bg-surface-1 p-0.5"
    >
      {MODES.map((m) => {
        const active = value === m;
        return (
          <button
            key={m}
            type="button"
            onClick={() => onChange(m)}
            aria-pressed={active}
            className={`cursor-pointer rounded-sm px-2.5 py-1 text-sm transition-colors duration-[--duration-fast] ${
              active
                ? "bg-neutral-solid font-semibold text-on-neutral"
                : "text-ink-muted hover:bg-surface-2 hover:text-ink"
            }`}
          >
            {METRIC_LABEL[m]}
          </button>
        );
      })}
    </div>
  );
}

/**
 * 顺序色阶图例。
 *
 * **连续量没有图例就读不出数值** —— 用户看到一排深浅不一的紫，
 * 不可能自己反推出"多深算高专注"。所以指标模式下图例必须常驻。
 * 类别模式不显示：那个由当日汇总面板的分类行充当图例。
 */
export function ScaleLegend({ metric }: { metric: MetricMode }) {
  if (metric === "category") return null;
  const legend = metric === "focus" ? FOCUS_LEGEND : SWITCH_LEGEND;

  return (
    <div
      aria-label={`${METRIC_LABEL[metric]}色阶图例`}
      className="flex items-center gap-2 text-micro text-ink-faint"
    >
      <span>{legend[0]}</span>
      <span className="flex">
        {Array.from({ length: SCALE_STEPS }, (_, i) => (
          <span
            key={i}
            className="h-2.5 w-4 first:rounded-l-sm last:rounded-r-sm"
            style={{ background: scaleColor(i + 1) }}
          />
        ))}
      </span>
      <span>{legend[legend.length - 1]}</span>
    </div>
  );
}
