import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import ContributionWall from "./ContributionWall";
import { fillDays, wallWindow, type DateRange } from "../lib/summary";
import type { DayCell } from "../types";

/** 2026-10-01(周四) 起三天 —— 首列必然有 4 个补齐位。 */
const DAYS: DayCell[] = [
  { date: "2026-10-01", totalMs: 3_600_000 },
  { date: "2026-10-02", totalMs: 7_200_000 },
  { date: "2026-10-03", totalMs: 0 },
];

const NOOP = () => {};

function setup(selection: DateRange | null = null) {
  const onSelectDay = vi.fn();
  const onSelectWeek = vi.fn();
  const onSelectMonth = vi.fn();
  render(
    <ContributionWall
      days={DAYS}
      selection={selection}
      onSelectDay={onSelectDay}
      onSelectWeek={onSelectWeek}
      onSelectMonth={onSelectMonth}
    />,
  );
  return { onSelectDay, onSelectWeek, onSelectMonth };
}

describe("ContributionWall", () => {
  it("空数据给空状态，不画空墙", () => {
    // Review Focus #1：空库是新装用户的第一屏，不能画出一面假墙
    const { container } = render(
      <ContributionWall days={[]} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    expect(container.querySelector("[role='grid']")).toBeNull();
    expect(screen.getByText(/还没有采集数据/)).toBeTruthy();
  });

  it("有数据时渲染 7 行（周日到周六）", () => {
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    const grid = container.querySelector("[role='grid']") as HTMLElement;
    // 行高必须显式给 7 行。不写 grid-template-rows 时 grid-auto-flow:column
    // 只排出一行，整面墙高度塌成 0（真踩过）。
    expect(grid.style.gridTemplateRows).toContain("repeat(7");
    expect(grid.style.gridTemplateRows).toContain("var(--cw)");
    expect(grid.style.gridAutoFlow).toBe("column");
  });

  it("列优先填充：自上而下填满 7 行才换列，否则周会被打横", () => {
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    const grid = container.querySelector("[role='grid']") as HTMLElement;
    expect(grid.style.gridAutoFlow).toBe("column");
  });

  it("补齐位不进 data-date，但仍要占着槽位", () => {
    // 删掉补齐位会让 grid-auto-flow:column 的按列填充整体上移：
    // 10-01（周四）会画到第 0 行（周日），整个星期对错 4 天。
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    expect(container.querySelectorAll("[data-date]")).toHaveLength(3);
  });

  it("格子的 DOM 顺序按列优先，且总数含补齐位", () => {
    // 10-01(周四)..10-03(周六) 共 3 天，首列往前补 3 格到周一、
    // 末格已是周六不用补 -> 整面墙正好 1 列 7 格
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    const all = container.querySelectorAll("[role='grid'] > *");
    expect(all.length).toBe(7);
    // 补齐位没有 data-date，但仍在网格里占着。行 0 = 周一，
    // 10-01 是周四 -> 第 3 格。
    expect(all[3].getAttribute("data-date")).toBe("2026-10-01");
  });

  it("10-01 落在第 0 列的第 3 行（周四），不是第 0 行", () => {
    // 这是上一条的真实后果：补齐位被删就会跑到第 0 行（周一）去
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    const cells = [...container.querySelectorAll("[role='grid'] > *")];
    const idx = cells.findIndex((c) => c.getAttribute("data-date") === "2026-10-01");
    expect(idx).toBe(3);                 // 3 个补齐位之后
    expect(idx % 7).toBe(3);             // 第 3 行 = 周四
  });

  it("补齐位不可见也不可点", () => {
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    const pads = [...container.querySelectorAll("[role='grid'] > *")]
      .filter((c) => !c.getAttribute("data-date"));
    expect(pads.length).toBeGreaterThan(0);
    for (const p of pads) {
      expect((p as HTMLElement).style.background).toBe("transparent");
      expect(p.getAttribute("aria-hidden")).toBe("true");
    }
  });

  it("点格子回调那一天", () => {
    const { onSelectDay } = setup();
    fireEvent.click(screen.getByTestId("cell-2026-10-02"));
    expect(onSelectDay).toHaveBeenCalledWith("2026-10-02");
  });

  it("点周条回调那一周的周日", () => {
    const { onSelectWeek } = setup();
    fireEvent.click(screen.getAllByTestId("week-strip-btn")[0]);
    // 2026-10-01 是周四，首列从**周一** 2026-09-28 起
    expect(onSelectWeek).toHaveBeenCalledWith("2026-09-28");
  });

  it("点月标签回调那个月的首日", () => {
    const { onSelectMonth } = setup();
    fireEvent.click(screen.getByTestId("month-label-2026-10"));
    expect(onSelectMonth).toHaveBeenCalledWith("2026-10-01");
  });





  it("周条有内描边，否则浅色主题下 53 根细条看不见", () => {
    // 和 0 档格子同一个毛病：--color-surface-2 在 #ffffff 面板上等于透明。
    // 周条是「点一下选一周」的唯一入口，看不见就等于没有。
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    const strip = container.querySelectorAll("[data-testid='week-strip-btn']");
    expect(strip.length).toBeGreaterThan(0);
    for (const b of strip) {
      expect((b as HTMLElement).style.boxShadow)
        .toContain("--color-line-strong");
    }
  });

  it("列宽是响应式的：1fr 均分，不写死像素", () => {
    // 53 列写死 11px 只有 742px，面板宽 1080px 时右边空三分之一。
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    const grid = container.querySelector("[role='grid']") as HTMLElement;
    // 1fr 均分而不是写死像素
    expect(grid.style.gridTemplateColumns).toContain("1fr");
    expect(grid.style.gridTemplateColumns).not.toContain("px");
    // 行高用 cqw（容器查询单位）：grid-template-rows 里的百分比解析的是
    // 高度，而高度是 auto -> 循环依赖、行高塌 0
    expect(grid.style.gridTemplateRows).toContain("var(--cw)");
    const host = grid.closest('[style*="container-type"]') as HTMLElement;
    expect(host, "外层必须是 query container，cqw 才有得量").toBeTruthy();
  });

  it("墙整体占满容器宽度", () => {
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    const root = container.firstElementChild as HTMLElement;
    expect(root.className).toContain("w-full");
  });


  it("左侧 7 行全标，从周一到周日", () => {
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    const labels = [...container.querySelectorAll("[data-weekday]")];
    expect(labels.map((l) => l.textContent)).toEqual(["一","二","三","四","五","六","日"]);
    expect(labels.map((l) => l.getAttribute("data-weekday")))
      .toEqual(["0", "1", "2", "3", "4", "5", "6"]);
  });

  it("星期标签跟着 --cw 定位，不会跟格子错行", () => {
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    const wed = container.querySelector("[data-weekday='2']") as HTMLElement;
    const mon = container.querySelector("[data-weekday='0']") as HTMLElement;
    // 相邻两行差一个 (cw + gap)
    const d = (el: HTMLElement) =>
      Number(/\* (\d+)/.exec(el.style.top)?.[1] ?? -1);
    expect(d(wed) - d(mon)).toBe(2);
  });

  it("星期标签列顶部有与月份标签行同高的占位，否则整体偏上半行", () => {
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    const spacer = container.querySelector("[data-weekday-spacer]") as HTMLElement;
    expect(spacer).toBeTruthy();
    // 撑起的高度必须和月份标签行一致（h-3 + mb-1 = 12 + 4px），
    // 否则星期标签的绝对定位原点比格子高 16px，整体错半行。
    expect(spacer.className).toContain("h-3");
    expect(spacer.className).toContain("mb-1");
    const monthRow = container.querySelector(".h-3.w-full") as HTMLElement;
    expect(monthRow).toBeTruthy();
    expect(monthRow.className).toContain("h-3");
    // 占位必须在定位原点**里面**（它是原点的流内兄弟，靠它把原点推下去）
    const origin = container.querySelector("[data-weekday-origin]") as HTMLElement;
    expect(origin.previousElementSibling).toBe(spacer);
  });

  it("星期标签列绝对定位，不从网格宽度里挖走像素", () => {
    // 这是「点周框错一列」的真凶：标签列占 12px + 间距 6px 后，网格比
    // query container 窄 18px，而 --cw 仍按容器整宽算 -> 每列多 0.34px，
    // 累到第 52 列正好偏一列。标签必须移出布局流（absolute + 容器左内边距）。
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    const col = container.querySelector("[data-weekday-column]") as HTMLElement;
    expect(col.className).toContain("absolute");
    const host = col.closest('[style*="container-type"]') as HTMLElement;
    expect(host).toBeTruthy();
    // 容器用左内边距让出标签的位置，而不是让标签去挤网格
    expect(host.className).toMatch(/pl-\d/);
  });

  it("网格容器与 query container 等宽（--cw 的计算基准）", () => {
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    const grid = container.querySelector("[role='grid']") as HTMLElement;
    const host = grid.closest('[style*="container-type"]') as HTMLElement;
    // 网格不在任何会再吃宽度的 flex 子项里
    let n: HTMLElement | null = grid.parentElement;
    while (n && n !== host) {
      expect(n.className).not.toContain("shrink-0");
      n = n.parentElement;
    }
  });

  it("星期标签不抢焦点也不可点（纯说明）", () => {
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    for (const l of container.querySelectorAll("[data-weekday]")) {
      expect(l.tagName).toBe("SPAN");
      expect(l.closest("[aria-hidden]"), "整列应当对读屏隐藏").toBeTruthy();
    }
  });

  it("0 小时的格子用轨道色而不是紫阶 1", () => {
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    // 10-03 是 0h
    const zero = container.querySelector("[data-date='2026-10-03']") as HTMLElement;
    expect(zero.style.background).toContain("surface-2");
  });

  it("连续多日的墙有多列，周条也对应多根", () => {
    const many: DayCell[] = Array.from({ length: 20 }, (_, i) => ({
      date: `2026-09-${String(14 + i).padStart(2, "0")}`,
      totalMs: 3_600_000,
    }));
    const { container } = render(
      <ContributionWall days={many} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    const weeks = Number(
      (container.querySelector("[role='grid']") as HTMLElement)
        .style.gridTemplateColumns.match(/repeat\((\d+)/)![1],
    );
    // 09-14(周一)..10-03(周六) = 20 天 + 首列补 1 = 21 格 = 3 列
    expect(weeks).toBe(3);
    expect(container.querySelectorAll("[data-testid='week-strip-btn']")).toHaveLength(weeks);
  });
})

  it("选中范围靠格子自己的日期判定，不用跨格矩形", () => {
    const { container } = render(
      <ContributionWall
        days={DAYS}
        selection={{ kind: "week", from: "2026-09-28", to: "2026-10-04", label: "" }}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    // 跨格矩形已彻底删除
    expect(container.querySelector("[data-frame]")).toBeNull();
    // 范围内（以及范围内有数据的那几天）的格子被标记
    const marked = [...container.querySelectorAll("[data-in-range]")];
    expect(marked.length).toBeGreaterThan(0);
    for (const m of marked) {
      const d = m.getAttribute("data-date")!;
      expect(d >= "2026-09-28" && d <= "2026-10-04").toBe(true);
    }
  });

  it("没有选中时没有任何格子被标记", () => {
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    expect(container.querySelectorAll("[data-in-range]")).toHaveLength(0);
  });

  it("选一个月时，跨月的边界天不误标", () => {
    const { container } = render(
      <ContributionWall
        days={DAYS}
        selection={{ kind: "month", from: "2026-10-01", to: "2026-10-31", label: "" }}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    for (const m of container.querySelectorAll("[data-in-range]")) {
      expect(m.getAttribute("data-date")!.startsWith("2026-10-")).toBe(true);
    }
  });

  it("选中靠 box-shadow 外环连成一片，不再用 ::after 伪元素", () => {
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    // 组件不再注入 <style>，也不再有伪元素：描边全在 theme.css
    // （环半径 = 半格间隙这条几何在 tokens.test.ts 里对着 theme.css 断言）
    expect(container.querySelector("style")).toBeNull();
    expect(container.innerHTML).not.toContain("::after");
    // 选中的格子只靠 data-in-range 说话，不在 JS 里拼 box-shadow
    const marked = container.querySelectorAll("[data-in-range]");
    expect(marked.length).toBe(0);
    for (const el of container.querySelectorAll("[data-date]")) {
      expect((el as HTMLElement).style.boxShadow).toBe("");
    }
  });

  it("空档按 data-tone 分「还没装」和「装了但没活动」", () => {
    const mixed: DayCell[] = [
      { date: "2026-10-01", totalMs: 0 },          // 早于首次采集 -> 幽灵格子
      { date: "2026-10-02", totalMs: 7_200_000 },  // 有活动 -> 不描边
      { date: "2026-10-03", totalMs: 0 },          // 装了但当天 0 时长
    ];
    const { container } = render(
      <ContributionWall days={mixed} trackedFrom="2026-10-02" selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    const tone = (d: string) =>
      container.querySelector(`[data-date='${d}']`)!.getAttribute("data-tone");
    expect(tone("2026-10-01")).toBe("uninstalled");
    expect(tone("2026-10-02")).toBeNull();
    expect(tone("2026-10-03")).toBe("empty");
  });

  it("满窗口的槽位恰好 371，其中可点的「天」按钮 365~371", () => {
    // 墙固定铺 53 周（371 槽位），但窗口是「52 周前的周一 → 今天」，
    // 所以真正是「天」的格子数 = 364 + 今天在墙里的行号 + 1：
    // 周一 -> 365，周日 -> 371（一个补齐位都没有）。
    const w = wallWindow("2026-10-06");            // 周二 -> 366
    const filled = fillDays(w.start, w.end, new Map());
    const { container } = render(
      <ContributionWall days={filled} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    expect(container.querySelectorAll("[role='grid'] > *")).toHaveLength(371);
    expect(container.querySelectorAll("[role='grid'] [data-date]")).toHaveLength(366);
  });

  it("框外不压暗（只描边；压暗会跌破 ordinal 的 2:1 底线）", () => {
    const { container } = render(
      <ContributionWall
        days={DAYS}
        selection={{ kind: "day", from: "2026-10-02", to: "2026-10-02", label: "" }}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    // 最低档实测 2.29:1（深）/ 2.07:1（浅），压到 α=0.85 就掉到 1.99:1、
    // α=0.4 只剩 1.32:1 —— 任何全局压暗都破 ordinal 的 2:1 底线（MASTER §2.5）。
    expect(container.innerHTML).not.toContain("opacity");
    for (const el of container.querySelectorAll("[data-date]")) {
      expect((el as HTMLElement).style.opacity).toBe("");
    }
  });

// --- B8：日格要有可访问名 ---

describe("ContributionWall 日格的可访问名", () => {
  function wallCells(): HTMLElement[] {
    return Array.from(
      document.querySelectorAll<HTMLElement>('[data-testid^="cell-"]'),
    );
  }

  it("每一天都有 aria-label，不是只有 title", () => {
    // B8。`title` 是鼠标悬停才出现的视觉提示，读屏与键盘用户拿不到，
    // 原生 title 在触摸/键盘下也根本不弹。
    setup();
    const cells = wallCells();
    expect(cells.length).toBeGreaterThan(0);
    for (const c of cells) {
      expect(c.getAttribute("aria-label"), `格子 ${c.getAttribute("data-date")} 没有 aria-label`)
        .toBeTruthy();
      expect(c.getAttribute("aria-label")).toMatch(/\d{4}-\d{2}-\d{2}/);
    }
  });

  it("有记录的格子在名字里带上时长", () => {
    setup();
    const withData = document.querySelector('[data-testid="cell-2026-10-01"]')!;
    expect(withData.getAttribute("aria-label")).toMatch(/\d/);
  });

  it("装之前的日子说清是「还没装」，不假装是「当天没活动」", () => {
    // 这两种"空"在界面上是分开画的（STATUS 第 20、21 条），
    // 可访问名也必须分开 —— 读屏用户看不到描边强弱。
    //
    // 要造出"未安装期"得让 `days` 里有早于 `trackedFrom` 的日子：
    // `buildWall` 的 `tracked = present && cursor >= trackedFrom`。
    // （缺省不传 `trackedFrom` 时它等于 `days[0]`，于是永远没有 uninstalled 格。）
    const withPreInstallDays: DayCell[] = [
      { date: "2026-09-29", totalMs: 0 },
      { date: "2026-09-30", totalMs: 0 },
      ...DAYS,
    ];
    render(
      <ContributionWall
        days={withPreInstallDays}
        trackedFrom="2026-10-01"
        selection={null}
        onSelectDay={NOOP}
        onSelectWeek={NOOP}
        onSelectMonth={NOOP}
      />,
    );
    const untracked = wallCells().find(
      (c) => c.getAttribute("data-tone") === "uninstalled",
    );
    expect(untracked).toBeDefined();
    expect(untracked!.getAttribute("aria-label")).toContain("还没装");
  });
});
