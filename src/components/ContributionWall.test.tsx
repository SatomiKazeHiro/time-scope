import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import ContributionWall from "./ContributionWall";
import type { DateRange } from "../lib/summary";
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
    expect(grid.style.gridTemplateRows).toContain("repeat(7");
  });

  it("列优先填充：自上而下填满 7 行才换列，否则周会被打横", () => {
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    const grid = container.querySelector("[role='grid']") as HTMLElement;
    expect(grid.style.gridAutoFlow).toBe("column");
  });

  it("补齐位不渲染成格子", () => {
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    expect(container.querySelectorAll("[data-date]")).toHaveLength(3);
  });

  it("点格子回调那一天", () => {
    const { onSelectDay } = setup();
    fireEvent.click(screen.getByTestId("cell-2026-10-02"));
    expect(onSelectDay).toHaveBeenCalledWith("2026-10-02");
  });

  it("点周条回调那一周的周日", () => {
    const { onSelectWeek } = setup();
    fireEvent.click(screen.getAllByTestId("week-strip-btn")[0]);
    // 2026-10-01 是周四，首列的周日是 2026-09-27
    expect(onSelectWeek).toHaveBeenCalledWith("2026-09-27");
  });

  it("点月标签回调那个月的首日", () => {
    const { onSelectMonth } = setup();
    fireEvent.click(screen.getByTestId("month-label-10"));
    expect(onSelectMonth).toHaveBeenCalledWith("2026-10-01");
  });

  it("选中单日时框是 1 格", () => {
    const { container } = render(
      <ContributionWall
        days={DAYS}
        selection={{ kind: "day", from: "2026-10-02", to: "2026-10-02", label: "" }}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP}
      />,
    );
    const f = container.querySelector("[data-frame]") as HTMLElement;
    expect(f.style.width).toBe("11px");
    expect(f.style.height).toBe("11px");
  });

  it("选中一周时框是 1 列 × 7 格", () => {
    const { container } = render(
      <ContributionWall
        days={DAYS}
        selection={{ kind: "week", from: "2026-09-27", to: "2026-10-03", label: "" }}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP}
      />,
    );
    const f = container.querySelector("[data-frame]") as HTMLElement;
    expect(f.style.width).toBe("11px");
    // 7 行 = 7×PITCH - GAP = 7*14 - 3
    expect(f.style.height).toBe("95px");
  });

  it("没有选中时不画框", () => {
    const { container } = render(
      <ContributionWall days={DAYS} selection={null}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP} />,
    );
    expect(container.querySelector("[data-frame]")).toBeNull();
  });

  it("框外不压暗（只描框；压暗会跌破 MASTER 的 3:1 对比度红线）", () => {
    const { container } = render(
      <ContributionWall
        days={DAYS}
        selection={{ kind: "day", from: "2026-10-02", to: "2026-10-02", label: "" }}
        onSelectDay={NOOP} onSelectWeek={NOOP} onSelectMonth={NOOP}
      />,
    );
    expect(container.innerHTML).not.toContain("opacity:");
    // 每个格子都该是满色，没有任何格子被调暗
    for (const el of container.querySelectorAll("[data-date]")) {
      expect((el as HTMLElement).style.opacity).toBe("");
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
});
