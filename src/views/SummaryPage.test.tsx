import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import SummaryPage from "./SummaryPage";
import type { DailyCalendar, Summary } from "../types";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

/**
 * 只把 `getTopTitles` 在**模块边界**上替换掉，其余两个仍走传输层 mock。
 *
 * 原因：`getTopTitles` 里的 `await import("@tauri-apps/api/core")` 在
 * 本文件里解析成 `undefined`（Vitest 对动态 import 的拦截依赖调用顺序，
 * 单独调用三个封装都能正常走 mock）。而本页的职责是「用对的范围调
 * getTopTitles 并渲染结果」，包装函数本身只是薄薄一行 invoke，
 * 由 tsc 和直接的模块测试守着。
 */
const getTopTitles = vi.fn();
vi.mock("../types", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  getTopTitles: (...a: unknown[]) => getTopTitles(...a),
}));

const CAL: DailyCalendar = {
  first: "2026-10-01",
  last: "2026-10-05",
  days: [
    { date: "2026-10-01", totalMs: 3_600_000 },
    { date: "2026-10-02", totalMs: 13_600_000 },
    { date: "2026-10-03", totalMs: 11_360_000 },
    { date: "2026-10-04", totalMs: 17_320_000 },
    { date: "2026-10-05", totalMs: 2_420_000 },
  ],
};

const SUM: Summary = {
  totalMs: 194_400_000,
  activeMs: 167_400_000,
  idleMs: 27_000_000,
  segmentCount: 458,
  switchCount: 12,
  hourlyMs: new Array(24).fill(3_600_000),
  donut: [
    { key: "work", ms: 5_700_000 },
    { key: "browsing", ms: 20_200_000 },
    { key: "idle", ms: 7_500_000 },
    { key: "unknown", ms: 20_600_000 },
  ],
  topApps: [{ name: "msedge.exe", ms: 87_200_000 }],
};

const TITLES = [
  { title: "无标题", hits: 7086, redacted: false },
  { title: "客户 42 - [redacted] - Code", hits: 120, redacted: true },
];

/**
 * jsdom 没有 IntersectionObserver，懒加载路径永远不触发。
 * 装一个可控的桩：`visible` 为真时立即回调 isIntersecting，
 * 用来模拟「用户滚到了底部面板」。
 */
let visible = false;
class StubObserver {
  constructor(private cb: (e: Array<{ isIntersecting: boolean }>) => void) {}
  observe() {
    if (visible) this.cb([{ isIntersecting: true }]);
  }
  unobserve() {}
  disconnect() {}
}
(globalThis as unknown as { IntersectionObserver: unknown }).IntersectionObserver =
  StubObserver;

function callsTo(cmd: string) {
  return invoke.mock.calls.filter((c) => c[0] === cmd).length;
}

function lastArgs(cmd: string) {
  return invoke.mock.calls.filter((c) => c[0] === cmd).at(-1)?.[1];
}

beforeEach(() => {
  visible = false;
  getTopTitles.mockReset();
  getTopTitles.mockResolvedValue(TITLES);
  invoke.mockReset();
  invoke.mockImplementation((cmd: string) => {
    if (cmd === "get_daily_calendar") return Promise.resolve(CAL);
    if (cmd === "get_summary") return Promise.resolve(SUM);
    if (cmd === "get_top_titles") return Promise.resolve(TITLES);
    return Promise.resolve(null);
  });
});

describe("SummaryPage", () => {
  it("首屏拉日历与指标", async () => {
    render(<SummaryPage />);
    await waitFor(() => expect(callsTo("get_summary")).toBeGreaterThan(0));
    expect(callsTo("get_daily_calendar")).toBe(1);
  });

  it("标题排名是懒加载的，首屏不发（它是最贵的那一项）", async () => {
    render(<SummaryPage />);
    await waitFor(() => expect(callsTo("get_summary")).toBeGreaterThan(0));
    expect(callsTo("get_top_titles")).toBe(0);
  });

  it("点格子改范围时热力图不重取", async () => {
    const { container } = render(<SummaryPage />);
    await waitFor(() => expect(callsTo("get_summary")).toBeGreaterThan(0));
    const before = callsTo("get_daily_calendar");

    fireEvent.click(screen.getByTestId("cell-2026-10-03"));
    await waitFor(() => expect(callsTo("get_summary")).toBe(2));

    expect(callsTo("get_daily_calendar")).toBe(before);
    expect(container.querySelector("[data-frame]")).toBeTruthy();
  });

  it("点格子后 get_summary 收到那一天", async () => {
    render(<SummaryPage />);
    await waitFor(() => expect(callsTo("get_summary")).toBeGreaterThan(0));
    fireEvent.click(screen.getByTestId("cell-2026-10-03"));
    await waitFor(() => {
      expect(lastArgs("get_summary")).toMatchObject({
        from: "2026-10-03",
        to: "2026-10-03",
      });
    });
  });

  it("默认范围是全部（首个数据日 → 末日）", async () => {
    render(<SummaryPage />);
    await waitFor(() => expect(callsTo("get_summary")).toBeGreaterThan(0));
    expect(lastArgs("get_summary")).toMatchObject({
      from: "2026-10-01",
      to: "2026-10-05",
    });
  });

  it("点已选中的同一格回到全部", async () => {
    render(<SummaryPage />);
    await waitFor(() => expect(callsTo("get_summary")).toBeGreaterThan(0));
    fireEvent.click(screen.getByTestId("cell-2026-10-03"));
    await waitFor(() => expect(callsTo("get_summary")).toBe(2));
    fireEvent.click(screen.getByTestId("cell-2026-10-03"));
    await waitFor(() => {
      expect(lastArgs("get_summary")).toMatchObject({
        from: "2026-10-01",
        to: "2026-10-05",
      });
    });
  });

  it("点周条选那一周", async () => {
    render(<SummaryPage />);
    await waitFor(() => expect(callsTo("get_summary")).toBeGreaterThan(0));
    fireEvent.click(screen.getAllByTestId("week-strip-btn")[0]);
    await waitFor(() => {
      // 首列的周日是 2026-09-27，该周是 09-27..10-03
      expect(lastArgs("get_summary")).toMatchObject({
        from: "2026-09-27",
        to: "2026-10-03",
      });
    });
  });

  it("点月标签选那一月", async () => {
    render(<SummaryPage />);
    await waitFor(() => expect(callsTo("get_summary")).toBeGreaterThan(0));
    fireEvent.click(screen.getByTestId("month-label-10"));
    await waitFor(() => {
      expect(lastArgs("get_summary")).toMatchObject({
        from: "2026-10-01",
        to: "2026-10-31",
      });
    });
  });

  it("范围 chip 跟随选中更新文案", async () => {
    render(<SummaryPage />);
    await waitFor(() => expect(callsTo("get_summary")).toBeGreaterThan(0));
    expect(screen.getByTestId("range-chip").textContent).toContain("全部");
    fireEvent.click(screen.getByTestId("cell-2026-10-03"));
    await waitFor(() =>
      expect(screen.getByTestId("range-chip").textContent).toContain("2026-10-03"),
    );
  });

  it("范围 chip 是死控件：点它不发任何请求", async () => {
    render(<SummaryPage />);
    await waitFor(() => expect(callsTo("get_summary")).toBeGreaterThan(0));
    const n = invoke.mock.calls.length;
    fireEvent.click(screen.getByTestId("range-chip"));
    expect(invoke.mock.calls.length).toBe(n);
  });

  it("应用 Top 用 get_summary 带来的数据，不需要额外请求", async () => {
    render(<SummaryPage />);
    await waitFor(() => expect(screen.getByText("msedge.exe")).toBeTruthy());
    expect(callsTo("get_top_titles")).toBe(0);
  });

  it("空日历显示空状态，不画墙也不崩（Review Focus #1）", async () => {
    invoke.mockImplementation(() => Promise.resolve(null));
    const { container } = render(<SummaryPage />);
    await waitFor(() =>
      expect(screen.getAllByText(/还没有采集数据/).length).toBeGreaterThan(0));
    expect(container.innerHTML).not.toContain("NaN");
    expect(container.querySelector("[role='grid']")).toBeNull();
  });

  it("空库时指标卡不是「加载中…」，而是「暂无数据」（Review Focus #1）", async () => {
    // 库里没有任何段：日历返回 null -> range 为 null -> getSummary 根本不发，
    // summary 一直是 null、error 一直是 false -> loading={!summary && !error}
    // 永远为 true。新装用户看到的是一张永远转不出来的卡。
    invoke.mockImplementation(() => Promise.resolve(null));
    render(<SummaryPage />);
    await waitFor(() =>
      expect(screen.getAllByText(/还没有采集数据/).length).toBeGreaterThan(0));
    expect(screen.getByText("暂无数据")).toBeTruthy();
    expect(screen.queryByText(/加载中/)).toBeNull();
  });

  it("空库时范围 chip 与标题面板也给出结论，不是永远等", async () => {
    invoke.mockImplementation(() => Promise.resolve(null));
    render(<SummaryPage />);
    await waitFor(() =>
      expect(screen.getAllByText(/还没有采集数据/).length).toBeGreaterThan(0));
    expect(screen.getByTestId("range-chip").textContent).toContain("—");
    expect(screen.queryByText("向下滚动加载")).toBeNull();
  });

  it("get_summary 失败时显示错误而不是白屏", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "get_daily_calendar") return Promise.resolve(CAL);
      if (cmd === "get_summary") return Promise.reject(new Error("no db"));
      return Promise.resolve([]);
    });
    render(<SummaryPage />);
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
  });

  it("面板滚进视口后拉标题排名并渲染", async () => {
    visible = true;
    render(<SummaryPage />);
    await waitFor(() => expect(getTopTitles).toHaveBeenCalled());
    expect(getTopTitles).toHaveBeenCalledWith("2026-10-01", "2026-10-05", 10);
    expect(await screen.findByText("无标题")).toBeTruthy();
  });

  it("脱敏标题带「已脱敏」标记", async () => {
    visible = true;
    render(<SummaryPage />);
    await waitFor(() => expect(screen.getByText("已脱敏")).toBeTruthy());
  });

  it("标题排名随选中范围重取", async () => {
    visible = true;
    render(<SummaryPage />);
    await waitFor(() => expect(getTopTitles).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByTestId("cell-2026-10-03"));
    await waitFor(() => expect(getTopTitles).toHaveBeenCalledTimes(2));
    expect(getTopTitles).toHaveBeenLastCalledWith("2026-10-03", "2026-10-03", 10);
  });

  it("热力图图例用真色阶色块，不是字符凑的", async () => {
    render(<SummaryPage />);
    await waitFor(() => expect(callsTo("get_summary")).toBeGreaterThan(0));
    const { container } = { container: document.body };
    const swatches = container.querySelectorAll("[data-legend-step]");
    expect(swatches).toHaveLength(5);
    const fills = [...swatches].map((s) => (s as HTMLElement).style.background);
    expect(fills).toEqual([1, 2, 3, 4, 5].map((n) => `var(--color-scale-${n})`));
    // 「少 … 多」两端要有字
    expect(screen.getByText("少")).toBeTruthy();
    expect(screen.getByText("多")).toBeTruthy();
  });

  it("标题失败时显示空状态而不是崩", async () => {
    visible = true;
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "get_daily_calendar") return Promise.resolve(CAL);
      if (cmd === "get_summary") return Promise.resolve(SUM);
      if (cmd === "get_top_titles") return Promise.reject(new Error("too broad"));
      return Promise.resolve(null);
    });
    const { container } = render(<SummaryPage />);
    await waitFor(() => expect(screen.getByText(/这个范围没有记录/)).toBeTruthy());
    expect(container.innerHTML).not.toContain("NaN");
  });
});
