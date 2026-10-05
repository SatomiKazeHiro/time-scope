import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { render } from "@testing-library/react";
import DayPartChart from "./DayPartChart";

const THEME_CSS = readFileSync("src/styles/theme.css", "utf8");
/** 每个 var() 引用都得在 theme.css 里被定义过。 */
function assertTokensDefined(uses: string[]) {
  for (const u of uses) {
    const name = /var\((--[a-z0-9-]+)\)/i.exec(u)?.[1];
    if (!name) continue;
    expect(THEME_CSS, `theme.css 未定义 ${name}`).toMatch(
      new RegExp(`\\s${name}\\s*:`),
    );
  }
}

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

  // 截图暴露的两个问题：条被 w-full 摊得太散，刻度被一起放大
  it("刻度线与刻度字的 var() 引用在 theme.css 里存在", () => {
    // stroke="var(--line)" 之前写成不存在的 token：刻度线直接消失，
    // 刻度字退回黑色，深色底上 00/06/12/18/24 全部看不见。
    const { container } = render(<DayPartChart hourlyMs={new Array(24).fill(0)} />);
    assertTokensDefined(container.innerHTML.match(/var\([^)]+\)/g) ?? []);
  });

  it("限制最大宽度，不跟着面板无限拉宽", () => {
    const { container } = render(<DayPartChart hourlyMs={new Array(24).fill(0)} />);
    // SVG 元素的 .className 是 SVGAnimatedString，要读 attribute。
    // w-full + max-w-[760px] 是「先铺满再封顶」，两个都要有。
    const cls = (container.querySelector("svg") as SVGSVGElement).getAttribute("class") ?? "";
    expect(cls).toContain("w-full");
    expect(cls).toContain("max-w-");
  });

  it("柱宽按格宽的一多半算，太细会像一排孤立的针", () => {
    const { container } = render(<DayPartChart hourlyMs={new Array(24).fill(HOUR)} />);
    const viewBoxW = 720;
    const barW = Number(bars(container)[0].getAttribute("width"));
    expect(barW).toBeGreaterThan(viewBoxW / 24 / 2);
    expect(barW).toBeLessThan(viewBoxW / 24);
  });

  it("末尾刻度不居中，否则「24」有一半被切在 viewBox 外", () => {
    const { container } = render(<DayPartChart hourlyMs={new Array(24).fill(0)} />);
    const labels = [...container.querySelectorAll("text")];
    const last = labels.find((t) => t.textContent === "24")!;
    expect(last).toBeTruthy();
    expect(last.getAttribute("text-anchor")).toBe("end");
    const first = labels.find((t) => t.textContent === "00")!;
    expect(first.getAttribute("text-anchor")).toBe("start");
  });
});
