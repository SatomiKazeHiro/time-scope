import { describe, it, expect } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
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

describe("SegmentTimeline 类别模式不受粒度影响", () => {
  const oneHour = seg("a", 9 * HOUR, 10 * HOUR, "work");

  for (const intervalMs of [0, 10 * 60_000, 30 * 60_000, 60 * 60_000, 120 * 60_000]) {
    it(`粒度 ${intervalMs / 60_000} 分钟时，一个段仍然是一个 rect`, () => {
      // 段是引擎判定的活动边界，它是什么就是什么。按时间格切一刀只会把一段
      // 连续活动切碎，既不增加信息（宽度已经表示时长），又让"这段多长"要靠心算。
      // 类别模式下粒度只管底部刻度尺的大刻度。
      const { container } = render(
        <SegmentTimeline
          segments={[oneHour]}
          dayStartMs={0}
          onSelect={() => {}}
          intervalMs={intervalMs}
        />,
      );
      expect(rects(container).length).toBe(1);
    });
  }

  it("贴着 24:00 的段，标签不会被 overflow-hidden 切掉", () => {
    // 曾经用切片时代的补偿公式定位标签，20:00–24:00 这种段算出来
    // left + width = 108%，标签右半截被容器裁掉。
    const { container } = render(
      <SegmentTimeline
        segments={[seg("a", 20 * HOUR, 24 * HOUR, "work")]}
        dayStartMs={0}
        onSelect={() => {}}
      />,
    );
    const span = container.querySelector("span[aria-hidden]") as HTMLElement;
    expect(span).toBeTruthy();
    const right = parseFloat(span.style.left) + parseFloat(span.style.width);
    expect(right).toBeLessThanOrEqual(100.01);
    const left = parseFloat(span.style.left);
    expect(left).toBeGreaterThanOrEqual(0);
  });

  it("倒挂 / 零长段不把轨道画歪", () => {
    // 引擎不该产出这种段，但负宽度会让 x 飞到视图外
    const { container } = render(
      <SegmentTimeline
        segments={[seg("a", 10 * HOUR, 9 * HOUR, "work"), seg("b", 12 * HOUR, 12 * HOUR, "idle")]}
        dayStartMs={0}
        onSelect={() => {}}
      />,
    );
    for (const r of rects(container)) {
      expect(Number(r.getAttribute("x"))).toBeLessThanOrEqual(1000);
      expect(Number(r.getAttribute("width"))).toBeGreaterThanOrEqual(0);
    }
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

// --- B9：hoverIndex 在「段下标」与「桶下标」两套语义间复用 ---

describe("SegmentTimeline 悬停态跨模式", () => {
  /** 碎片化的一天：125 个短段，下标比 30 分粒度的 48 个桶大得多。 */
  function fragmentedDay(n: number): Segment[] {
    const step = (DAY - 1) / n;
    return Array.from({ length: n }, (_, i) =>
      seg(`s${i}`, Math.floor(i * step), Math.floor((i + 0.6) * step), "work"),
    );
  }

  it("切到指标模式后不再拿段下标当桶下标读数", () => {
    // B9。`hoverIndex` 一个 state 承载两套语义：
    //   类别模式 `:325` 写的是**段**下标（125 段 → 0..124）
    //   指标模式 `:280` 写的是**桶**下标（30 分粒度 → 0..47）
    // 鼠标悬停后用键盘切模式，onMouseLeave 不会触发 → 悬停态原样留着。
    // 于是指标模式下拿"第 120 段"当"第 120 桶"去读，读出的是
    // **另一个时间范围**的专注度 —— 一个凭空捏造的数值。
    const segments = fragmentedDay(125);
    const { container, rerender } = render(
      <SegmentTimeline
        segments={segments}
        dayStartMs={0}
        intervalMs={30 * 60_000}
        metric="category"
        onSelect={() => {}}
      />,
    );

    // 悬停到最后一个段（段下标 124）
    const last = rects(container).at(-1)!;
    fireEvent.mouseMove(last, { clientX: 900, clientY: 10 });
    expect(screen.getByRole("tooltip")).toBeTruthy();

    // 鼠标没有移开（真实场景里是键盘切模式），切到指标模式
    rerender(
      <SegmentTimeline
        segments={segments}
        dayStartMs={0}
        intervalMs={30 * 60_000}
        metric="focus"
        onSelect={() => {}}
      />,
    );

    // 模式一换，旧的悬停下标就不再有意义了 —— 提示框应当消失
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("切粒度后同理：桶数变了，旧下标指向的是别的时段", () => {
    // 同源的第二个入口：桶下标也是粒度的函数。悬停时切粒度而不移开鼠标，
    // 提示框会继续报旧下标的读数 —— 那已经是另一段时间了。
    const segments = fragmentedDay(125);
    const { container, rerender } = render(
      <SegmentTimeline
        segments={segments}
        dayStartMs={0}
        intervalMs={10 * 60_000}
        metric="focus"
        onSelect={() => {}}
      />,
    );
    const bucket = container.querySelector('[data-bucket="120"]');
    expect(bucket).not.toBeNull();
    fireEvent.mouseMove(bucket!, { clientX: 900, clientY: 10 });
    expect(screen.getByRole("tooltip")).toBeTruthy();

    rerender(
      <SegmentTimeline
        segments={segments}
        dayStartMs={0}
        intervalMs={120 * 60_000}
        metric="focus"
        onSelect={() => {}}
      />,
    );
    expect(screen.queryByRole("tooltip")).toBeNull();
  });
});

// --- B8：时间线色块键盘可达 ---

describe("SegmentTimeline 键盘可达性", () => {
  const segments = [
    seg("a", 0, HOUR, "work"),
    seg("b", HOUR, HOUR * 2, "browsing"),
  ];

  it("色块是可聚焦的按钮，而不是只有鼠标能点", () => {
    // B8。`<svg role="img">` 把整棵树对辅助技术声明成「一张图」，
    // 子节点一律被遮蔽；加上 `<rect>` 本身没有 tabIndex/role，
    // 于是「点色块看这段在干什么」这个**核心操作纯键盘完全到不了**。
    const { container } = render(
      <SegmentTimeline segments={segments} dayStartMs={0} onSelect={() => {}} />,
    );
    const cells = rects(container);
    for (const c of cells) {
      expect(c.getAttribute("role")).toBe("button");
      expect(["0", "-1"], "每个色块都要可聚焦").toContain(c.getAttribute("tabindex"));
    }
    // 注意：**可聚焦 ≠ 每个都占一个 Tab 位**。那是 roving tabindex 的事
    //（见下面那组测试）——这里只断「每一个都是能聚焦的按钮」。
    expect(cells.filter((c) => c.getAttribute("tabindex") === "0").length).toBe(1);
  });

  it("色块的无障碍名里有时间、类别和时长", () => {
    // 光有 role 没有名字等于没说 —— 读屏只会念「按钮」。
    //
    // 时间用**本地分量**构造：`clockOf` 走的是本地时区，写死 epoch 0 会让
    // 断言随开发机时区漂（本机 UTC-8 下 `0` 渲染成 08:00）。
    const dayStart = new Date(2026, 0, 1, 9, 0, 0, 0).getTime();
    const { container } = render(
      <SegmentTimeline
        segments={[seg("a", dayStart, dayStart + HOUR, "work")]}
        dayStartMs={dayStart}
        onSelect={() => {}}
      />,
    );
    const label = rects(container)[0].getAttribute("aria-label") ?? "";
    expect(label).toContain("09:00");
    expect(label).toContain("10:00");
    expect(label).toContain("工作");
    expect(label).toContain("Code.exe");
    expect(label).toMatch(/\d/); // 时长
  });

  it("回车与空格都能选中色块", () => {
    // 只有 onClick 的元素对键盘等于死的：Enter/Space 不会触发。
    const picked: string[] = [];
    const { container } = render(
      <SegmentTimeline
        segments={segments}
        dayStartMs={0}
        onSelect={(s) => picked.push(s.id)}
      />,
    );
    fireEvent.keyDown(rects(container)[0], { key: "Enter" });
    fireEvent.keyDown(rects(container)[1], { key: " " });
    expect(picked).toEqual(["a", "b"]);
  });

  it("指标模式的桶同样可聚焦", () => {
    const { container } = render(
      <SegmentTimeline
        segments={segments}
        dayStartMs={0}
        intervalMs={30 * 60_000}
        metric="focus"
        onSelect={() => {}}
      />,
    );
    const buckets = Array.from(
      container.querySelectorAll("[data-bucket]"),
    ) as SVGRectElement[];
    expect(buckets.length).toBeGreaterThan(0);
    for (const b of buckets) {
      expect(b.getAttribute("role")).toBe("button");
      expect(["0", "-1"], "每个桶都要可聚焦").toContain(b.getAttribute("tabindex"));
    }
    expect(buckets.filter((b) => b.getAttribute("tabindex") === "0").length).toBe(1);
  });

  it("容器不再对辅助技术声明成一张图（否则子节点仍然被遮蔽）", () => {
    const { container } = render(
      <SegmentTimeline segments={segments} dayStartMs={0} onSelect={() => {}} />,
    );
    const svg = container.querySelector("svg")!;
    expect(svg.getAttribute("role")).not.toBe("img");
  });
});

// --- roving tabindex：整条时间线只占一个 Tab 位 ---

describe("SegmentTimeline 方向键导航（roving tabindex）", () => {
  /** 碎片化的一天：125 个短段（真实数据量级）。 */
  function fragmented(n: number): Segment[] {
    const step = (DAY - 1) / n;
    return Array.from({ length: n }, (_, i) =>
      seg(`s${i}`, Math.floor(i * step), Math.floor((i + 0.6) * step), "work"),
    );
  }

  /** 当前 tabbable 的 rect 下标 —— 只有一个是 0，其余都是 -1。 */
  function rovingIndex(container: HTMLElement): number {
    const tabbable = rects(container).filter((r) => r.getAttribute("tabindex") === "0");
    expect(tabbable.length, "整条时间线只能有一个 Tab 停靠点").toBe(1);
    return tabbable[0].getAttribute("data-id") === undefined
      ? Number(tabbable[0].getAttribute("data-bucket"))
      : rects(container).indexOf(tabbable[0]);
  }

  it("125 个段也只占一个 Tab 位，而不是 125 个", () => {
    // 这是 roving tabindex 存在的全部理由：每个色块都 tabIndex=0 的话，
    // 键盘用户要按 125 次才能穿过时间线，点完色块按 Tab 也一样出不来。
    const segments = fragmented(125);
    const { container } = render(
      <SegmentTimeline segments={segments} dayStartMs={0} onSelect={() => {}} />,
    );
    const tabbable = rects(container).filter((r) => r.getAttribute("tabindex") === "0");
    expect(tabbable.length).toBe(1);
    expect(rovingIndex(container)).toBe(0);
  });

  it("方向键把焦点移到下一段", () => {
    const segments = fragmented(5);
    const { container } = render(
      <SegmentTimeline segments={segments} dayStartMs={0} onSelect={() => {}} />,
    );
    fireEvent.keyDown(rects(container)[0], { key: "ArrowRight" });
    expect(rovingIndex(container)).toBe(1);
    fireEvent.keyDown(rects(container)[1], { key: "ArrowRight" });
    expect(rovingIndex(container)).toBe(2);
    fireEvent.keyDown(rects(container)[2], { key: "ArrowLeft" });
    expect(rovingIndex(container)).toBe(1);
  });

  it("Home / End 跳到当天两端", () => {
    const segments = fragmented(5);
    const { container } = render(
      <SegmentTimeline segments={segments} dayStartMs={0} onSelect={() => {}} />,
    );
    fireEvent.keyDown(rects(container)[0], { key: "End" });
    expect(rovingIndex(container)).toBe(4);
    fireEvent.keyDown(rects(container)[4], { key: "Home" });
    expect(rovingIndex(container)).toBe(0);
  });

  it("到头就停，不循环", () => {
    // 与「上一个/下一个」按钮定的行为一致：到头停，不跳到另一头。
    // （从 23:59 按一下跳到 00:00 跨度太大，位置感会丢。）
    const segments = fragmented(3);
    const { container } = render(
      <SegmentTimeline segments={segments} dayStartMs={0} onSelect={() => {}} />,
    );
    // 先走到最后一段
    fireEvent.keyDown(rects(container)[0], { key: "End" });
    expect(rovingIndex(container)).toBe(2);
    // 再按 → 不动（不循环回第 0 段）
    fireEvent.keyDown(rects(container)[2], { key: "ArrowRight" });
    expect(rovingIndex(container)).toBe(2);
    // 回到最前，← 同理不动
    fireEvent.keyDown(rects(container)[2], { key: "Home" });
    expect(rovingIndex(container)).toBe(0);
    fireEvent.keyDown(rects(container)[0], { key: "ArrowLeft" });
    expect(rovingIndex(container)).toBe(0);
  });

  it("方向键移动后 DOM 焦点真的落在那一格上", () => {
    // 光改 tabIndex 不够：不把焦点移过去，用户 Tab 出去再回来会回到原处。
    const segments = fragmented(5);
    const { container } = render(
      <SegmentTimeline segments={segments} dayStartMs={0} onSelect={() => {}} />,
    );
    rects(container)[0].focus();
    fireEvent.keyDown(document.activeElement ?? rects(container)[0], { key: "ArrowRight" });
    expect(document.activeElement).toBe(rects(container)[1]);
  });

  it("Enter 仍然是打开详情，不是移动", () => {
    // B8 已经定下的行为，别被方向键改掉。
    const picked: string[] = [];
    const segments = fragmented(3);
    const { container } = render(
      <SegmentTimeline segments={segments} dayStartMs={0} onSelect={(s) => picked.push(s.id)} />,
    );
    fireEvent.keyDown(rects(container)[0], { key: "Enter" });
    expect(picked).toEqual(["s0"]);
    expect(rovingIndex(container)).toBe(0);
  });

  it("指标模式：方向键跳过没有任何活动的空格", () => {
    // 空格属于"空白区 / 未监测"，不是一段活动。键盘走过去没有东西可看，
    // 也不该让读屏念一个按了没反应的按钮。
    const segments = [
      seg("a", 0, HOUR, "work"),
      // 1–23 点没有任何活动：24 格里有 23 格是空的
    ];
    const { container } = render(
      <SegmentTimeline
        segments={segments}
        dayStartMs={0}
        intervalMs={60 * 60_000}
        metric="focus"
        onSelect={() => {}}
      />,
    );
    const buckets = Array.from(container.querySelectorAll("[data-bucket]")) as SVGRectElement[];
    expect(buckets.length).toBe(24);
    expect(buckets.filter((b) => b.getAttribute("tabindex") === "0").length).toBe(1);

    // 只有第 0 格有数据，所以 End 之后还停在第 0 格
    fireEvent.keyDown(buckets[0], { key: "End" });
    const stillFirst = buckets.filter((b) => b.getAttribute("tabindex") === "0");
    expect(stillFirst.length).toBe(1);
    expect(stillFirst[0].getAttribute("data-bucket")).toBe("0");
  });

  it("指标模式：在有数据的格子之间移动", () => {
    const segments = [
      seg("a", 0, HOUR, "work"),
      seg("b", 3 * HOUR, 4 * HOUR, "browsing"),
    ];
    const { container } = render(
      <SegmentTimeline
        segments={segments}
        dayStartMs={0}
        intervalMs={60 * 60_000}
        metric="focus"
        onSelect={() => {}}
      />,
    );
    const buckets = Array.from(container.querySelectorAll("[data-bucket]")) as SVGRectElement[];
    // 第 0 格和第 3 格有数据；从第 0 格按 → 应直接到第 3 格，跳过 1、2
    fireEvent.keyDown(buckets[0], { key: "ArrowRight" });
    const tabbable = buckets.filter((b) => b.getAttribute("tabindex") === "0");
    expect(tabbable.length).toBe(1);
    expect(tabbable[0].getAttribute("data-bucket")).toBe("3");
  });

  it("切粒度后仍然只有一个 Tab 位", () => {
    // 桶数变了，roving 下标必须跟着复位，否则会指向不存在的格。
    const segments = fragmented(125);
    const { container, rerender } = render(
      <SegmentTimeline
        segments={segments}
        dayStartMs={0}
        intervalMs={30 * 60_000}
        metric="focus"
        onSelect={() => {}}
      />,
    );
    fireEvent.keyDown(
      (container.querySelector("[data-bucket]") as SVGRectElement)!,
      { key: "End" },
    );
    rerender(
      <SegmentTimeline
        segments={segments}
        dayStartMs={0}
        intervalMs={120 * 60_000}
        metric="focus"
        onSelect={() => {}}
      />,
    );
    const buckets = Array.from(container.querySelectorAll("[data-bucket]")) as SVGRectElement[];
    const tabbable = buckets.filter((b) => b.getAttribute("tabindex") === "0");
    expect(tabbable.length).toBe(1);
    expect(tabbable[0].getAttribute("data-bucket")).toBe("0");
  });
});
