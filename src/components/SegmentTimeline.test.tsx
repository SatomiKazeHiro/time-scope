import { describe, it, expect } from "vitest";
import { render } from "@testing-library/react";
import SegmentTimeline, { CATEGORY_COLOR, colorForCategory } from "./SegmentTimeline";
import type { Category, Segment } from "../types";

const HOUR = 3_600_000;
const DAY = 86_400_000;

function seg(
  id: string,
  start: number,
  end: number,
  category: Category,
  application: string | null = "Code.exe",
): Segment {
  return {
    id,
    startAt: start,
    endAt: end,
    category,
    application,
    confidence: 0.9,
    classifier: "rule",
    classifierVersion: "rules:15",
    evidenceEventIds: ["e1", "e2"],
  };
}

function rects(container: HTMLElement): SVGRectElement[] {
  return Array.from(container.querySelectorAll("rect")) as SVGRectElement[];
}

describe("SegmentTimeline", () => {
  it("renders one rect per segment", () => {
    const { container } = render(
      <SegmentTimeline
        segments={[seg("a", 0, HOUR, "work"), seg("b", HOUR, HOUR * 2, "idle")]}
        dayStartMs={0}
        onSelect={() => {}}
      />,
    );
    expect(rects(container).length).toBe(2);
  });

  it("width is proportional to duration, not fixed", () => {
    const { container } = render(
      <SegmentTimeline
        segments={[seg("short", 0, 60_000, "work"), seg("long", 0, HOUR, "work")]}
        dayStartMs={0}
        onSelect={() => {}}
      />,
    );
    const byId = Object.fromEntries(
      rects(container).map((r) => [r.getAttribute("data-id"), Number(r.getAttribute("width"))]),
    );
    expect(byId["long"]).toBeGreaterThan(byId["short"]);
  });

  it("gives every category a distinct color", () => {
    const cats: Category[] = [
      "work", "study", "entertainment", "communication",
      "browsing", "life", "idle", "unknown",
    ];
    const colors = cats.map(colorForCategory);
    expect(new Set(colors).size).toBe(cats.length);
    for (const c of cats) {
      expect(CATEGORY_COLOR[c]).toBeTruthy();
    }
  });

  it("falls back to a neutral color for an unknown category", () => {
    // 后端加了新 category 而前端没跟上时，不该渲染成透明/无色
    const bogus = "brand_new" as Category;
    expect(colorForCategory(bogus)).toBe("#e0e0e0");
  });

  it("clamps segments extending past the day", () => {
    const { container } = render(
      <SegmentTimeline
        segments={[seg("a", -HOUR, DAY + HOUR, "work")]}
        dayStartMs={0}
        onSelect={() => {}}
      />,
    );
    const r = rects(container)[0];
    expect(Number(r.getAttribute("x"))).toBeGreaterThanOrEqual(0);
    expect(Number(r.getAttribute("width"))).toBeLessThanOrEqual(1000);
  });

  it("gives a zero-length segment a visible sliver", () => {
    // 0 宽度的 rect 看不见，用户会以为数据丢了
    const { container } = render(
      <SegmentTimeline segments={[seg("a", HOUR, HOUR, "work")]} dayStartMs={0} onSelect={() => {}} />,
    );
    expect(Number(rects(container)[0].getAttribute("width"))).toBeGreaterThan(0);
  });

  it("calls onSelect on click", () => {
    let picked: string | null = null;
    const { container } = render(
      <SegmentTimeline
        segments={[seg("a", 0, HOUR, "work")]}
        dayStartMs={0}
        onSelect={(s) => { picked = s.id; }}
      />,
    );
    rects(container)[0].dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(picked).toBe("a");
  });

  it("shows a hint for an empty day instead of an empty bar", () => {
    const { container } = render(
      <SegmentTimeline segments={[]} dayStartMs={0} onSelect={() => {}} />,
    );
    expect(rects(container).length).toBe(0);
    expect(container.textContent).toMatch(/还没有活动段/);
  });
});
