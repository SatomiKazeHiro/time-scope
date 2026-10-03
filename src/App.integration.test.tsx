import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, fireEvent, act } from "@testing-library/react";
import App from "./App";
import type { Segment } from "./types";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

// 时间必须基于"今天"：DateSummary 会把段截到 [今天 00:00, 次日 00:00)，
// 硬编码日期的段在别的日子会算出空汇总。
function dayAt(hour: number): number {
  const d = new Date();
  d.setHours(hour, 0, 0, 0);
  return d.getTime();
}

function seg(
  id: string,
  startH: number,
  endH: number,
  category: Segment["category"] = "work",
  application: string | null = "Code.exe",
): Segment {
  return {
    id,
    startAt: dayAt(startH),
    endAt: dayAt(endH),
    category,
    application,
    confidence: 0.9,
    classifier: "rule",
    classifierVersion: "rules:15",
    evidenceEventIds: ["e1", "e2", "e3"],
  };
}

beforeEach(() => {
  invoke.mockReset();
  vi.useFakeTimers({ shouldAdvanceTime: true });
});

afterEach(() => {
  vi.useRealTimers();
});

/**
 * 只取时间线里的 rect。
 *
 * 两个坑叠在一起，所以必须按无障碍名定位到时间线本身：
 *  1. 页面里现在不止时间线一个 svg —— lucide 的 CalendarDays 自带一个 <rect>，
 *     `container.querySelectorAll("rect")` 会把它数进去。
 *  2. 时间线默认 30 分钟粒度，一个跨边界的段会被切成多块 ——
 *     **rect 数量是"块"数，不再是"段"数**。
 *
 * 「一个段 = 一个 rect」的精确对应关系在 SegmentTimeline 的单测里锁
 * （那里传 intervalMs=0，不切片）。
 */
function timelineRects(): SVGRectElement[] {
  const svg = screen.getByRole("img", { name: "24h 活动时间线" });
  return Array.from(svg.querySelectorAll("rect")) as SVGRectElement[];
}

describe("App integration (segments)", () => {
  it("asks the backend for today's segments and renders them", async () => {
    invoke.mockResolvedValue([seg("s1", 9, 10)]);
    render(<App />);

    // 9:00–10:00 整一小时，默认 30 分粒度切成两块
    await waitFor(() => expect(timelineRects().length).toBe(2));
    expect(invoke).toHaveBeenCalledWith("get_segments", {
      date: expect.stringMatching(/^\d{4}-\d{2}-\d{2}$/),
    });
  });

  it("reports how many pieces the current granularity cuts the day into", async () => {
    invoke.mockResolvedValue([seg("s1", 9, 10)]);
    render(<App />);
    await waitFor(() => expect(timelineRects().length).toBe(2));
    // 粒度控制必须真的对视图有反应，头部读数就是证据
    expect(screen.getByText("1 段 → 2 块（30 分）")).toBeTruthy();
  });

  it("a finer granularity cuts the same day into more pieces", async () => {
    invoke.mockResolvedValue([seg("s1", 9, 10)]);
    render(<App />);
    await waitFor(() => expect(timelineRects().length).toBe(2));

    fireEvent.click(screen.getByText("10分"));
    // 1 小时按 10 分钟切 = 6 块
    await waitFor(() => expect(timelineRects().length).toBe(6));
    expect(screen.getByText("1 段 → 6 块（10 分）")).toBeTruthy();
  });

  it("shows an error state instead of throwing when the backend rejects", async () => {
    invoke.mockRejectedValue(new Error("no db"));
    render(<App />);
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
  });

  it("shows an empty-day hint rather than a blank bar", async () => {
    invoke.mockResolvedValue([]);
    render(<App />);
    await waitFor(() => expect(screen.getByText(/还没有活动段/)).toBeTruthy());
  });

  it("re-queries with the newly picked date", async () => {
    invoke.mockResolvedValue([]);
    render(<App />);
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));

    // 挑一个与"今天"不同的日期，否则日期输入框的值不变，change 不会触发重新查询
    const other = "2019-01-02";
    fireEvent.change(screen.getByDisplayValue(/^\d{4}-\d{2}-\d{2}$/), {
      target: { value: other },
    });

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("get_segments", { date: other }),
    );
  });

  it("renders segment details when a block is clicked", async () => {
    invoke.mockResolvedValue([seg("s1", 9, 10)]);
    render(<App />);
    await waitFor(() => expect(timelineRects().length).toBe(2));

    fireEvent.click(timelineRects()[0]);

    // "work" 在详情标题和汇总行里都会出现，所以断言详情专属的文案
    await waitFor(() => expect(screen.getByText("3 条事件支撑")).toBeTruthy());
    expect(screen.getByText("Code.exe")).toBeTruthy();
    expect(screen.getByText("3 条事件支撑")).toBeTruthy();
    expect(screen.getByText("规则 rules:15")).toBeTruthy();
  });

  it("shows an unknown application as a placeholder, not a blank", async () => {
    invoke.mockResolvedValue([seg("s1", 9, 10, "idle", null)]);
    render(<App />);
    await waitFor(() => expect(timelineRects().length).toBe(2));
    fireEvent.click(timelineRects()[0]);
    await waitFor(() => expect(screen.getByText("（未知）")).toBeTruthy());
  });

  it("changing granularity does not re-query the backend", async () => {
    // spec §8.2：分桶在前端做，切换粒度不重查
    invoke.mockResolvedValue([seg("s1", 0, 5)]);
    render(<App />);
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByText("120分"));
    await waitFor(() => expect(screen.getByText(/120 分/)).toBeTruthy());
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("renders the day summary with per-category totals", async () => {
    invoke.mockResolvedValue([
      seg("s1", 0, 2, "work"),
      seg("s2", 2, 3, "browsing"),
    ]);
    render(<App />);
    await waitFor(() => expect(screen.getByLabelText("当日汇总")).toBeTruthy());
    expect(screen.getByText("2 时")).toBeTruthy();
    expect(screen.getByText("1 时")).toBeTruthy();
  });
});

describe("App auto-refresh", () => {
  it("polls the backend so a running app does not look frozen", async () => {
    // 后台引擎一直在产出新段，不轮询的话界面就是一张静止的图，看着像程序坏了。
    invoke.mockResolvedValue([seg("s1", 9, 10)]);
    render(<App />);
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));

    await act(async () => {
      await vi.advanceTimersByTimeAsync(6_000);
    });
    expect(invoke.mock.calls.length).toBeGreaterThan(1);
  });

  it("stops polling after unmount", async () => {
    invoke.mockResolvedValue([]);
    const { unmount } = render(<App />);
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    const before = invoke.mock.calls.length;

    unmount();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(12_000);
    });
    expect(invoke.mock.calls.length).toBe(before);
  });
});
