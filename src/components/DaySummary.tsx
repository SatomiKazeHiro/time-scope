import { formatDuration, summarize } from "../lib/bucket";
import { colorForCategory, metaForCategory } from "../design/categories";
import type { Category, Segment } from "../types";

interface Props {
  segments: Segment[];
  dayStartMs: number;
}

/**
 * 当日汇总：各类别时长与占比（spec §10）。数据源是 `get_segments` 的原始段，后端不另给 summary。
 *
 * **这个面板不只是"读数"，它还是整张时间线的图例。** 八个类别里有两档
 * （study / life）的色块打不了段内直标，身份识别就落在这里 —— 所以每行都必须
 * 同时有色块**和**中文类名，识别不能只靠颜色。
 */
export default function DaySummary({ segments, dayStartMs }: Props) {
  const rows = summarize(segments, dayStartMs);
  if (rows.length === 0) return null;

  return (
    <section aria-label="当日汇总" className="panel p-4">
      <div className="mb-3 flex items-baseline justify-between">
        <h2 className="panel-title">当日汇总</h2>
        <span className="tnum text-micro text-ink-ghost">{rows.length} 个类别</span>
      </div>

      <ul className="m-0 flex list-none flex-col gap-2 p-0">
        {rows.map((r) => (
          <SummaryRow key={r.category} category={r.category} ratio={r.ratio} durationMs={r.durationMs} />
        ))}
      </ul>
    </section>
  );
}

function SummaryRow({
  category,
  ratio,
  durationMs,
}: {
  category: Category;
  ratio: number;
  durationMs: number;
}) {
  const meta = metaForCategory(category);
  const pct = Math.max(Math.round(ratio * 100), 1);

  return (
    <li className="flex items-center gap-3">
      <span
        className="size-2.5 shrink-0 rounded-[2px] shadow-[inset_0_0_0_1px_var(--color-line-strong)]"
        style={{ background: colorForCategory(category) }}
        aria-hidden
      />
      {/* 原始 key 留在 title 里：用户要拿它去 rules.toml 里对规则 */}
      <span className="w-12 shrink-0 text-sm text-ink" title={category}>
        {meta.label}
      </span>
      <div className="h-2.5 flex-1 overflow-hidden rounded-full bg-surface-2">
        <div
          className="h-full rounded-full"
          style={{ width: `${pct}%`, background: colorForCategory(category) }}
        />
      </div>
      <span className="tnum w-24 shrink-0 text-right text-sm whitespace-nowrap text-ink-muted">
        {formatDuration(durationMs)}
      </span>
      <span className="tnum w-9 shrink-0 text-right text-micro text-ink-ghost">{pct}%</span>
    </li>
  );
}
