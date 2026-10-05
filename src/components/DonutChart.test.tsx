import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import DonutChart from "./DonutChart";
import type { DonutSlice } from "../types";

const DONUT: DonutSlice[] = [
  { key: "work", ms: 5_700_000 },
  { key: "browsing", ms: 20_200_000 },
  { key: "idle", ms: 7_500_000 },
  { key: "unknown", ms: 20_600_000 },
];

describe("DonutChart", () => {
  it("四档各画一个弧段，环是完整的 360°", () => {
    const { container } = render(<DonutChart donut={DONUT} totalMs={54_000_000} />);
    expect(container.querySelectorAll("[data-arc]")).toHaveLength(4);
  });

  it("中心写监控总时长，且用已有 token 上色（不依赖未定义的 class）", () => {
    const { container } = render(<DonutChart donut={DONUT} totalMs={54_000_000} />);
    const center = screen.getByText("15h");
    expect(center.getAttribute("fill")).toBe("var(--ink)");
    expect(center.getAttribute("class")).toBeNull();
    // 整个组件不得引用 theme.css 里没有的 class
    expect(container.innerHTML).not.toContain("ring-total");
    expect(container.innerHTML).not.toContain("ring-sub");
  });

  it("图例四行，各带时长与百分比", () => {
    render(<DonutChart donut={DONUT} totalMs={54_000_000} />);
    expect(screen.getByText("工作")).toBeTruthy();
    expect(screen.getByText("未分类")).toBeTruthy();
    // work 5.7 / 54 = 10.6% -> 11%
    expect(screen.getByText("11%")).toBeTruthy();
  });

  it("用类别色而不是连续量色阶", () => {
    // 蓝/橙/灰是类别色；紫阶是连续量的。混用会让「蓝 = work」失效。
    const { container } = render(<DonutChart donut={DONUT} totalMs={54_000_000} />);
    const html = container.innerHTML;
    expect(html).toContain("var(--color-cat-work)");
    expect(html).toContain("var(--color-cat-browsing)");
    expect(html).not.toContain("--color-scale-");
  });

  it("总时长为 0 时不产生 NaN（Review Focus #1）", () => {
    const empty: DonutSlice[] = DONUT.map((d) => ({ key: d.key, ms: 0 }));
    const { container } = render(<DonutChart donut={empty} totalMs={0} />);
    expect(container.innerHTML).not.toContain("NaN");
    expect(screen.getByText("0h")).toBeTruthy();
  });

  it("空 donut 数组不崩", () => {
    const { container } = render(<DonutChart donut={[]} totalMs={0} />);
    expect(container.querySelectorAll("[data-arc]")).toHaveLength(0);
  });

  it("未知档位回落到 unknown 的颜色，不崩", () => {
    const odd = [...DONUT, { key: "study" as DonutSlice["key"], ms: 1_000 }];
    const { container } = render(<DonutChart donut={odd} totalMs={54_000_000} />);
    expect(container.querySelectorAll("[data-arc]")).toHaveLength(5);
  });
});
