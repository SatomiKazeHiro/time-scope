import { describe, it, expect } from "vitest";
import { render } from "@testing-library/react";
import DayPartChart from "./DayPartChart";

const HOUR = 3_600_000;

function bars(container: HTMLElement) {
  return [...container.querySelectorAll("[data-bar]")] as SVGRectElement[];
}

/** SVG 的 height 是**属性**不是内联样式，要读 attribute。 */
const hOf = (b: SVGRectElement) => Number(b.getAttribute("height"));

describe("DayPartChart", () => {
  it("画 24 根柱", () => {
    const { container } = render(<DayPartChart hourlyMs={new Array(24).fill(0)} />);
    expect(bars(container)).toHaveLength(24);
  });

  it("按传入值定柱高，最大值满格", () => {
    const h = new Array(24).fill(0);
    h[3] = 9 * HOUR;
    h[12] = 1 * HOUR;
    const { container } = render(<DayPartChart hourlyMs={h} />);
    const b = bars(container);
    expect(hOf(b[3])).toBeGreaterThan(hOf(b[12]));
    expect(hOf(b[3])).toBe(56);   // 满格
    expect(hOf(b[12])).toBeCloseTo(56 / 9, 1);
  });

  it("全 0 时不产生 NaN，且不画零高柱（Review Focus #1）", () => {
    const { container } = render(<DayPartChart hourlyMs={new Array(24).fill(0)} />);
    expect(container.innerHTML).not.toContain("NaN");
    for (const b of bars(container)) {
      expect(hOf(b)).toBe(0);
    }
  });

  it("极小但非零的值仍留一根发丝线（0 时长要读得出「有但很少」）", () => {
    const h = new Array(24).fill(0);
    h[0] = 1000;
    const { container } = render(<DayPartChart hourlyMs={h} />);
    expect(hOf(bars(container)[0])).toBeGreaterThan(0);
  });

  it("用中档紫而不是类别色", () => {
    const h = new Array(24).fill(HOUR);
    const { container } = render(<DayPartChart hourlyMs={h} />);
    expect(container.innerHTML).toContain("--color-scale-3");
    expect(container.innerHTML).not.toContain("--color-cat-");
  });

  it("数组长度不足 24 时补齐，不越界", () => {
    const { container } = render(<DayPartChart hourlyMs={[HOUR, HOUR]} />);
    expect(bars(container)).toHaveLength(24);
  });

  it("每根柱有 title，悬停能看到该小时的读数", () => {
    const h = new Array(24).fill(0);
    h[3] = 90 * 60_000;
    const { container } = render(<DayPartChart hourlyMs={h} />);
    expect(bars(container)[3].querySelector("title")?.textContent).toContain("03:00");
  });

  it("空数组不崩", () => {
    const { container } = render(<DayPartChart hourlyMs={[]} />);
    expect(bars(container)).toHaveLength(24);
  });
});
