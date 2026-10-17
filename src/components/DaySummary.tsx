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

  // **没有段时仍然渲染这个面板**（2026-10-17 用户报的 bug）。
  //
  // 这里原来 `return null`，后果不是「少一块内容」而是**布局塌一格**：
  // 下方那行是两列网格，左边这一格没了，`SegmentDetail` 就被排到左格、
  // 右边空出一大片 —— 看起来像界面坏了。面板在，格子就在；内容换成一句空状态。
  //
  // 空状态不说「这一天没有活动」：没有段 ≠ 有记录但全是空闲，
  // 它是「这一天压根还没有数据」（明天、或者应用没跑过的日子）。
  if (rows.length === 0) {
    return (
      <section aria-label="当日汇总" className="panel p-4">
        <div className="mb-3 flex items-baseline justify-between">
          <h2 className="panel-title">当日汇总</h2>
        </div>
        <p className="m-0 text-sm text-ink-faint">这一天还没有数据。</p>
      </section>
    );
  }

  // spec §10：「各 category 时长条形 + 活跃/空闲比」。
  // 活跃 = 非 idle 的时长。`unknown` 算活跃 —— 它是"没分类出是什么"，
  // 不是"没在做事"，把未分类的时间算成空闲会低估人实际在用电脑。
  const idleMs = rows.find((r) => r.category === "idle")?.durationMs ?? 0;
  const activeMs = rows.reduce((sum, r) => sum + r.durationMs, 0) - idleMs;
  const total = activeMs + idleMs;
  const activePct = total > 0 ? Math.round((activeMs / total) * 100) : 0;

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
            className="h-full bg-mark-fill"
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
