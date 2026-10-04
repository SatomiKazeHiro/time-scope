import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, fireEvent, act } from "@testing-library/react";
import App from "./App";
import type { Segment } from "./types";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

// 时间必须基于"今天"：DateSummary 会把段截到 [今天 00:00, 次日 00:00)，
// 硬编码日期的段在别的日子会算出空汇总。
function dayAt(hour: number, minute = 0): number {
  const d = new Date();
  d.setHours(hour, minute, 0, 0);
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
 * 页面里现在不止时间线一个 svg —— lucide 的 CalendarDays 自带一个 <rect>，
 * `container.querySelectorAll("rect")` 会把它数进去，所以按无障碍名定位。
 *
 * 类别模式下**一个段 = 一个 rect**，与粒度无关（粒度在那儿只管刻度尺）。
 * 指标模式下 rect 是桶，计数语义不同，别拿这里的 helper 去断指标模式。
 */
function timelineRects(): SVGRectElement[] {
  const svg = screen.getByRole("img", { name: "24h 活动时间线" });
  return Array.from(svg.querySelectorAll("rect")) as SVGRectElement[];
}

describe("App integration (segments)", () => {
  it("asks the backend for today's segments and renders them", async () => {
    invoke.mockResolvedValue([seg("s1", 9, 10)]);
    render(<App />);

    await waitFor(() => expect(timelineRects().length).toBe(1));
    expect(invoke).toHaveBeenCalledWith("get_segments", {
      date: expect.stringMatching(/^\d{4}-\d{2}-\d{2}$/),
    });
  });

  it("类别模式不受粒度影响 —— 切粒度不改变段的数量", async () => {
    // 段是引擎判定的活动边界。按时间格切一刀只会把连续活动切碎，
    // 粒度在类别模式下只该动刻度尺。
    invoke.mockResolvedValue([seg("s1", 9, 10)]);
    render(<App />);
    await waitFor(() => expect(timelineRects().length).toBe(1));
    expect(screen.getByText("1 段")).toBeTruthy();

    for (const label of ["10分", "60分", "120分"]) {
      fireEvent.click(screen.getByText(label));
      await waitFor(() => expect(timelineRects().length).toBe(1));
    }
    expect(screen.getByText("1 段")).toBeTruthy();
  });

  it("指标模式读数说的是指标，不是段数", async () => {
    // 头部那行是模式的唯一文字出口，三种模式说三件不同的事。
    // 两个类别必须落在**同一小时**里：09:00–09:30 work + 09:30–10:00 browsing。
    // 否则 60 分桶里每桶只有一个类别，专注度恒为 100%，测不出东西。
    const half = (id: string, fromH: number, fromM: number, toM: number, category: Segment["category"], app: string): Segment => ({
      id,
      startAt: dayAt(fromH, fromM),
      endAt: dayAt(toM < fromM ? fromH + 1 : fromH, toM),
      category,
      application: app,
      confidence: 0.9,
      classifier: "rule",
      classifierVersion: "rules:15",
      evidenceEventIds: ["e1", "e2", "e3"],
    });
    invoke.mockResolvedValue([
      half("s1", 9, 0, 30, "work", "Code.exe"),
      half("s2", 9, 30, 0, "browsing", "chrome.exe"),
    ]);
    render(<App />);
    await waitFor(() => expect(timelineRects().length).toBeGreaterThan(0));
    // 这两段各占 30 分钟，在 30 分粒度下没被切开，所以头部走短格式
    expect(screen.getByText("2 段")).toBeTruthy();

    fireEvent.click(screen.getByText("60分"));
    fireEvent.click(screen.getByRole("button", { name: "专注度" }));
    await waitFor(() => expect(screen.getByText(/平均专注度/)).toBeTruthy());
    expect(screen.getByText(/平均专注度 50%/)).toBeTruthy();
    expect(screen.queryByText(/段 →/)).toBeNull();

    // 同一桶里 work→browsing 是一次应用切换
    fireEvent.click(screen.getByRole("button", { name: "切换次数" }));
    await waitFor(() => expect(screen.getByText(/全天切换/)).toBeTruthy());
    expect(screen.getByText(/全天切换 1 次/)).toBeTruthy();
  });

  it("指标模式下头部读数跟着粒度走", async () => {
    invoke.mockResolvedValue([seg("s1", 9, 10, "work")]);
    render(<App />);
    await waitFor(() => expect(timelineRects().length).toBeGreaterThan(0));

    fireEvent.click(screen.getByRole("button", { name: "专注度" }));
    await waitFor(() => expect(screen.getByText(/30 分一格/)).toBeTruthy());
    fireEvent.click(screen.getByText("120分"));
    await waitFor(() => expect(screen.getByText(/120 分一格/)).toBeTruthy());
  });

  it("没有活动的日子不显示指标读数，而不是显示 0%", async () => {
    // 分母是 0，算出来是 NaN —— 显示 NaN% 比不显示更糟
    invoke.mockResolvedValue([seg("s1", 9, 10, "work")]);
    const { unmount } = render(<App />);
    await waitFor(() => expect(timelineRects().length).toBeGreaterThan(0));
    fireEvent.click(screen.getByRole("button", { name: "专注度" }));
    await waitFor(() => expect(screen.getByText(/平均专注度/)).toBeTruthy());
    unmount();

    invoke.mockResolvedValue([]);
    render(<App />);
    await waitFor(() => expect(screen.getByText(/还没有活动段/)).toBeTruthy());
    expect(screen.queryByText(/平均专注度/)).toBeNull();
    expect(screen.queryByText(/全天切换/)).toBeNull();
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
    await waitFor(() => expect(timelineRects().length).toBe(1));

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
    await waitFor(() => expect(timelineRects().length).toBe(1));
    fireEvent.click(timelineRects()[0]);
    await waitFor(() => expect(screen.getByText("（未知）")).toBeTruthy());
  });

  it("changing granularity does not re-query the backend", async () => {
    // spec §8.2：粒度只是前端参数，改它不重新请求
    invoke.mockResolvedValue([seg("s1", 0, 5)]);
    render(<App />);
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByText("120分"));
    // 类别模式下粒度只动刻度尺，段数不变
    await waitFor(() => expect(timelineRects().length).toBe(1));
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
