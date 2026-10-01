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

describe("App integration (segments)", () => {
  it("asks the backend for today's segments and renders them", async () => {
    invoke.mockResolvedValue([seg("s1", 9, 10)]);
    const { container } = render(<App />);

    await waitFor(() => expect(container.querySelectorAll("rect").length).toBe(1));
    expect(invoke).toHaveBeenCalledWith("get_segments", {
      date: expect.stringMatching(/^\d{4}-\d{2}-\d{2}$/),
    });
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
    const { container } = render(<App />);
    await waitFor(() => expect(container.querySelectorAll("rect").length).toBe(1));

    fireEvent.click(container.querySelector("rect")!);

    // "work" 在详情标题和汇总行里都会出现，所以断言详情专属的文案
    await waitFor(() => expect(screen.getByText("3 条事件支撑")).toBeTruthy());
    expect(screen.getByText("Code.exe")).toBeTruthy();
    expect(screen.getByText("3 条事件支撑")).toBeTruthy();
    expect(screen.getByText("规则 rules:15")).toBeTruthy();
  });

  it("shows an unknown application as a placeholder, not a blank", async () => {
    invoke.mockResolvedValue([seg("s1", 9, 10, "idle", null)]);
    const { container } = render(<App />);
    await waitFor(() => expect(container.querySelectorAll("rect").length).toBe(1));
    fireEvent.click(container.querySelector("rect")!);
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
