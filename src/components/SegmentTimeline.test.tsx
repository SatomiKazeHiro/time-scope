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
    // 后端加了新 category 而前端没跟上时，不该渲染成透明/无色。
    // 兜底指向 CSS 变量而不是写死 hex：色值真相只在 styles/theme.css 一处。
    const bogus = "brand_new" as Category;
    expect(colorForCategory(bogus)).toBe("var(--color-cat-unknown)");
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

/** 「此刻」游标：唯一一根用 cursor 色的 line。 */
function nowCursor(container: HTMLElement): SVGLineElement | null {
  return (
    Array.from(container.querySelectorAll("line")).find(
      (l) => l.getAttribute("stroke") === "var(--color-cursor-now)",
    ) ?? null
  );
}

describe("SegmentTimeline 刻度尺与此刻游标", () => {
  const today: Segment[] = [
    {
      id: "a",
      startAt: Date.now() - 3_600_000,
      endAt: Date.now(),
      category: "work",
      application: "Code.exe",
      confidence: 0.9,
      classifier: "rule",
      classifierVersion: "rules:15",
      evidenceEventIds: [],
    },
  ];

  it("看今天时画此刻游标", () => {
    const { container } = render(
      <SegmentTimeline segments={today} dayStartMs={0} onSelect={() => {}} showNow />,
    );
    expect(nowCursor(container)).not.toBeNull();
  });

  it("看历史日期时不画 —— 那天没有「现在」", () => {
    const { container } = render(
      <SegmentTimeline segments={today} dayStartMs={0} onSelect={() => {}} />,
    );
    expect(nowCursor(container)).toBeNull();
  });

  it("游标不吃鼠标事件，绝不挡用户点色块", () => {
    const { container } = render(
      <SegmentTimeline segments={today} dayStartMs={0} onSelect={() => {}} showNow />,
    );
    expect(nowCursor(container)!.getAttribute("pointer-events")).toBe("none");
  });

  it("游标贯穿轨道和底部刻度尺", () => {
    // 刻度尺在轨道下方，游标只画轨道的一半就等于没有时间感
    const { container } = render(
      <SegmentTimeline segments={today} dayStartMs={0} onSelect={() => {}} showNow />,
    );
    const svg = container.querySelector("svg")!;
    const cursor = nowCursor(container)!;
    expect(Number(cursor.getAttribute("y2"))).toBe(Number(svg.getAttribute("viewBox")!.split(" ")[3]));
  });

  it("底部每 10 分钟一根刻度、每 60 分钟一根更高的", () => {
    const { container } = render(
      <SegmentTimeline segments={today} dayStartMs={0} onSelect={() => {}} />,
    );
    // 刻度组用 data-ruler 定位：stroke 挂在 <g> 上，逐根 <line> 读不到
    const ruler = Array.from(
      container.querySelectorAll('[data-ruler="ticks"] line'),
    ) as SVGLineElement[];
    // 一天 24×60 根
    expect(ruler.length).toBe(144);
    const heights = new Set(
      ruler.map((l) => Number(l.getAttribute("y2")) - Number(l.getAttribute("y1"))),
    );
    expect(heights.size).toBe(2); // 小刻度 8 单位、大刻度 16 单位
  });

  it("刻度和游标都不增加 rect —— 测试拿 rect 数量断言段数", () => {
    const { container } = render(
      <SegmentTimeline segments={today} dayStartMs={0} onSelect={() => {}} showNow />,
    );
    expect(rects(container).length).toBe(today.length);
  });
});
