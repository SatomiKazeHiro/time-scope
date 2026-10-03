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

describe("SegmentTimeline 粒度切片", () => {
  const oneHour = seg("a", 9 * HOUR, 10 * HOUR, "work");

  it("intervalMs 为 0 时不切，一个段一个 rect", () => {
    const { container } = render(
      <SegmentTimeline segments={[oneHour]} dayStartMs={0} onSelect={() => {}} intervalMs={0} />,
    );
    expect(rects(container).length).toBe(1);
  });

  it("给了 intervalMs 就按桶边界真的切段", () => {
    // 这个控件之前只改了个桶计数，视图纹丝不动 —— 这条断言就是防它退回去
    const { container } = render(
      <SegmentTimeline
        segments={[oneHour]}
        dayStartMs={0}
        onSelect={() => {}}
        intervalMs={30 * 60_000}
      />,
    );
    expect(rects(container).length).toBe(2);
  });

  it("点切片里的任意一块，选中的都是整段活动", () => {
    // 切的是时间，不是活动 —— 选中半段会让人以为后半段是另一次活动
    const picked: string[] = [];
    const { container } = render(
      <SegmentTimeline
        segments={[oneHour]}
        dayStartMs={0}
        onSelect={(s) => picked.push(s.id)}
        intervalMs={10 * 60_000}
      />,
    );
    const r = rects(container);
    r.forEach((rect) => rect.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    expect(rects(container).length).toBe(6);
    expect(picked).toEqual(["a", "a", "a", "a", "a", "a"]);
  });

  it("切出来的块共享同一个 data-id，测试与读数都还认得出它们是一段", () => {
    const { container } = render(
      <SegmentTimeline
        segments={[oneHour]}
        dayStartMs={0}
        onSelect={() => {}}
        intervalMs={30 * 60_000}
      />,
    );
    expect(new Set(rects(container).map((r) => r.getAttribute("data-id")))).toEqual(
      new Set(["a"]),
    );
  });

  it("切片只标第一块，同一段连切三刀不会标三次", () => {
    const long = seg("b", 9 * HOUR, 12 * HOUR, "work"); // 3 小时 → 6 块
    const { container } = render(
      <SegmentTimeline
        segments={[long]}
        dayStartMs={0}
        onSelect={() => {}}
        intervalMs={30 * 60_000}
      />,
    );
    expect(rects(container).length).toBe(6);
    // 段内直标是 aria-hidden 的装饰文本，数 span 即可
    const labels = Array.from(container.querySelectorAll("span")).filter((s) =>
      ["工作"].includes(s.textContent ?? ""),
    );
    expect(labels.length).toBe(1);
  });

  it("大刻度跟随粒度，于是这个控件在刻度尺上也看得见反应", () => {
    const majors = (intervalMs: number) => {
      const { container } = render(
        <SegmentTimeline
          segments={[oneHour]}
          dayStartMs={0}
          onSelect={() => {}}
          intervalMs={intervalMs}
        />,
      );
      const g = container.querySelector('[data-ruler="ticks"]')!;
      return Array.from(g.querySelectorAll("line")).filter((l) => {
        const h = Number(l.getAttribute("y2")) - Number(l.getAttribute("y1"));
        return h > 6; // 整根高度的是大刻度
      }).length;
    };
    // 120 分粒度一天 12 根大刻度，30 分粒度 48 根，10 分粒度 144 根
    expect(majors(120 * 60_000)).toBe(12);
    expect(majors(30 * 60_000)).toBe(48);
    expect(majors(10 * 60_000)).toBe(144);
  });
});

describe("SegmentTimeline 指标模式", () => {
  const HOUR_30 = 30 * 60_000;
  /** 9:00–11:00 连着做，中间夹一次应用切换 */
  const busy = seg("a", 9 * HOUR, 11 * HOUR, "work");

  it("指标模式画的是桶，不是段", () => {
    const { container } = render(
      <SegmentTimeline
        segments={[busy]}
        dayStartMs={0}
        onSelect={() => {}}
        intervalMs={HOUR_30}
        metric="focus"
      />,
    );
    // 30 分桶一天 48 格，每格一个桶；段只有 1 个
    expect(container.querySelectorAll("[data-bucket]").length).toBe(48);
    expect(container.querySelectorAll("[data-id]").length).toBe(0);
  });

  it("没有活动的桶留空，不涂成最低档", () => {
    // 涂最低档会被读成"这里有活动但专注度极低"，和"根本没活动"是两回事
    const { container } = render(
      <SegmentTimeline
        segments={[busy]}
        dayStartMs={0}
        onSelect={() => {}}
        intervalMs={HOUR_30}
        metric="focus"
      />,
    );
    const empty = container.querySelector('[data-bucket="2"]')!; // 01:00–01:30
    expect(empty.getAttribute("fill")).toBe("transparent");
    const filled = container.querySelector('[data-bucket="18"]')!; // 09:00–09:30
    expect(filled.getAttribute("fill")).toMatch(/^var\(--color-scale-/);
  });

  it("点一格选中的是该桶里的活动", () => {
    const picked: string[] = [];
    const { container } = render(
      <SegmentTimeline
        segments={[busy]}
        dayStartMs={0}
        onSelect={(s) => picked.push(s.id)}
        intervalMs={HOUR_30}
        metric="switch"
      />,
    );
    container
      .querySelector('[data-bucket="18"]')!
      .dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(picked).toEqual(["a"]);
  });

  it("指标模式只圈被点的那一格，不圈整段覆盖的所有桶", () => {
    // 一段横跨四格，全圈上会画出一道白栅栏
    const { container } = render(
      <SegmentTimeline
        segments={[busy]}
        dayStartMs={0}
        onSelect={() => {}}
        intervalMs={HOUR_30}
        metric="focus"
        selectedBucket={18}
      />,
    );
    const ringed = Array.from(container.querySelectorAll("[data-bucket]")).filter(
      (r) => r.getAttribute("style")?.includes("var(--color-ink)"),
    );
    expect(ringed.length).toBe(1);
  });

  it("全是空闲的桶不涂色 —— 空闲是「没在工作」，不是「不专注」", () => {
    // 曾经把 idle 当成一个类别算进专注度，挂机两小时显示成「高度专注 100%」
    const idleAll = seg("i", 9 * HOUR, 11 * HOUR, "idle", null);
    const { container } = render(
      <SegmentTimeline
        segments={[idleAll]}
        dayStartMs={0}
        onSelect={() => {}}
        intervalMs={30 * 60_000}
        metric="focus"
      />,
    );
    const b = (i: number) => container.querySelector(`[data-bucket="${i}"]`)!;
    expect(b(18).getAttribute("fill")).toBe("transparent"); // 09:00–09:30
    expect(b(21).getAttribute("fill")).toBe("transparent"); // 10:30–11:00
  });

  it("混合桶（工作 + 挂机）照常涂色，工作那部分算专注度", () => {
    const mixed = seg("m", 9 * HOUR, 10 * HOUR, "work");
    const idle = seg("i", 10 * HOUR, 11 * HOUR, "idle", null);
    const { container } = render(
      <SegmentTimeline
        segments={[mixed, idle]}
        dayStartMs={0}
        onSelect={() => {}}
        intervalMs={60 * 60_000}
        metric="focus"
      />,
    );
    const work = container.querySelector('[data-bucket="9"]')!;
    const idleB = container.querySelector('[data-bucket="10"]')!;
    expect(work.getAttribute("fill")).toMatch(/^var\(--color-scale-/);
    expect(idleB.getAttribute("fill")).toBe("transparent");
  });

  it("空桶点了也不会炸", () => {
    const picked: string[] = [];
    const { container } = render(
      <SegmentTimeline
        segments={[busy]}
        dayStartMs={0}
        onSelect={(s) => picked.push(s.id)}
        intervalMs={HOUR_30}
        metric="focus"
      />,
    );
    container
      .querySelector('[data-bucket="2"]')!
      .dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(picked).toEqual([]);
  });
});
