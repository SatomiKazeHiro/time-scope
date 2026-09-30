import { describe, it, expect } from "vitest";
import { render } from "@testing-library/react";
import Timeline from "./Timeline";
import type { StoredEvent } from "../types";

const DAY_MS = 86_400_000;

const base: StoredEvent[] = [
  {
    id: "1",
    timestamp: 0,
    type: "window_focus",
    payload: JSON.stringify({ process_name: "Code.exe", window_title: "a", exe_path: null }),
  },
  {
    id: "2",
    timestamp: DAY_MS - 1,
    type: "system_idle",
    payload: JSON.stringify({ type: "system_idle" }),
  },
];

function rects(container: HTMLElement): SVGRectElement[] {
  return Array.from(container.querySelectorAll("rect")) as SVGRectElement[];
}

describe("Timeline", () => {
  it("renders one rect per event", () => {
    const { container } = render(
      <Timeline events={base} dayStartMs={0} onSelect={() => {}} />,
    );
    expect(rects(container).length).toBe(2);
  });

  it("does not crash on null window_title payload", () => {
    const ev: StoredEvent = {
      id: "3",
      timestamp: 1000,
      type: "window_focus",
      payload: JSON.stringify({ process_name: "x.exe", window_title: null, exe_path: null }),
    };
    const { container } = render(
      <Timeline events={[ev]} dayStartMs={0} onSelect={() => {}} />,
    );
    expect(rects(container).length).toBe(1);
  });

  it("positions a midnight event at the left edge and end-of-day near the right", () => {
    const { container } = render(
      <Timeline events={base} dayStartMs={0} onSelect={() => {}} />,
    );
    const r = rects(container);
    expect(Number(r[0].getAttribute("x"))).toBeCloseTo(0, 5);
    // 最后一个事件距天末 1ms，应贴近右边缘
    expect(Number(r[1].getAttribute("x"))).toBeGreaterThan(990);
  });

  it("clamps events outside the requested day to the edges", () => {
    // 事件可能因为时区/边界落在 [0, DAY_MS) 之外，UI 不能画出负 x 或超宽
    const outside: StoredEvent[] = [
      { id: "a", timestamp: -5_000, type: "window_focus", payload: "{}" },
      { id: "b", timestamp: DAY_MS + 5_000, type: "window_focus", payload: "{}" },
    ];
    const { container } = render(
      <Timeline events={outside} dayStartMs={0} onSelect={() => {}} />,
    );
    const r = rects(container);
    expect(Number(r[0].getAttribute("x"))).toBeGreaterThanOrEqual(0);
    expect(Number(r[1].getAttribute("x"))).toBeLessThanOrEqual(1000);
  });

  it("shows nothing but stays mounted for an empty day", () => {
    const { container } = render(
      <Timeline events={[]} dayStartMs={0} onSelect={() => {}} />,
    );
    expect(rects(container).length).toBe(0);
    expect(container.querySelector("svg")).toBeTruthy();
  });

  it("calls onSelect when a block is clicked", () => {
    let picked: string | null = null;
    const { container } = render(
      <Timeline
        events={base}
        dayStartMs={0}
        onSelect={(e) => {
          picked = e.id;
        }}
      />,
    );
    rects(container)[0].dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(picked).toBe("1");
  });

  it("colors each event type distinctly and falls back for unknown types", () => {
    const mixed: StoredEvent[] = [
      { id: "i", timestamp: 1000, type: "system_idle", payload: "{}" },
      { id: "t", timestamp: 2000, type: "input_heartbeat", payload: "{}" },
      { id: "x", timestamp: 3000, type: "not_a_real_type" as never, payload: "{}" },
    ];
    const { container } = render(
      <Timeline events={mixed} dayStartMs={0} onSelect={() => {}} />,
    );
    const fills = rects(container).map((r) => r.getAttribute("fill"));
    expect(new Set(fills).size).toBe(3);
    // 未知类型应落到兜底色
    expect(fills[2]).toBe("#bdbdbd");
  });
});
