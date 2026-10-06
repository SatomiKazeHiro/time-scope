import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import MetricRow from "./MetricRow";
import type { Summary } from "../types";

const S: Summary = {
  totalMs: 194_400_000,      // 54h
  activeMs: 167_400_000,     // 46.5h
  idleMs: 27_000_000,        // 7.5h
  segmentCount: 458,
  switchCount: 12,
  hourlyMs: new Array(24).fill(0),
  donut: [
    { key: "work", ms: 5_700_000 },
    { key: "browsing", ms: 20_200_000 },
    { key: "idle", ms: 7_500_000 },
    { key: "unknown", ms: 20_600_000 },
  ],
  topApps: [],
};

describe("MetricRow", () => {
  it("五张卡：总时长 / 活跃·空闲 / 圆环 / 段数·切换 / 24h 分布", () => {
    // 24h 时段分布是**指标卡**，不是热力图下面的全宽条
    const { container } = render(<MetricRow summary={S} loading={false} />);
    expect(screen.getByText("24h 时段分布")).toBeTruthy();
    expect(container.querySelector("[data-bar]")).toBeTruthy();
    const row = container.querySelector("[data-metric-row]") as HTMLElement;
    expect(row.className).toContain("grid-cols-6");
  });

  it("圆环卡跨两列，否则图例被压成竖排单字", () => {
    // 5 张卡在 1200px 下每张只有 215px，环 64px + 四行图例放不下
    render(<MetricRow summary={S} loading={false} />);
    const donut = screen.getByText("类别构成").closest("div") as HTMLElement;
    expect(donut.className).toContain("col-span-2");
  });

  it("24h 卡在窄窗口下折行，不被压扁", () => {
    const { container } = render(<MetricRow summary={S} loading={false} />);
    const row = container.querySelector("[data-metric-row]") as HTMLElement;
    expect(row.className).toContain("lg:grid-cols-6");
    expect(row.className).toContain("grid-cols-1");
  });

  it("四张卡：总时长 / 活跃·空闲 / 圆环 / 段数·切换", () => {
    const { container } = render(<MetricRow summary={S} loading={false} />);
    expect(screen.getByText("监控总时长")).toBeTruthy();
    expect(screen.getByText("活跃 / 空闲")).toBeTruthy();
    expect(screen.getByText("活动段")).toBeTruthy();
    expect(container.querySelector("[data-arc]")).toBeTruthy();
  });

  it("段数与切换次数并排", () => {
    render(<MetricRow summary={S} loading={false} />);
    expect(screen.getByText("458 段")).toBeTruthy();
    expect(screen.getByText("切换 12 次")).toBeTruthy();
  });

  it("活跃占比按非空闲算", () => {
    // 167.4 / 194.4 = 86.1% -> 86%
    render(<MetricRow summary={S} loading={false} />);
    expect(screen.getByText("86% 活跃")).toBeTruthy();
  });

  it("summary 为 null 时渲染占位符而不是崩（Review Focus #1）", () => {
    const { container } = render(<MetricRow summary={null} loading />);
    expect(container.querySelectorAll("[data-metric-card]")).toHaveLength(5);
    expect(container.textContent).toContain("—");
  });

  it("加载中用 aria-busy 标出，不再靠文案区分", () => {
    const { container, rerender } = render(<MetricRow summary={null} loading />);
    expect(container.querySelector("[data-metric-row]")!.getAttribute("aria-busy"))
      .toBe("true");
    rerender(<MetricRow summary={S} loading={false} />);
    expect(container.querySelector("[data-metric-row]")!.getAttribute("aria-busy"))
      .toBeNull();
  });

  it("零时长时活跃占比是 0% 而不是 NaN", () => {
    const zero: Summary = { ...S, totalMs: 0, activeMs: 0, idleMs: 0 };
    const { container } = render(<MetricRow summary={zero} loading={false} />);
    expect(container.innerHTML).not.toContain("NaN");
    expect(screen.getByText("0% 活跃")).toBeTruthy();
  });
})

describe("布局稳定性", () => {
  // 之前没数据时整行 return 一行字，5 张卡全消失 -> 布局塌陷 ->
  // 下面内容上移；数据到了再展开 -> 看着「内容弹了一下」。
  it("没数据时仍然渲染全部 5 张卡的外壳", () => {
    const { container } = render(<MetricRow summary={null} loading />);
    expect(container.querySelector("[data-metric-row]")).toBeTruthy();
    // 5 个卡片容器：3 个 Card + 圆环卡 + 24h 卡
    expect(container.querySelectorAll("[data-metric-card]")).toHaveLength(5);
  });

  it("有数据时卡片数与没数据时一样（结构不因数据有无而变）", () => {
    const withData = render(<MetricRow summary={S} loading={false} />);
    const n1 = withData.container.querySelectorAll("[data-metric-card]").length;
    withData.unmount();
    const noData = render(<MetricRow summary={null} loading />);
    const n2 = noData.container.querySelectorAll("[data-metric-card]").length;
    noData.unmount();
    expect(n2).toBe(n1);
  });

  it("没数据时圆环与 24h 条仍然占位（高度不被压塌）", () => {
    const { container } = render(<MetricRow summary={null} loading />);
    // 圆环 SVG 固定 64x64、图例恒 4 行；24h 条 viewBox 高度固定。
    // 传空数组也照常渲染 -> 几何与有数据时一致。
    expect(container.querySelectorAll("svg")).toHaveLength(2);
    expect(container.querySelectorAll("[data-legend]")).toHaveLength(4);
  });

  it("没数据时显示占位符而不是假的 0", () => {
    const { container } = render(<MetricRow summary={null} loading />);
    expect(container.innerHTML).toContain("—");
    expect(container.textContent).not.toContain("0 段");
  });

  it("grid 列数不随数据有无变化", () => {
    const a = render(<MetricRow summary={null} loading />);
    const cls1 = a.container.querySelector("[data-metric-row]")!.className;
    a.unmount();
    const b = render(<MetricRow summary={S} loading={false} />);
    const cls2 = b.container.querySelector("[data-metric-row]")!.className;
    b.unmount();
    expect(cls1).toBe(cls2);
  });
});
