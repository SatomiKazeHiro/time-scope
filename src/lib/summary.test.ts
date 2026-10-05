import { describe, it, expect } from "vitest";
import {
  buildWall, endOfMonth, fillDays, rangeFor, scaleColor, scaleStep,
  scaleStroke, selectionFrame, startOfWeek, wallWindow, weekdayOf,
  type DateRange,
} from "./summary";
import type { DayCell } from "../types";

const ALL: DateRange = { kind: "all", from: "2026-10-01", to: "2026-10-05", label: "全部" };

function days(spec: Array<[string, number]>): DayCell[] {
  return spec.map(([date, totalMs]) => ({ date, totalMs }));
}

/** 从 `from` 起连续 `count` 天的 [date, 1] 序列。 */
/** 复刻组件里给格子算 style 的那段逻辑，测它而不测 DOM。 */
function cellStyle(c: { tracked: boolean; step: number }): string {
  if (!c.tracked) return "bg: surface-1";
  return `bg: ${scaleColor(c.step)}; box-shadow: ${scaleStroke(c.step) ?? "none"}`;
}

function range(from: string, count: number): Array<[string, number]> {
  const [y, m, d] = from.split("-").map(Number);
  return Array.from({ length: count }, (_, i) => {
    const dt = new Date(y, m - 1, d + i);
    const p = (n: number) => String(n).padStart(2, "0");
    return [`${dt.getFullYear()}-${p(dt.getMonth() + 1)}-${p(dt.getDate())}`, 1] as [string, number];
  });
}

/** 2026-10-01 是周四，10-05 是周一 —— 这组日期能同时验首列补齐与跨列。 */
describe("weekdayOf", () => {
  it("周日返回 0", () => {
    expect(weekdayOf("2026-10-04")).toBe(0);
  });
  it("周六返回 6", () => {
    expect(weekdayOf("2026-10-03")).toBe(6);
  });
  it("与 Date 的一致", () => {
    for (let d = 1; d <= 31; d++) {
      const s = `2026-10-${String(d).padStart(2, "0")}`;
      expect(weekdayOf(s)).toBe(new Date(2026, 9, d).getDay());
    }
  });
});

describe("fillDays", () => {
  it("把没有记录的日期补成 totalMs 0", () => {
    const counts = new Map([["2026-10-01", 100], ["2026-10-04", 200]]);
    const out = fillDays("2026-10-01", "2026-10-05", counts);
    expect(out.map((d) => d.date)).toEqual([
      "2026-10-01", "2026-10-02", "2026-10-03", "2026-10-04", "2026-10-05",
    ]);
    expect(out.map((d) => d.totalMs)).toEqual([100, 0, 0, 200, 0]);
  });

  it("from 晚于 to 时返回空数组而不是死循环", () => {
    expect(fillDays("2026-10-05", "2026-10-01", new Map())).toEqual([]);
  });

  it("单日范围返回一个元素", () => {
    expect(fillDays("2026-10-01", "2026-10-01", new Map())).toEqual([
      { date: "2026-10-01", totalMs: 0 },
    ]);
  });
});

describe("scaleStep", () => {
  it("0 小时落轨道色档 0", () => {
    expect(scaleStep(0, 10_000)).toBe(0);
  });
  it("四档边界取闭区间下界", () => {
    const max = 1000;
    expect(scaleStep(1, max)).toBe(1);      // 0.001
    expect(scaleStep(200, max)).toBe(1);
    expect(scaleStep(201, max)).toBe(2);
    expect(scaleStep(400, max)).toBe(2);
    expect(scaleStep(401, max)).toBe(3);
    expect(scaleStep(600, max)).toBe(3);
    expect(scaleStep(601, max)).toBe(4);
    expect(scaleStep(800, max)).toBe(4);
    expect(scaleStep(801, max)).toBe(5);
    expect(scaleStep(max, max)).toBe(5);
  });
  it("maxMs 为 0 时不产生 NaN", () => {
    expect(Number.isFinite(scaleStep(0, 0))).toBe(true);
    expect(scaleStep(0, 0)).toBe(0);
  });
  it("档 0 走轨道色，1..5 走紫阶", () => {
    expect(scaleColor(0)).toBe("var(--color-surface-2)");
    expect(scaleColor(1)).toBe("var(--color-scale-1)");
    expect(scaleColor(5)).toBe("var(--color-scale-5)");
  });
});

describe("wallWindow", () => {
  // 热力图铺**固定 12 个月**，不随数据量伸缩 —— GitHub 就是这样。
  // 以前是「首个有数据的日子 → 今天」，所以 5 天数据只有 2 列 1 个月份标签。
  const W = wallWindow("2026-10-05");
  const WEEKS = 53;

  it("窗口是 53 周", () => {
    expect(W.weeks).toBe(WEEKS);
  });

  it("最后一格是给定的那天", () => {
    expect(W.end).toBe("2026-10-05");
  });

  it("第一格是周日（列从周日起算）", () => {
    expect(weekdayOf(W.start)).toBe(0);
  });

  it("首格距本周周日正好 52 周", () => {
    // start 是**本周周日**往前 52 周，不是距今天 52 周 ——
    // 距今天是 364 + weekday(today) 天，锚在周日才能让每列对齐。
    const [y, m, d] = W.start.split("-").map(Number);
    const startDate = new Date(y, m - 1, d);
    const sunday = new Date(2026, 9, 4);       // 2026-10-04，本周周日
    expect(Math.round((sunday.getTime() - startDate.getTime()) / 86_400_000))
      .toBe((WEEKS - 1) * 7);
  });
});

describe("buildWall 的固定跨度", () => {
  it("只有 5 天数据时，墙仍然是 53 列", () => {
    // 回归：以前按数据跨度画，5 天 -> 2 列 1 个月份标签，看着像坏了
    const w = wallWindow("2026-10-05");
    const five = days([
      ["2026-10-01", 1], ["2026-10-02", 2], ["2026-10-03", 3],
      ["2026-10-04", 4], ["2026-10-05", 5],
    ]);
    const filled = fillDays(w.start, w.end, new Map(five.map((d) => [d.date, d.totalMs])));
    const layout = buildWall(filled);
    expect(layout.weeks).toBe(53);
    // 网格是满的 53×7；末列的补齐位由 buildWall 补，present 的是窗口内真实日子
    expect(layout.cells).toHaveLength(53 * 7);
    // 网格末格是补齐到周六的 10-10；最后一个**有数据的**格子才是今天
    expect(layout.cells.at(-1)!.col).toBe(52);
    const lastReal = layout.cells.filter((c) => c.present).at(-1)!;
    expect(lastReal.date).toBe("2026-10-05");
    // 10-05 是周一 -> 第 1 行
    expect(lastReal.row).toBe(1);
  });

  it("窗口里没数据的日子是 0 档（轨道色），不是缺席", () => {
    const w = wallWindow("2026-10-05");
    const filled = fillDays(w.start, w.end, new Map([["2026-10-03", 3_600_000]]));
    const layout = buildWall(filled);
    const byDate = Object.fromEntries(layout.cells.map((c) => [c.date, c.step]));
    expect(byDate["2026-10-03"]).toBeGreaterThan(0);
    expect(byDate["2026-01-15"]).toBe(0);
  });

  it("12 个月里 12 个月份标签都在（列距够）", () => {
    const w = wallWindow("2026-10-05");
    const filled = fillDays(w.start, w.end, new Map());
    const labels = buildWall(filled).monthLabels.map((m) => m.label);
    expect(labels.length).toBeGreaterThanOrEqual(12);
    expect(labels).toContain("1月");
    expect(labels).toContain("10月");
  });
});

describe("未安装期", () => {
  // GitHub 的空格有意义是因为 GitHub 一直存在。Time Scope 三个月前还不存在，
  // 那段日子的空格读作「我一整年几乎没用过」——而事实是「我 5 天前才装上」。
  // 两种「空」必须分开：还没装 vs 装了没活动。
  const w = wallWindow("2026-10-05");
  // 10-03 特意给 0：装了但那天完全没活动
  const five = days([
    ["2026-10-01", 3_600_000], ["2026-10-02", 7_200_000], ["2026-10-03", 0],
    ["2026-10-04", 14_400_000], ["2026-10-05", 1_800_000],
  ]);
  const filled = fillDays(w.start, w.end, new Map(five.map((d) => [d.date, d.totalMs])));
  const layout = buildWall(filled, "2026-10-01");
  const byDate = Object.fromEntries(layout.cells.map((c) => [c.date, c]));

  it("首次采集之前的日期标为未安装", () => {
    expect(byDate["2026-05-15"].tracked).toBe(false);
    expect(byDate["2026-09-30"].tracked).toBe(false);
  });
  it("从首次采集当天起标为已跟踪（哪怕当天 0 时长）", () => {
    expect(byDate["2026-10-01"].tracked).toBe(true);
    expect(byDate["2026-10-05"].tracked).toBe(true);
  });
  it("未安装不参与色阶：再大的 max 也不改变它的样子", () => {
    expect(byDate["2026-05-15"].step).toBe(0);
  });
  it("两种空的样式不同", () => {
    // 未安装 = 面板色、无描边（读作「什么都没有」）
    expect(cellStyle(byDate["2026-05-15"])).not.toContain("line-strong");
    // 装了没活动 = 轨道色 + 描边（读作「有个空方块」）
    const noActivity = Object.entries(byDate).find(
      ([, c]) => c.tracked && c.totalMs === 0,
    )?.[1];
    expect(noActivity).toBeDefined();
    expect(cellStyle(noActivity!)).toContain("line-strong");
  });
});

describe("buildWall", () => {
  it("第一行是周日", () => {
    const w = buildWall(days([["2026-10-01", 1], ["2026-10-02", 2], ["2026-10-03", 3]]));
    for (const c of w.cells.filter((x) => x.row === 0)) {
      expect(weekdayOf(c.date)).toBe(0);
    }
  });

  it("每一行的 date 的星期都等于 row", () => {
    const w = buildWall(days([["2026-09-20", 1], ["2026-10-11", 2]]));
    for (const c of w.cells) {
      expect(weekdayOf(c.date)).toBe(c.row);
    }
  });

  it("首列往前补齐到周日：10-01 是周四，首列前 4 格是补齐位", () => {
    const w = buildWall(days([["2026-10-01", 1]]));
    const col0 = w.cells.filter((c) => c.col === 0);
    expect(col0).toHaveLength(7);
    // 周四 -> row 4；row 0..3 是补齐位
    expect(col0.slice(0, 4).every((c) => c.present === false)).toBe(true);
    expect(col0[4]).toMatchObject({ date: "2026-10-01", present: true, row: 4 });
  });

  it("末列往后补齐到周六", () => {
    const w = buildWall(days([["2026-10-05", 1]]));   // 周一
    const lastCol = w.cells.filter((c) => c.col === w.weeks - 1);
    const present = lastCol.filter((c) => c.present);
    expect(present).toHaveLength(1);
    expect(present[0].date).toBe("2026-10-05");
    expect(lastCol[6].present).toBe(false);
  });

  it("格子总数恒为 7 的倍数（补齐后不留半列）", () => {
    for (const start of ["2026-09-27", "2026-10-01", "2026-10-05", "2026-01-01"]) {
      const w = buildWall(days([[start, 1]]));
      expect(w.cells.length % 7).toBe(0);
      expect(w.weeks).toBe(w.cells.length / 7);
    }
  });

  it("只有一天数据时仍能画出整周（Review Focus #2）", () => {
    const w = buildWall(days([["2026-10-05", 3_600_000]]));
    expect(w.cells.filter((c) => c.present)).toHaveLength(1);
    expect(w.weeks).toBeGreaterThanOrEqual(1);
    // 补齐位不参与色阶，step 全是 0
    expect(w.cells.filter((c) => !c.present).every((c) => c.step === 0)).toBe(true);
  });

  it("空数据返回空布局而不是崩", () => {
    const w = buildWall([]);
    expect(w.cells).toEqual([]);
    expect(w.weeks).toBe(0);
    expect(w.monthLabels).toEqual([]);
  });

  it("色阶按最大值相对分档", () => {
    const w = buildWall(days([["2026-10-01", 10_000], ["2026-10-02", 2_500]]));
    const by = Object.fromEntries(
      w.cells.filter((c) => c.present).map((c) => [c.date, c.step]),
    );
    expect(by["2026-10-01"]).toBe(5);
    // 2500/10000 = 0.25，落在 (0.2, 0.4] -> 第 2 档
    expect(by["2026-10-02"]).toBe(2);
  });

  it("跨 4 个月每月各出一个标签（月初列距都 >= 4）", () => {
    const w = buildWall(days(range("2026-07-01", 120)));
    expect(w.monthLabels.map((m) => m.label)).toEqual(["7月", "8月", "9月", "10月"]);
  });

  it("相邻月份列距不足 4 列时后一个不画标签（GitHub 的做法）", () => {
    const w = buildWall(days([["2026-09-30", 1], ["2026-10-01", 1]]));
    const labels = w.monthLabels.map((m) => m.label);
    // 9 月在第 0 列，10 月只隔 1 列 -> 只应有 9 月
    expect(labels).toEqual(["9月"]);
  });

  it("相邻月只隔 2 列也要标出来（标签宽 ~18px，2 列 = 28px，够放）", () => {
    // 之前用 min gap 4，5 月(col0) 和 6 月(col2) 差 2 就被吞掉了 ——
    // 看着像「怎么只有 10 月的」。GitHub 判的是像素不是列数。
    const w = buildWall(days(range("2026-05-18", 140)));
    expect(w.monthLabels.map((m) => m.label))
      .toEqual(["5月", "6月", "7月", "8月", "9月", "10月"]);
  });

  it("只隔 1 列时仍不标（两个标签会叠在一起）", () => {
    const w = buildWall(days(range("2026-08-31", 4)));
    expect(w.monthLabels.map((m) => m.label)).toEqual(["8月"]);
  });
});

describe("selectionFrame", () => {
  // 10-01(周四) .. 10-10(周六)，连续 10 天 = 首列补 4 格 + 10 + 末列补 0 = 2 列
  const w = buildWall(days(range("2026-10-01", 10)));

  it("选中单日 = 1 格", () => {
    expect(selectionFrame(w, "2026-10-02", "2026-10-02")).toEqual({
      col: 0, row: 5, cols: 1, rows: 1,
    });
  });

  it("选中一个自然周 = 1 列 × 7 格", () => {
    // 10-04 是周日，10-04..10-10 才是完整的一周
    const f = selectionFrame(w, "2026-10-04", "2026-10-10");
    expect(f).not.toBeNull();
    expect(f!.cols).toBe(1);
    expect(f!.rows).toBe(7);
  });

  it("选中不足一周的区间 = 1 列，框住实际覆盖的那几行", () => {
    // 10-01 周四 / 10-02 周五：框 2 行，不是 7 行
    const f = selectionFrame(w, "2026-10-01", "2026-10-02")!;
    expect(f.cols).toBe(1);
    expect(f.rows).toBe(2);
  });

  it("范围伸进补齐位时仍按日历位置框满 7 行", () => {
    // 墙从 10-01(周四) 起，col 0 的前 4 格是补齐位。
    // 选 09-25..10-05 时范围伸进了补齐区，框仍要覆盖 10-05 那一列的全部 7 行。
    const f = selectionFrame(w, "2026-09-25", "2026-10-05")!;
    expect(f.col).toBe(0);
    expect(f.rows).toBe(7);
  });

  it("选中一个月 = 跨多列 × 7 格", () => {
    const w2 = buildWall(days(range("2026-09-01", 30)));
    const f = selectionFrame(w2, "2026-09-01", "2026-09-30")!;
    expect(f.rows).toBe(7);
    expect(f.cols).toBeGreaterThan(1);
  });

  it("范围里一天都没有时返回 null", () => {
    expect(selectionFrame(w, "2026-11-01", "2026-11-30")).toBeNull();
  });

  it("范围跨出数据两端时框住日历矩形（含补齐位）", () => {
    const f = selectionFrame(w, "2026-09-01", "2026-12-31")!;
    const firstCell = w.cells[0];
    const lastCell = w.cells.at(-1)!;
    expect(f.col).toBe(firstCell.col);
    expect(f.cols).toBe(lastCell.col - firstCell.col + 1);
  });
});

describe("范围推导", () => {
  it("startOfWeek 从周日起算", () => {
    // 2026-10-01 周四 -> 上一个周日 2026-09-27
    expect(startOfWeek("2026-10-01")).toBe("2026-09-27");
    expect(startOfWeek("2026-10-04")).toBe("2026-10-04");  // 本身是周日
  });

  it("endOfMonth 取当月最后一天", () => {
    expect(endOfMonth("2026-10-15")).toBe("2026-10-31");
    expect(endOfMonth("2026-02-10")).toBe("2026-02-28");
    expect(endOfMonth("2028-02-10")).toBe("2028-02-29");  // 闰年
  });

  it("rangeFor day 收敛成同一天", () => {
    const r = rangeFor("day", "2026-10-03", ALL);
    expect(r.from).toBe("2026-10-03");
    expect(r.to).toBe("2026-10-03");
  });

  it("rangeFor week 覆盖整周", () => {
    const r = rangeFor("week", "2026-10-01", ALL);
    expect(r.from).toBe("2026-09-27");
    expect(r.to).toBe("2026-10-03");
  });

  it("rangeFor month 覆盖整月", () => {
    const r = rangeFor("month", "2026-10-01", ALL);
    expect(r.from).toBe("2026-10-01");
    expect(r.to).toBe("2026-10-31");
  });

  it("rangeFor all 原样返回传入的 all", () => {
    expect(rangeFor("all", "2026-10-01", ALL)).toEqual(ALL);
  });
});
