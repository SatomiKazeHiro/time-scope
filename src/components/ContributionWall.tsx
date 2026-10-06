import { useCallback, useMemo } from "react";
import {
  buildWall, uninstalledColor,
  GAP, scaleColor, toneOf, LABELLED_ROWS, WEEKDAY_LABELS,
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

/** 周条的内描边。常量提到组件外：53 根每次渲染都新建对象没有意义。 */
const WEEK_STRIP_STYLE: React.CSSProperties = {
  boxShadow: "inset 0 0 0 1px var(--color-line-strong)",
};

/** 补齐位是不可见的占位（但仍要占槽位，见下方说明）。 */
const PAD_STYLE: React.CSSProperties = { background: "transparent" };

/**
 * 一个日格的可访问名（也用作它的原生 `title`）。
 *
 * 两种"空"必须分开说（STATUS 第 20、21 条）：界面上它们靠**描边强弱**区分，
 * 而读屏用户看不到描边 —— 名字就是他们唯一的区分通道。
 */
function dayLabel(c: { date: string; totalMs: number; tracked: boolean }): string {
  if (!c.tracked) return `${c.date} · 还没装 Time Scope`;
  const hours = Math.round((c.totalMs / 3_600_000) * 10) / 10;
  return `${c.date} · ${hours}h`;
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
 * 监控时长的 GitHub 式贡献墙，但**行序跟 GitHub 不同**：行 0 = 周一（ISO 8601），
 * 1 列 = 1 周（自周一起）。GitHub 是周日开头，这里按 ISO 走。
 *
 * 选中范围**逐格判断**（按格子自己的日期），不画跨格矩形 —— 跨格矩形要算
 * 「第 N 列第 M 行」再绝对定位，`--cw` 稍有偏差就整块错到隔壁列（已错过三轮）。
 * 每格的外环向外扩**半格间隙**，相邻选中格自然连成一整片，所以框的宽度
 * 仍然一眼能看出选的是日、周还是月。
 *
 * 框外**不压暗**（聚光灯）：最低档卡在 ordinal 的 2:1 底线上，任何压暗都破线
 * ——详见 theme.css 里 `.wall-cell[data-in-range]` 的注释。
 */
export default function ContributionWall({
  days, trackedFrom, selection, onSelectDay, onSelectWeek, onSelectMonth,
}: ContributionWallProps) {
  const layout = useMemo(() => buildWall(days, trackedFrom), [days, trackedFrom]);
  /** `--cw` 由列数算出来；外层容器与内层网格共用同一份。 */
  const vars = useMemo(() => gridVars(layout.weeks), [layout.weeks]);

  /** 某个格子是否落在当前选中范围内 —— 按**格子自己的日期**判断。 */
  const inSelection = useCallback(
    (date: string) =>
      selection !== null && date >= selection.from && date <= selection.to,
    [selection],
  );

  if (layout.cells.length === 0) {
    return <p className="text-sm text-ink-faint">还没有采集数据。</p>;
  }

  /**
   * 某列的**周一**（row 0）。补齐位也有真实日期，所以每周都拿得到 ——
   * 这才是 `onSelectWeek` 声明的那个参数。`rangeFor("week", …)` 内部
   * 还会再 snap 一次，两处一致才不会被传错日期坑到。
   *
   * **O(1) 取下标**：网格是列优先铺的（`buildWall` 按 i = col*7 + row 推），
   * 所以 `cells[ci * 7]` 必然就是第 ci 列的 row 0。以前用 `find` 扫全表，
   * 53 列 × 371 格 = 每次渲染两万次比较，纯浪费。
   */
  const weekStartOf = (ci: number): string | undefined => layout.cells[ci * 7]?.date;

  return (
    // **左内边距 + 绝对定位的标签列**，标签不占布局宽度。
    //
    // 这不是审美选择，是正确性：--cw 用 `100cqw` 量**这个容器**的
    // 行内尺寸。若让标签列正常占 12px + 6px 间距，网格会比容器窄 18px，
    // 而 --cw 仍按整宽算 —— 每列多算 0.34px，累到第 52 列正好偏**一整列**，
    // 周框看起来就落到下一列去了。
    <div
      /* relative 不能省：container-type 在 Chrome 里**不会**给绝对定位的
         后代当包含块，星期标签会跑到页面根上去（压住页头）。 */
      className="relative w-full pl-4"
      /* --cw 必须定义在容器上：星期标签列是内层 div 的兄弟节点，
         定义在内层它拿不到 -> calc() 整条失效 -> top 退回 auto ->
         三个标签叠在一处，只剩最后那个露出来。 */
      style={{ containerType: "inline-size", ...vars }}
    >
      <div className="w-full">
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

      <div className="relative w-full" style={vars}>
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
                /* 选中/未选中、空档/有活动都由这两个属性表达，
                   描边与选中外环在 theme.css 的 .wall-cell 规则里 ——
                   内描边和外环必须能叠加，只有 CSS 能做到不写笛卡尔积 */
                data-tone={toneOf(c)}
                data-in-range={inSelection(c.date) ? "" : undefined}
                /* title 与 aria-label **必须给同一句话**（B8）：`title` 只是
                   鼠标悬停才出现的视觉提示，读屏与键盘用户拿不到，原生 title
                   在触摸/键盘下也根本不弹。而这两种"空"在界面上是分开画的
                   （见 STATUS 第 20、21 条），读屏用户看不到描边强弱，
                   所以名字里也必须分开说。 */
                title={dayLabel(c)}
                aria-label={dayLabel(c)}
                onClick={() => onSelectDay(c.date)}
                className="wall-cell cursor-pointer rounded-[2px] outline-offset-1 focus-visible:outline-1 focus-visible:outline-ink"
                style={{ background: c.tracked ? scaleColor(c.step) : uninstalledColor() }}
              />
            ) : (
              <span key={c.date} aria-hidden style={PAD_STYLE} />
            ),
          )}
        </div>

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
          const s = weekStartOf(ci);
          return (
            <button
              key={ci}
              type="button"
              data-testid="week-strip-btn"
              aria-label={s ? `选择 ${s} 那一周` : "这一周"}
              disabled={!s}
              onClick={() => s && onSelectWeek(s)}
              // 同样的毛病：surface-2 在浅色下等于透明。MASTER §2.4 的既定解法。
              style={WEEK_STRIP_STYLE}
              className="h-1 cursor-pointer rounded-sm bg-surface-2 transition-colors hover:bg-ink-ghost disabled:cursor-default"
            />
          );
        })}
      </div>

      <p className="mt-1 text-[10px] text-ink-faint">
        一列一周，上到下是周一到周日。点格子选一天，点下方细条选一周，点月份选整月。
      </p>
      </div>

      {/* 星期标签：绝对定位在左侧留白里，GitHub 只标 Mon/Wed/Fri 三行 */}
      <div
        data-weekday-column=""
        aria-hidden
        className="absolute top-0 left-0 w-3 text-[9px] leading-none text-ink-faint"
      >
        {/* 占位：与月份标签行同高，把定位原点推到网格顶端 */}
        <div className="mb-1 h-3" data-weekday-spacer="" />
        <div className="relative" data-weekday-origin="">
          {LABELLED_ROWS.map((row) => (
            <span
              key={row}
              data-weekday={row}
              className="absolute right-0"
              /* 行 r 的中线：r 行之前是 r*(cw+gap)，再加半行高 */
              style={{
                top: `calc(var(--cw) * ${row} + var(--gap) * ${row} + var(--cw) / 2)`,
                transform: "translateY(-50%)",
              }}
            >
              {WEEKDAY_LABELS[row]}
            </span>
          ))}
        </div>
      </div>
    </div>
  );
}
