import { formatDuration, summarize, unclassifiedApps } from "../lib/bucket";
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

  // spec §10：「各 category 时长条形 + 活跃/空闲比」。
  // 活跃 = 非 idle 的时长。`unknown` 算活跃 —— 它是"没分类出是什么"，
  // 不是"没在做事"，把未分类的时间算成空闲会低估人实际在用电脑。
  const idleMs = rows.find((r) => r.category === "idle")?.durationMs ?? 0;
  const activeMs = rows.reduce((sum, r) => sum + r.durationMs, 0) - idleMs;
  const total = activeMs + idleMs;
  const activePct = total > 0 ? Math.round((activeMs / total) * 100) : 0;

  // 「未分类 33%」本身没法行动 —— 得直接给出该往 rules.toml 补哪几个进程。
  const gaps = unclassifiedApps(segments, dayStartMs, 5);
  const gapTotalMs = gaps.reduce((s, g) => s + g.durationMs, 0);

  return (
    <section aria-label="当日汇总" className="panel p-4">
      <div className="mb-3 flex items-baseline justify-between">
        <h2 className="panel-title">当日汇总</h2>
        <span className="tnum text-micro text-ink-ghost">{rows.length} 个类别</span>
      </div>

      <div
        aria-label="活跃与空闲占比"
        className="mb-4 flex items-center gap-3 rounded-md bg-surface-2 px-3 py-2"
      >
        <span className="tnum w-9 text-sm font-semibold text-ink">{activePct}%</span>
        <div className="flex h-2.5 flex-1 overflow-hidden rounded-full">
          <div
            className="h-full bg-ink"
            style={{ width: `${Math.max(activePct, activePct > 0 ? 1 : 0)}%` }}
          />
          <div
            className="h-full bg-ink-ghost"
            style={{ width: `${Math.max(100 - activePct, idleMs > 0 ? 1 : 0)}%` }}
          />
        </div>
        <span className="tnum shrink-0 text-micro text-ink-faint">
          活跃 {formatDuration(activeMs)} · 空闲 {formatDuration(idleMs)}
        </span>
      </div>

      <ul className="m-0 flex list-none flex-col gap-2 p-0">
        {rows.map((r) => (
          <SummaryRow key={r.category} category={r.category} ratio={r.ratio} durationMs={r.durationMs} />
        ))}
      </ul>

      {gaps.length > 0 && (
        <div className="mt-4 rounded-md border border-line bg-surface-2 p-3">
          <div className="mb-2 flex items-baseline justify-between">
            <span className="text-label font-semibold text-ink">还没规则覆盖的程序</span>
            <span className="tnum text-micro text-ink-faint">{formatDuration(gapTotalMs)}</span>
          </div>
          <ul className="m-0 flex list-none flex-col gap-1 p-0">
            {gaps.map((g) => (
              <li key={g.application} className="flex items-baseline gap-2 text-sm">
                <code className="tnum min-w-0 flex-1 truncate text-ink-muted">
                  {g.application}
                </code>
                <span className="tnum shrink-0 text-micro text-ink-ghost">
                  {Math.round(g.ratio * 100)}%
                </span>
                <span className="tnum w-16 shrink-0 text-right text-micro text-ink-faint">
                  {formatDuration(g.durationMs)}
                </span>
              </li>
            ))}
          </ul>
          <p className="mt-2 text-micro text-ink-faint">
            在 <code className="tnum">%APPDATA%\time-scope\rules.toml</code> 加一条{" "}
            <code className="tnum">[[rule]]</code>，<code className="tnum">process</code> 填上面的文件名，
            重启后这段就会归到对应类别。
          </p>
        </div>
      )}
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
