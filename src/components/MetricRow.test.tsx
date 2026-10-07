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

const card = (name: string) =>
  screen.getByText(name).closest("[data-metric-card]") as HTMLElement;

describe("MetricRow 的层级", () => {
  it("总时长是整页唯一的大数字，范围跟着它", () => {
    render(<MetricRow summary={S} loading={false} rangeLabel="近一年" />);
    const total = card("监控总时长");
    const hero = total.querySelector("b")!;
    // 唯一用 text-lg 的读数：其余是 text-md 以下
    expect(hero.className).toContain("text-lg");
    expect(hero.textContent).toBe("54 时");
    // 1094 小时这种数没有单位会读不懂，所以范围必须跟着
    expect(total.textContent).toContain("近一年");
  });

  it("活跃占比是主读数，两段时长退到下面", () => {
    // 167.4 / 194.4 = 86.1% -> 86%
    render(<MetricRow summary={S} loading={false} />);
    const active = card("活跃占比");
    expect(active.querySelector("b")!.textContent).toBe("86%");
    expect(active.textContent).toContain("活跃 46 时 30 分");
    expect(active.textContent).toContain("空闲 7 时 30 分");
  });

  it("段数与切换次数降到诊断行，不占大字", () => {
    // 「引擎切成了 458 段」是诊断量，问「时间去哪了」的人不需要它当主读数
    render(<MetricRow summary={S} loading={false} />);
    const row = card("平均每段");
    expect(row.className).toContain("lg:col-span-12");
    expect(row.textContent).toContain("458 段");
    expect(row.textContent).toContain("切换 12 次");
  });

  it("平均段长补上了 —— 数据早就在手上，除一下就行", () => {
    // 167_400_000 / 458 ≈ 365_502 ms ≈ 6 分
    render(<MetricRow summary={S} loading={false} />);
    expect(card("平均每段").querySelector("b")!.textContent).toBe("6 分");
  });

  it("峰值时段从 hourlyMs 里取最高的那个小时", () => {
    const h = new Array(24).fill(0);
    h[14] = 9_000_000;
    h[20] = 4_000_000;
    render(<MetricRow summary={{ ...S, hourlyMs: h }} loading={false} />);
    expect(card("24h 时段分布").textContent).toContain("最忙 14:00");
  });

  it("全天无活动时峰值写「无峰值」，不写 00:00", () => {
    render(<MetricRow summary={S} loading={false} />);
    expect(card("24h 时段分布").textContent).toContain("无峰值");
  });

  it("24h 图表撑满卡高 —— 原来 74px 的图浮在 150px 盒子顶上", () => {
    render(<MetricRow summary={S} loading={false} />);
    const hours = card("24h 时段分布");
    expect(hours.className).toContain("flex-col");
    expect(hours.querySelector("svg")!.getAttribute("class")).toContain("flex-1");
    // 不等比拉伸，否则铺满高度时两侧留白、柱子被压窄
    expect(hours.querySelector("svg")!.getAttribute("preserveAspectRatio")).toBe("none");
    // 刻度标签因此必须搬出 SVG —— 会被一起压扁
    expect(hours.querySelectorAll("svg text").length).toBe(0);
  });
});

describe("MetricRow 的栅格", () => {
  it("12 列，四张主体卡各占 3 列", () => {
    const { container } = render(<MetricRow summary={S} loading={false} />);
    const row = container.querySelector("[data-metric-row]") as HTMLElement;
    expect(row.className).toContain("lg:grid-cols-12");
    for (const name of ["监控总时长", "活跃占比", "类别构成", "24h 时段分布"]) {
      expect(card(name).className).toMatch(/lg:col-span-[234]/);
    }
  });

  it("窄窗口下折成单列，不被压扁", () => {
    const { container } = render(<MetricRow summary={S} loading={false} />);
    const row = container.querySelector("[data-metric-row]") as HTMLElement;
    expect(row.className).toContain("grid-cols-1");
    expect(row.className).toContain("sm:grid-cols-2");
  });
});

describe("MetricRow 的空态", () => {
  it("summary 为 null 时渲染占位符而不是崩", () => {
    const { container } = render(<MetricRow summary={null} loading />);
    expect(container.querySelectorAll("[data-metric-card]")).toHaveLength(5);
    expect(container.textContent).toContain("—");
  });

  it("零时长时活跃占比是 0% 而不是 NaN", () => {
    const zero: Summary = { ...S, totalMs: 0, activeMs: 0, idleMs: 0 };
    const { container } = render(<MetricRow summary={zero} loading={false} />);
    expect(container.innerHTML).not.toContain("NaN");
    expect(card("活跃占比").querySelector("b")!.textContent).toBe("0%");
  });

  it("零时长时不显示平均段长的 0 分（会读成「每段 0 分钟」）", () => {
    const zero: Summary = { ...S, totalMs: 0, activeMs: 0, idleMs: 0 };
    render(<MetricRow summary={zero} loading={false} />);
    expect(card("平均每段").querySelector("b")!.textContent).toBe("—");
  });
});

describe("布局稳定性", () => {
  // 之前没数据时整行 return 一行字，5 张卡全消失 -> 布局塌陷 ->
  // 下面内容上移；数据到了再展开 -> 看着「内容弹了一下」。
  it("没数据时仍然渲染全部 5 个卡片容器", () => {
    const { container } = render(<MetricRow summary={null} loading />);
    expect(container.querySelector("[data-metric-row]")).toBeTruthy();
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

  it("加载中用 aria-busy 标出，不再靠文案区分", () => {
    const { container, rerender } = render(<MetricRow summary={null} loading />);
    expect(container.querySelector("[data-metric-row]")!.getAttribute("aria-busy")).toBe("true");
    rerender(<MetricRow summary={S} loading={false} />);
    expect(container.querySelector("[data-metric-row]")!.getAttribute("aria-busy")).toBeNull();
  });
});