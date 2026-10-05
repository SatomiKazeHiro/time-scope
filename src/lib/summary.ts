/**
 * 汇总页的纯逻辑层：热力图布局、色阶分档、选中框、范围推导。
 *
 * 全部是无 IO 的纯函数——布局算法是这个页面最容易错的地方
 * （周日起始、首列补齐、跨格框），把它从 React 组件里剥出来，
 * 才能一条条地断言。
 *
 * 热力图口径见 spec §5.1：**7 行 = 周日到周六**（row 0 是周日），
 * 1 列 = 1 周（自周日起）。
 */

import { shiftDate, todayString, type DayCell } from "../types";

/** 格子尺寸常量。年的设计以后定，列数策略也只动 buildWall 一处。 */
export const CELL = 11;
export const GAP = 3;
/** 格子在一行里占的宽度（含间隙）—— 选中框用它算绝对定位。 */
export const PITCH = CELL + GAP;

/** 一个月至少隔这么多列才画第二个月份标签，否则标签会叠在一起。 */
const MONTH_LABEL_MIN_GAP = 4;

/** `YYYY-MM-DD` 的本地星期，0 = 周日。 */
export function weekdayOf(date: string): number {
  const [y, m, d] = date.split("-").map(Number);
  return new Date(y, m - 1, d).getDay();
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

export interface WallCell {
  date: string;
  totalMs: number;
  col: number;
  /** 0 = 周日 */
  row: number;
  /** false = 补齐位（数据范围之外），不渲染但占位，保证不整列错位 */
  present: boolean;
  step: number;
}

export interface WallLayout {
  cells: WallCell[];
  weeks: number;
  monthLabels: Array<{ col: number; label: string }>;
}

/**
 * 把连续的日期序列摊成 GitHub 贡献墙的网格。
 *
 * 首列往前补到周日、末列往后补到周六 —— 不补的话第一列和最后一列
 * 会整列错位（spec §5.1）。补齐后总格数恒为 7 的倍数。
 *
 * **前置条件：`days` 必须是连续、升序的日期序列。** 缺日会算错补齐量
 * （末列位置由 `days[last].date` 决定）。调用方用 `fillDays` 产出，
 * 它保证连续；`totalMs` 为 0 的日子照样在序列里。
 */
export function buildWall(days: DayCell[]): WallLayout {
  if (days.length === 0) return { cells: [], weeks: 0, monthLabels: [] };

  const maxMs = Math.max(...days.map((d) => d.totalMs), 0);
  const first = days[0].date;
  const last = days[days.length - 1].date;
  const lead = weekdayOf(first);
  // 首列往前补 lead 天到周日，末列往后补 (6 - weekday(last)) 天到周六。
  // 这三项相加恒是 7 的倍数（补齐后不留半列）。
  const total = days.length + lead + (6 - weekdayOf(last));

  const cells: WallCell[] = [];
  const monthLabels: Array<{ col: number; label: string }> = [];
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
      step: scaleStep(totalMs, maxMs),
    });

    if (present) {
      const month = Number(cursor.slice(5, 7));
      if (month !== lastMonth) {
        // 一个月只标一次；离上一个标签太近就不画（GitHub 的做法）
        if (col - lastLabelCol >= MONTH_LABEL_MIN_GAP) {
          monthLabels.push({ col, label: `${month}月` });
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

/** 该日期所在周的**周日**。 */
export function startOfWeek(date: string): string {
  return shiftDate(date, -weekdayOf(date));
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

/** 给用户看的范围文案。 */
export function rangeLabel(r: DateRange): string {
  if (r.kind === "all") return "全部";
  if (r.kind === "day") return r.from;
  if (r.kind === "week") return `${r.from} – ${r.to}`;
  return r.label;
}
