import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import SegmentDetail from "./EventDetail";
import type { Segment } from "../types";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

function segment(over: Partial<Segment> = {}): Segment {
  return {
    id: "s1",
    // 用相对时间：这个组件不做日期过滤，但硬编码日期会在别的日子显得很奇怪
    startAt: Date.now() - 3_600_000,
    endAt: Date.now(),
    category: "work",
    application: "Code.exe",
    confidence: 0.9,
    classifier: "rule",
    classifierVersion: "rules:15",
    evidenceEventIds: ["e1", "e2"],
    ...over,
  };
}

beforeEach(() => {
  invoke.mockReset();
});

describe("SegmentDetail 的窗口标题", () => {
  it("点开段时按 evidence id 拉取标题", async () => {
    invoke.mockResolvedValue([{ title: "main.rs - Code", redacted: false, count: 3 }]);
    render(<SegmentDetail segment={segment()} />);
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("get_segment_titles", { eventIds: ["e1", "e2"] }),
    );
    await waitFor(() => expect(screen.getByText("main.rs - Code")).toBeTruthy());
  });

  it("给脱敏标题打上可识别的标记", async () => {
    invoke.mockResolvedValue([
      { title: "registry.ts - [redacted] - Code", redacted: true, count: 5 },
    ]);
    render(<SegmentDetail segment={segment()} />);
    await waitFor(() => expect(screen.getByText("已脱敏")).toBeTruthy());
  });

  it("普通标题不带脱敏标记", async () => {
    invoke.mockResolvedValue([{ title: "main.rs - Code", redacted: false, count: 1 }]);
    render(<SegmentDetail segment={segment()} />);
    await waitFor(() => expect(screen.getByText("main.rs - Code")).toBeTruthy());
    expect(screen.queryByText("已脱敏")).toBeNull();
  });

  it("给出脱敏规则的配置位置", async () => {
    invoke.mockResolvedValue([{ title: "a - [redacted]", redacted: true, count: 1 }]);
    render(<SegmentDetail segment={segment()} />);
    await waitFor(() => expect(screen.getByText(/rules\.toml/)).toBeTruthy());
  });

  it("多个标题时逐条列出并显示出现次数", async () => {
    invoke.mockResolvedValue([
      { title: "first - Code", redacted: false, count: 2 },
      { title: "second - Code", redacted: true, count: 1 },
    ]);
    render(<SegmentDetail segment={segment()} />);
    await waitFor(() => expect(screen.getByText("first - Code")).toBeTruthy());
    expect(screen.getByText("second - Code")).toBeTruthy();
    expect(screen.getByText("× 2")).toBeTruthy();
  });

  it("没有标题时说明原因而不是留空白", async () => {
    // 段还没落库（正在生长）时没有 evidence，自然也没有标题
    invoke.mockResolvedValue([]);
    render(<SegmentDetail segment={segment({ evidenceEventIds: [] })} />);
    await waitFor(() => expect(screen.getByText(/该段尚未落库，还没有标题/)).toBeTruthy());
  });

  it("有 evidence 但全部没有标题时给出另一种说明", async () => {
    // 例如这段时间只有心跳和空闲事件，没有任何窗口事件
    invoke.mockResolvedValue([]);
    render(<SegmentDetail segment={segment()} />);
    await waitFor(() => expect(screen.getByText(/没有带标题的窗口事件/)).toBeTruthy());
  });

  it("拉取标题失败不让详情面板崩掉", async () => {
    invoke.mockRejectedValue(new Error("boom"));
    render(<SegmentDetail segment={segment()} />);
    // 其余字段仍应正常显示
    await waitFor(() => expect(screen.getByText("Code.exe")).toBeTruthy());
  });

  it("切换选中的段会重新拉取标题", async () => {
    invoke.mockResolvedValue([{ title: "t1", redacted: false, count: 1 }]);
    const { rerender } = render(<SegmentDetail segment={segment()} />);
    await waitFor(() => expect(screen.getByText("t1")).toBeTruthy());

    invoke.mockResolvedValue([{ title: "t2", redacted: true, count: 1 }]);
    rerender(<SegmentDetail segment={segment({ id: "s2", evidenceEventIds: ["e9"] })} />);
    await waitFor(() => expect(screen.getByText("t2")).toBeTruthy());
    expect(screen.queryByText("t1")).toBeNull();
  });

  it("未选中任何段时不请求标题", () => {
    render(<SegmentDetail segment={null} />);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("正在生长的段证据变多时重新拉取标题", async () => {
    // B6。正在生长的那一段 `id` 一直是 `open-{start_at}` 不变，而
    // `evidenceEventIds` 每 5 秒轮询就变长。effect 的依赖原来只有 `[key]`
    // （= segment.id），于是面板自相矛盾：「N 条事件支撑」在涨，
    // 标题列表永远停在第一次的结果。
    invoke.mockResolvedValue([{ title: "t1", redacted: false, count: 1 }]);
    const { rerender } = render(<SegmentDetail segment={segment({ id: "open-1" })} />);
    await waitFor(() => expect(screen.getByText("t1")).toBeTruthy());
    const callsBefore = invoke.mock.calls.length;

    // 同一个 id，只是证据多了两条（又来两次窗口切换）
    rerender(
      <SegmentDetail
        segment={segment({ id: "open-1", evidenceEventIds: ["e1", "e2", "e3", "e4"] })}
      />,
    );
    await waitFor(() =>
      expect(invoke.mock.calls.length).toBeGreaterThan(callsBefore),
    );
    expect(invoke).toHaveBeenLastCalledWith("get_segment_titles", {
      eventIds: ["e1", "e2", "e3", "e4"],
    });
  });

  it("证据没变时不重复请求标题", async () => {
    // 与上一条互补：依赖变了以后不能变成"每次轮询都重取"，
    // 那会把 events 表的查询放大 5 秒一次。
    invoke.mockResolvedValue([{ title: "t1", redacted: false, count: 1 }]);
    const { rerender } = render(<SegmentDetail segment={segment()} />);
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
    rerender(<SegmentDetail segment={segment()} />);
    rerender(<SegmentDetail segment={segment()} />);
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
  });
});
