import { useMemo } from "react";
import {
  buildWall, selectionFrame, uninstalledColor, uninstalledStroke,
  GAP, scaleColor, scaleStroke,
  type DateRange,
} from "../lib/summary";
import type { DayCell } from "../types";

/**
 * 列宽（`--cw`）与间隙。选中框、月份标签、`grid-template-rows` 全从它算，
 * 墙随容器缩放时它们不会错位。
 *
 * 用 **cqw（容器查询单位）** 而不是 `%`：`grid-template-rows` 里的百分比
 * 解析的是**块向**尺寸（高度），而高度是 auto，于是循环依赖、行高塌成 0。
 * cqw 解析的是**行内**尺寸（宽度），宽度是定值，所以成立。
 */
function gridVars(weeks: number): React.CSSProperties {
  return {
    "--cw": `calc((100cqw - (${weeks} - 1) * var(--gap)) / ${weeks})`,
    "--gap": `${GAP}px`,
  } as React.CSSProperties;
}

interface ContributionWallProps {
  /** **连续升序**的日期序列（`lib/summary.ts` 的 `fillDays` 产出） */
  days: DayCell[];
  /**
   * 首个有记录的日子。这之前的格子画成「还没装」——
   * 和「装了但当天没活动」是两回事（见 `uninstalledColor`）。
   */
  trackedFrom?: string;
  /** `null` = 当前是「全部」，不框 */
  selection: DateRange | null;
  onSelectDay(date: string): void;
  onSelectWeek(sunday: string): void;
  onSelectMonth(date: string): void;
}

/**
 * 监控时长的 GitHub 贡献墙。7 行 = 周日到周六，1 列 = 1 周。
 *
 * **框是跨格的一整块绝对定位矩形，不是逐格描边** —— 框的宽度本身就
 * 告诉用户当前框的是日、周还是月（spec §5.3）。
 * 框外**不压暗**：聚光灯方案会让远处月份的对比度跌破 3:1。
 */
export default function ContributionWall({
  days, trackedFrom, selection, onSelectDay, onSelectWeek, onSelectMonth,
}: ContributionWallProps) {
  const layout = useMemo(() => buildWall(days, trackedFrom), [days, trackedFrom]);
  const frame = useMemo(
    () => (selection ? selectionFrame(layout, selection.from, selection.to) : null),
    [layout, selection],
  );

  if (layout.cells.length === 0) {
    return <p className="text-sm text-ink-faint">还没有采集数据。</p>;
  }

  /**
   * 某列的**周日**（row 0）。补齐位也有真实日期，所以每周都拿得到 ——
   * 这才是 `onSelectWeek` 声明的那个参数。`rangeFor("week", …)` 内部
   * 还会再 snap 一次，两处一致才不会被传错日期坑到。
   */
  const sundayOf = (ci: number): string | undefined =>
    layout.cells.find((c) => c.col === ci && c.row === 0)?.date;

  return (
    // 外层是 query container：cqw 量的是**它**的行内尺寸（= 面板内容宽），
    // 内层才能拿到「一列多宽」。
    <div className="w-full" style={{ containerType: "inline-size" }}>
      <div className="w-full" style={gridVars(layout.weeks)}>
      <div className="relative mb-1 h-3 w-full">
        {layout.monthLabels.map((m) => (
          <button
            key={`${m.label}-${m.col}`}
            type="button"
            data-testid={`month-label-${m.date.slice(0, 7)}`}
            onClick={() => onSelectMonth(m.date)}
            className="absolute cursor-pointer text-[9px] whitespace-nowrap text-ink-faint hover:text-ink"
            style={{ left: `calc(var(--cw) * ${m.col} + var(--gap) * ${m.col})` }}
          >
            {m.label}
          </button>
        ))}
      </div>

      <div className="relative w-full" style={gridVars(layout.weeks)}>
        <div
          role="grid"
          aria-label="监控时长"
          className="grid"
          style={{
            // 1fr 均分而不是写死像素：53 列写死 11px 只占 742px，
            // 在 1080px 的面板上右边空三分之一。
            gridTemplateColumns: `repeat(${layout.weeks}, minmax(0, 1fr))`,
            // **行高必须显式给**：不写 grid-template-rows 时
            // `grid-auto-flow: column` 只排出一行，整面墙的高度塌成 0。
            gridTemplateRows: `repeat(7, var(--cw))`,
            // 必须列优先：一列自上而下填满 7 行才换列，否则整个周会被打横
            gridAutoFlow: "column",
            gap: GAP,
          }}
        >
          {/* 补齐位**必须留在 DOM 里占槽位**。网格是 grid-auto-flow:column
              按列填的，删掉开头的补齐位会让整面墙往上错位 lead 行 ——
              10-01（周四）会画到第 0 行（周日），整个星期对错 4 天。
              它们透明、aria-hidden、不可点，但占着。 */}
          {layout.cells.map((c) =>
            c.present ? (
              <button
                key={c.date}
                type="button"
                role="gridcell"
                data-date={c.date}
                data-testid={`cell-${c.date}`}
                title={
                  c.tracked
                    ? `${c.date} · ${Math.round((c.totalMs / 3_600_000) * 10) / 10}h`
                    : `${c.date} · 还没装 Time Scope`
                }
                onClick={() => onSelectDay(c.date)}
                className="cursor-pointer rounded-[2px] outline-offset-1 focus-visible:outline-1 focus-visible:outline-ink"
                style={{
                  background: c.tracked ? scaleColor(c.step) : uninstalledColor(),
                  boxShadow: c.tracked ? scaleStroke(c.step) : uninstalledStroke(),
                }}
              />
            ) : (
              <span key={c.date} aria-hidden style={{ background: "transparent" }} />
            ),
          )}
        </div>

        {/* 选中框：一整块。跨 cols 列 × rows 行，位置全部由 --cw 算 */}
        {frame && (
          <span
            data-frame=""
            aria-hidden
            className="pointer-events-none absolute rounded-[3px] border-[1.5px] border-ink"
            style={{
              left: `calc(var(--cw) * ${frame.col} + var(--gap) * ${frame.col})`,
              top: `calc(var(--cw) * ${frame.row} + var(--gap) * ${frame.row})`,
              width: `calc(var(--cw) * ${frame.cols} + var(--gap) * ${frame.cols - 1})`,
              height: `calc(var(--cw) * ${frame.rows} + var(--gap) * ${frame.rows - 1})`,
            }}
          />
        )}
      </div>

      {/* 周条：格子下方那条空隙。点了就是选一周。 */}
      <div
        data-testid="week-strip"
        className="mt-1.5 grid w-full"
        style={{
          gridTemplateColumns: `repeat(${layout.weeks}, minmax(0, 1fr))`,
          columnGap: GAP,
        }}
      >
        {/* 53 周全是真实日历周，都可选 —— 选一个空周看到的是零，不是禁止 */}
        {Array.from({ length: layout.weeks }, (_, ci) => {
          const s = sundayOf(ci);
          return (
            <button
              key={ci}
              type="button"
              data-testid="week-strip-btn"
              aria-label={s ? `选择 ${s} 那一周` : "这一周"}
              disabled={!s}
              onClick={() => s && onSelectWeek(s)}
              // 同样的毛病：surface-2 在浅色下等于透明。MASTER §2.4 的既定解法。
              style={{ boxShadow: "inset 0 0 0 1px var(--color-line-strong)" }}
              className="h-1 cursor-pointer rounded-sm bg-surface-2 transition-colors hover:bg-ink-ghost disabled:cursor-default"
            />
          );
        })}
      </div>

      <p className="mt-1 text-[10px] text-ink-faint">
        左起为周日 → 周六，一列一周。点格子选一天，点下方细条选一周，点月份选整月。
      </p>
      </div>
    </div>
  );
}
