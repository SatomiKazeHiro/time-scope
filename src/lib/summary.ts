/**
 * 汇总页的纯逻辑层：热力图布局、色阶分档、选中框、范围推导。
 *
 * 全部是无 IO 的纯函数——布局算法是这个页面最容易错的地方
 * （周日起始、首列补齐、跨格框），把它从 React 组件里剥出来，
 * 才能一条条地断言。
 *
 * 热力图口径：**7 行 = 周日到周六**（行 0 = 周日），1 列 = 1 周（自周日起），
 * 与 GitHub 贡献墙一致（左侧标 Mon / Wed / Fri）。
 */

import { shiftDate, todayString, type DayCell } from "../types";

/** 格子尺寸常量。年的设计以后定，列数策略也只动 buildWall 一处。 */
export const CELL = 11;
export const GAP = 3;
/** 格子在一行里占的宽度（含间隙）—— 选中框用它算绝对定位。 */
export const PITCH = CELL + GAP;

/**
 * 一个月至少隔这么多列才画第二个月份标签，否则标签会叠在一起。
 *
 * 2 而不是 4：标签「10月」在 9px 字下约 18px 宽，一列十几 px，
 * 隔 2 列（28px）就放得下。原先设 4 把只隔 2 列的相邻月吞掉了——
 * 5 个月的数据只标出 5/7/8/9/10，看着像「怎么只有 10 月的」。
 * GitHub 判的是像素，不是列数。
 */
const MONTH_LABEL_MIN_GAP = 2;

/**
 * 一行的星期标签，**下标 = 墙的行号**，行 0 = 周日。
 * 与 `weekdayOf`（JS 约定，也是 0 = 周日）同向，可以直接互换。
 */
export const WEEKDAY_LABELS = ["日", "一", "二", "三", "四", "五", "六"] as const;

/**
 * 左侧要标哪几行。GitHub 的贡献墙只标 Mon / Wed / Fri ——
 * 周日开头时它们是第 1/3/5 行。
 */
export const LABELLED_ROWS = [1, 3, 5] as const;

/** 热力图铺多少周。GitHub 的贡献墙是 53 周。 */
export const WALL_WEEKS = 53;

/**
 * 热力图的横轴窗口：**固定 53 周，最后一格是 `today`**。
 *
 * 固定跨度而不是「首个有数据的日子 → 今天」：那样只有 5 天数据时整面墙
 * 只有 2 列 1 个月份标签，看起来像渲染坏了。GitHub 不管你有多少数据都
 * 铺满一整年，没数据的日子是空的 —— 那个「空」本身就是「我那时候还没
 * 开始用」的信息。
 *
 * 首格从今天所在周的**周日**往前推 52 周，所以每列都对齐周。
 */
export function wallWindow(today: string): { start: string; end: string; weeks: number } {
  const end = today;
  const start = shiftDate(startOfWeek(end), -(WALL_WEEKS - 1) * 7);
  return { start, end, weeks: WALL_WEEKS };
}

/** `YYYY-MM-DD` 的本地星期，沿用 JS 约定：**0 = 周日**。 */
export function weekdayOf(date: string): number {
  const [y, m, d] = date.split("-").map(Number);
  return new Date(y, m - 1, d).getDay();
}

/**
 * 这天在墙里的**行号**：0 = 周日 … 6 = 周六。
 *
 * 与 `weekdayOf` 同向同值（都是 0 = 周日），保留这个别名是为了让
 * 「行号」和「星期几」在读代码时不说混。
 */
export function rowOf(date: string): number {
  return weekdayOf(date);
}

/** 从 `from` 到 `to` 的连续日期序列，没记录的填 0。 */
export function fillDays(
  first: string,
  last: string,
  counts: Map<string, number>,
): DayCell[] {
  const out: DayCell[] = [];
  // 上限是防御性的：万一调用方传了乱序或畸形范围，别把这里变成死循环
  for (let i = 0; i < 4000; i++) {
    if (first > last) break;
    out.push({ date: first, totalMs: counts.get(first) ?? 0 });
    first = shiftDate(first, 1);
  }
  return out;
}

/** 分成 0..5 六档。0 是轨道色（0 活跃就该看起来是空的），1..5 走紫阶。 */
export function scaleStep(ms: number, maxMs: number): number {
  if (ms <= 0) return 0;
  const r = ms / Math.max(maxMs, 1);
  if (r <= 0.2) return 1;
  if (r <= 0.4) return 2;
  if (r <= 0.6) return 3;
  if (r <= 0.8) return 4;
  return 5;
}

/** 档位 -> CSS 颜色。0 用轨道色，1..5 用指标模式那套紫阶。 */
export function scaleColor(step: number): string {
  return step === 0 ? "var(--color-surface-2)" : `var(--color-scale-${step})`;
}

/**
 * 还没装 Time Scope 的那段日子怎么画。
 *
 * **它和「装了但当天没活动」是两件事，必须分开。** GitHub 的空格有意义是因为
 * GitHub 一直存在；Time Scope 三个月前还不存在，那段日子的空格读作「我一整年
 * 几乎没用过」——而事实是「我 5 天前才装上」。
 *
 * 两者**底色相同**（轨道色），只差描边强弱：
 * - 未安装：弱描边 `--color-line`，读作「还没装的幽灵格子」
 * - 没活动：强描边 `--color-line-strong`，读作「有个确切的空方块」
 *
 * 曾经把未安装做成面板色（等于背景），结果整片白茫茫 —— 看不见就不是区分。
 * 不新增 token。
 */
export function uninstalledColor(): string {
  return "var(--color-surface-2)";
}

export function uninstalledStroke(): string {
  return "inset 0 0 0 1px var(--color-line)";
}

/** 0 档格子的内描边。
 *
 * `--color-surface-2` 在浅色主题下是 #f4f5f7，压在 #ffffff 的面板上
 * 几乎看不见 —— 「哪天没活动」就变成了「哪里是空白」，读不出信息。
 * MASTER §2.4 对「颜色太淡看不见」有既定解法：1px 内描边，用
 * `--color-line-strong`，**不新增 token**。
 */
export function scaleStroke(step: number): string | undefined {
  return step === 0 ? "inset 0 0 0 1px var(--color-line-strong)" : undefined;
}

export interface WallCell {
  date: string;
  totalMs: number;
  col: number;
  /** 0 = 周日 */
  row: number;
  /** false = 补齐位（窗口之外），不渲染但占位，保证不整列错位 */
  present: boolean;
  /**
   * 这天是否在 Time Scope 的运行期内。
   * false = 还没装（`uninstalledColor()`），true = 装了但可能没活动（0 档）。
   */
  tracked: boolean;
  step: number;
}

export interface WallLayout {
  cells: WallCell[];
  weeks: number;
  /** `date` 是**该月第一天**，不是那一列的列首日期 —— 月首常落在周中，
   *  拿列首去推 rangeFor("month") 会选中上一个月。 */
  monthLabels: Array<{ col: number; label: string; date: string }>;
}

/**
 * 把连续的日期序列摊成 GitHub 贡献墙的网格。
 *
 * 首列往前补到周日、末列往后补到周六 —— 不补的话第一列和最后一列
 * 会整列错位。补齐后总格数恒为 7 的倍数。
 *
 * **前置条件：`days` 必须是连续、升序的日期序列。** 缺日会算错补齐量
 * （末列位置由 `days[last].date` 决定）。调用方用 `fillDays` 产出，
 * 它保证连续；`totalMs` 为 0 的日子照样在序列里。
 */
export function buildWall(days: DayCell[], trackedFrom?: string): WallLayout {
  if (days.length === 0) return { cells: [], weeks: 0, monthLabels: [] };

  const first = days[0].date;
  const last = days[days.length - 1].date;
  const maxMs = Math.max(...days.map((d) => d.totalMs), 0);
  // 缺省为「从窗口第一天起就在运行」——调用方不传时不做未安装期区分。
  const since = trackedFrom ?? first;
  // 首列往前补到**周日**，末列往后补到周六。
  const lead = rowOf(first);
  const total = days.length + lead + (6 - rowOf(last));

  const cells: WallCell[] = [];
  const monthLabels: Array<{ col: number; label: string; date: string }> = [];
  let cursor = shiftDate(first, -lead);
  let lastMonth = -1;
  let lastLabelCol = -MONTH_LABEL_MIN_GAP;

  for (let i = 0; i < total; i++) {
    const col = Math.floor(i / 7);
    const row = i % 7;
    const present = i >= lead && i < lead + days.length;
    const totalMs = present ? (days[i - lead]?.totalMs ?? 0) : 0;
    cells.push({
      date: cursor,
      totalMs,
      col,
      row,
      present,
      tracked: present && cursor >= since,
      step: scaleStep(totalMs, maxMs),
    });

    if (present) {
      const month = Number(cursor.slice(5, 7));
      if (month !== lastMonth) {
        // 一个月只标一次；离上一个标签太近就不画（GitHub 的做法）
        if (col - lastLabelCol >= MONTH_LABEL_MIN_GAP) {
          monthLabels.push({ col, label: `${month}月`, date: cursor });
          lastLabelCol = col;
        }
        lastMonth = month;
      }
    }
    cursor = shiftDate(cursor, 1);
  }

  return { cells, weeks: Math.ceil(total / 7), monthLabels };
}

/** 选中框的尺寸，单位是「格」。`cols`/`rows` 决定框跨多大一块。 */
export interface Frame {
  col: number;
  row: number;
  cols: number;
  rows: number;
}

/**
 * 算出 `[from, to]` 在墙里占的那一块。
 *
 * 用**日历位置**算，不只取有数据的格子：用户选「10 月」时，框要框住整个月的
 * 7 行高度，跟那天有没有活动无关。补齐位有真实日期，所以直接按日期过滤
 * `cells` 就能得到日历矩形。
 *
 * 范围内一天都不在这面墙上时返回 `null`。
 */
export function selectionFrame(
  layout: WallLayout,
  from: string,
  to: string,
): Frame | null {
  const hits = layout.cells.filter((c) => c.date >= from && c.date <= to);
  if (hits.length === 0) return null;
  const col = Math.min(...hits.map((c) => c.col));
  const row = Math.min(...hits.map((c) => c.row));
  const colEnd = Math.max(...hits.map((c) => c.col));
  const rowEnd = Math.max(...hits.map((c) => c.row));
  return { col, row, cols: colEnd - col + 1, rows: rowEnd - row + 1 };
}

/** 该日期所在周的**周日**（墙里的一周从周日开始）。 */
export function startOfWeek(date: string): string {
  return shiftDate(date, -rowOf(date));
}

/** 该日期所在月的最后一天。 */
export function endOfMonth(date: string): string {
  const [y, m] = date.split("-").map(Number);
  // m = 0 会退成上个月最后一天（JS 的 Date 惯例）
  return todayString(new Date(y, m, 0));
}

export type RangeKind = "all" | "day" | "week" | "month";

export interface DateRange {
  kind: RangeKind;
  /** `YYYY-MM-DD`，闭区间 */
  from: string;
  to: string;
  label: string;
}

/** 从热力图上的一次点击推导出新的选中范围。 */
export function rangeFor(
  kind: RangeKind,
  date: string,
  all: DateRange,
): DateRange {
  switch (kind) {
    case "all":
      return all;
    case "day":
      return { kind, from: date, to: date, label: date };
    case "week": {
      const from = startOfWeek(date);
      return { kind, from, to: shiftDate(from, 6), label: "本周" };
    }
    case "month": {
      const from = `${date.slice(0, 7)}-01`;
      return {
        kind,
        from,
        to: endOfMonth(date),
        label: `${Number(date.slice(5, 7))}月`,
      };
    }
  }
}

/**
 * 给用户看的范围文案。
 *
 * 默认档叫「近一年」而不是「全部」：它就是墙铺的那 53 周窗口。
 * 叫「全部」会跟「我有数据的全部区间」混淆 —— 那只有 5 天，
 * 而墙上是 53 周，三处口径必须一致。
 */
export function rangeLabel(r: DateRange): string {
  if (r.kind === "all") return "近一年";
  if (r.kind === "day") return r.from;
  if (r.kind === "week") return `${r.from} – ${r.to}`;
  return r.label;
}
