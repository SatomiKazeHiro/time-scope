import { GRANULARITIES } from "../lib/bucket";

interface Props {
  value: number;
  onChange: (minutes: number) => void;
}

/**
 * 粒度切换器（spec §10：10 / 30（默认）/ 60 / 120 分钟）。切换只改前端参数，不重查后端。
 *
 * 分段控件而不是四个独立按钮：它们是一个维度的四个取值，不该看起来像四个操作。
 * 选中态用中性亮色而不是蓝色 —— 蓝是 work 的色相，界面层不借用数据色。
 */
export default function GranularityPicker({ value, onChange }: Props) {
  return (
    <div
      role="group"
      aria-label="时间粒度"
      className="inline-flex gap-0.5 rounded-md border border-line bg-surface-1 p-0.5"
    >
      {GRANULARITIES.map((m) => {
        const active = value === m;
        return (
          <button
            key={m}
            type="button"
            onClick={() => onChange(m)}
            aria-pressed={active}
            className={`tnum cursor-pointer rounded-sm px-2.5 py-1 text-sm transition-colors duration-[--duration-fast] ${
              active
                ? "bg-neutral-solid font-semibold text-on-neutral"
                : "text-ink-muted hover:bg-surface-2 hover:text-ink"
            }`}
          >
            {m}分
          </button>
        );
      })}
    </div>
  );
}
