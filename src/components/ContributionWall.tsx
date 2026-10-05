import { useMemo } from "react";
import {
  buildWall, selectionFrame, CELL, GAP, PITCH, scaleColor, scaleStroke,
  type DateRange,
} from "../lib/summary";
import type { DayCell } from "../types";

interface ContributionWallProps {
  /** **连续升序**的日期序列（`lib/summary.ts` 的 `fillDays` 产出） */
  days: DayCell[];
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
  days, selection, onSelectDay, onSelectWeek, onSelectMonth,
}: ContributionWallProps) {
  const layout = useMemo(() => buildWall(days), [days]);
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

  const width = layout.weeks * PITCH - GAP;

  return (
    <div className="inline-block">
      {/* 月份标签：浮在每月首次出现的那一列上方 */}
      <div className="relative mb-1 h-3" style={{ width }}>
        {layout.monthLabels.map((m) => {
          // 用标签自带的该月首日，不是那一列的第一个有数据的格子
          const d = m.date;
          return (
            <button
              key={`${m.label}-${m.col}`}
              type="button"
              data-testid={`month-label-${m.date.slice(0, 7)}`}
              onClick={() => onSelectMonth(d)}
              className="absolute cursor-pointer text-[9px] whitespace-nowrap text-ink-faint hover:text-ink"
              style={{ left: m.col * PITCH }}
            >
              {m.label}
            </button>
          );
        })}
      </div>

      <div className="relative inline-block" style={{ width }}>
        <div
          role="grid"
          aria-label="监控时长"
          className="grid"
          style={{
            gridTemplateColumns: `repeat(${layout.weeks}, ${CELL}px)`,
            gridTemplateRows: `repeat(7, ${CELL}px)`,
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
                title={`${c.date} · ${Math.round((c.totalMs / 3_600_000) * 10) / 10}h`}
                onClick={() => onSelectDay(c.date)}
                className="cursor-pointer rounded-[2px] outline-offset-1 focus-visible:outline-1 focus-visible:outline-ink"
                style={{ background: scaleColor(c.step), boxShadow: scaleStroke(c.step) }}
              />
            ) : (
              <span key={c.date} aria-hidden style={{ background: "transparent" }} />
            ),
          )}
        </div>

        {/* 选中框：一整块。宽 = 跨的列数 × PITCH - GAP */}
        {frame && (
          <span
            data-frame=""
            aria-hidden
            className="pointer-events-none absolute rounded-[3px] border-[1.5px] border-ink"
            style={{
              left: frame.col * PITCH,
              top: frame.row * PITCH,
              width: frame.cols * PITCH - GAP,
              height: frame.rows * PITCH - GAP,
            }}
          />
        )}
      </div>

      {/* 周条：格子下方那条空隙。点了就是选一周。 */}
      <div
        data-testid="week-strip"
        className="mt-1.5 grid"
        style={{ gridTemplateColumns: `repeat(${layout.weeks}, ${CELL}px)`, columnGap: GAP }}
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
              className="h-1 cursor-pointer rounded-sm bg-surface-2 transition-colors hover:bg-ink-ghost disabled:cursor-default"
            />
          );
        })}
      </div>

      <p className="mt-1 text-[10px] text-ink-faint">
        左起为周日 → 周六，一列一周。点格子选一天，点下方细条选一周，点月份选整月。
      </p>
    </div>
  );
}
