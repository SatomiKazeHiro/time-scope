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

  it("summary 为 null 时显示加载态而不是崩（Review Focus #1）", () => {
    render(<MetricRow summary={null} loading />);
    expect(screen.getByText(/加载中/)).toBeTruthy();
  });

  it("summary 为 null 且不在加载时给另一种文案", () => {
    render(<MetricRow summary={null} loading={false} />);
    expect(screen.getByText(/暂无数据/)).toBeTruthy();
  });

  it("零时长时活跃占比是 0% 而不是 NaN", () => {
    const zero: Summary = { ...S, totalMs: 0, activeMs: 0, idleMs: 0 };
    const { container } = render(<MetricRow summary={zero} loading={false} />);
    expect(container.innerHTML).not.toContain("NaN");
    expect(screen.getByText("0% 活跃")).toBeTruthy();
  });
});
