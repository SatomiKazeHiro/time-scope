import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  bucketMetrics,
  focusStep,
  switchStep,
  scaleColor,
  FOCUS_LEGEND,
  SWITCH_LEGEND,
  __resetOverlapReportForTest,
} from "./metrics";
import type { Category, Segment } from "../types";

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 86_400_000;
/** 当天 00:00，用整数基准避免时区把边界推到别处 */
const DAY_START = new Date(2026, 0, 15, 0, 0, 0, 0).getTime();

function seg(
  id: string,
  startMin: number,
  endMin: number,
  category: Category = "work",
  application: string | null = "Code.exe",
): Segment {
  return {
    id,
    startAt: DAY_START + startMin * MIN,
    endAt: DAY_START + endMin * MIN,
    category,
    application,
    confidence: 0.9,
    classifier: "rule",
    classifierVersion: "rules:15",
    evidenceEventIds: [],
  };
}

/** 找包含某分钟的那个桶。minute 是当天第几分钟。 */
function bucketAt(metrics: ReturnType<typeof bucketMetrics>, minute: number) {
  const widthMin = (24 * 60) / metrics.length;
  return metrics[Math.floor(minute / widthMin)];
}

describe("bucketMetrics", () => {
  it("covers the whole day with equally wide buckets", () => {
    const m = bucketMetrics([], DAY_START, 60 * MIN);
    expect(m.length).toBe(24);
    expect(m[0].start).toBe(DAY_START);
    expect(m[23].end).toBe(DAY_START + DAY);
    for (let i = 1; i < m.length; i++) {
      expect(m[i].start - m[i - 1].end).toBe(0); // 无缝无叠
    }
  });

  it("honours the granularity: 10/30/60/120 give 144/48/24/12 buckets", () => {
    for (const [min, n] of [
      [10, 144],
      [30, 48],
      [60, 24],
      [120, 12],
    ] as const) {
      expect(bucketMetrics([], DAY_START, min * MIN).length).toBe(n);
    }
  });

  it("falls back to the default granularity rather than returning nothing", () => {
    // 返回空数组会让整条时间线消失，比粒度不对更难排查
    for (const bad of [0, NaN, -1, Infinity]) {
      expect(bucketMetrics([], DAY_START, bad).length).toBe(48);
    }
  });

  it("puts a segment in every bucket it spans, splitting its time at the edges", () => {
    // 10:00–11:00 整一小时，30 分桶 = 两块，各 30 分钟
    const m = bucketMetrics([seg("a", 600, 660)], DAY_START, 30 * MIN);
    expect(bucketAt(m, 610).coveredMs).toBe(30 * MIN);
    expect(bucketAt(m, 640).coveredMs).toBe(30 * MIN);
    expect(bucketAt(m, 600).coverage).toBe(1);
  });

  it("counts only the part of a straddling segment that falls in the bucket", () => {
    // 10:50–11:10：前后两个桶各只有 10 分钟
    const m = bucketMetrics([seg("a", 650, 670)], DAY_START, 30 * MIN);
    expect(bucketAt(m, 655).coveredMs).toBe(10 * MIN);
    expect(bucketAt(m, 665).coveredMs).toBe(10 * MIN);
    expect(bucketAt(m, 655).coverage).toBeCloseTo(1 / 3, 5);
  });

  it("leaves untouched buckets empty rather than zero-filled", () => {
    const m = bucketMetrics([seg("a", 600, 660)], DAY_START, 60 * MIN);
    const empty = m[2]; // 02:00–03:00
    expect(empty.segmentCount).toBe(0);
    expect(empty.coverage).toBe(0);
    expect(empty.focus).toBe(0);
    expect(empty.dominantCategory).toBeNull();
  });

  it("clamps a segment crossing midnight to the requested day", () => {
    const crossing = seg("a", 23 * 60 + 30, 24 * 60 + 30);
    const m = bucketMetrics([crossing], DAY_START, 60 * MIN);
    const last = m[23];
    expect(last.coveredMs).toBe(30 * MIN); // 只算 23:30–24:00
    expect(last.coverage).toBeCloseTo(0.5, 5);
  });

  it("ignores zero-length and inverted segments", () => {
    const m = bucketMetrics(
      [seg("a", 600, 600), seg("b", 660, 600)],
      DAY_START,
      60 * MIN,
    );
    expect(m[10].segmentCount).toBe(0);
    expect(m[11].segmentCount).toBe(0);
  });

  it("never lets coverage exceed 1 even if segments overlap", () => {
    // 引擎不该产出重叠段，但覆盖率的定义不能依赖那个前提
    const m = bucketMetrics(
      [seg("a", 600, 660), seg("b", 605, 665)],
      DAY_START,
      60 * MIN,
    );
    expect(m[10].coverage).toBeLessThanOrEqual(1);
    expect(m[10].focus).toBeLessThanOrEqual(1);
  });

  it("counts a switch only when the application actually changes", () => {
    // 同一应用的两次切段（引擎切出来的）不算用户在跳
    const m = bucketMetrics(
      [
        seg("a", 600, 610, "work", "Code.exe"),
        seg("b", 610, 620, "work", "Code.exe"),
        seg("c", 620, 630, "browsing", "chrome.exe"),
        seg("d", 630, 640, "browsing", "chrome.exe"),
        seg("e", 640, 650, "communication", "WeChat.exe"),
      ],
      DAY_START,
      60 * MIN,
    );
    expect(m[10].segmentCount).toBe(5);
    // Code→Code(0) →chrome(1) →chrome(0) →WeChat(1) = 2
    expect(m[10].switches).toBe(2);
  });

  it("reports 0 switches for a single continuous segment", () => {
    const m = bucketMetrics([seg("a", 600, 660)], DAY_START, 60 * MIN);
    expect(m[10].switches).toBe(0);
  });

  it("focus is the dominant category's share of *active* time", () => {
    const m = bucketMetrics(
      [
        seg("a", 600, 630, "work"), // 30 分
        seg("b", 630, 645, "browsing"), // 15 分
      ],
      DAY_START,
      60 * MIN,
    );
    expect(m[10].dominantCategory).toBe("work");
    expect(m[10].focus).toBeCloseTo(30 / 45, 5);
  });

  it("focus reaches 1 for a bucket of one category", () => {
    const m = bucketMetrics([seg("a", 600, 660, "work")], DAY_START, 60 * MIN);
    expect(m[10].focus).toBe(1);
  });

  it("focus stays 0 for an empty bucket instead of becoming NaN", () => {
    const m = bucketMetrics([], DAY_START, 60 * MIN);
    expect(m[10].focus).toBe(0);
    expect(Number.isNaN(m[10].focus)).toBe(false);
  });

  it("counts distinct applications", () => {
    const m = bucketMetrics(
      [
        seg("a", 600, 620, "work", "Code.exe"),
        seg("b", 620, 640, "work", "Code.exe"),
        seg("c", 640, 660, "work", "Code.exe"),
      ],
      DAY_START,
      60 * MIN,
    );
    expect(m[10].appCount).toBe(1);
  });

  it("treats a null application as one named bucket, not as 'no app'", () => {
    const m = bucketMetrics(
      [seg("a", 600, 630, "idle", null), seg("b", 630, 660, "idle", null)],
      DAY_START,
      60 * MIN,
    );
    expect(m[10].appCount).toBe(1);
    expect(m[10].switches).toBe(0);
  });

  it("focus ignores idle time entirely", () => {
    // 挂机两小时曾被读成「高度专注 100%」：idle 是一个类别，
    // 被当成主导类别就算进了专注度。空闲不是"不专注"，是"没在工作"。
    const m = bucketMetrics(
      [seg("a", 9 * 60, 11 * 60, "idle", null)],
      DAY_START,
      60 * MIN,
    );
    const b = m[9];
    expect(b.coveredMs).toBe(HOUR);
    expect(b.activeMs).toBe(0);
    expect(b.focus).toBe(0);
    expect(b.allIdle).toBe(true);
    // 空闲桶的主导类别是 null，不是 "idle" —— 界面上据此不涂色
    expect(b.dominantCategory).toBeNull();
  });

  it("focus is diluted by idle time in the same bucket, not helped by it", () => {
    // 一半在工作一半在挂机 → 专注度 100%（只在活动部分内算），不是 50%
    const m = bucketMetrics(
      [
        seg("a", 9 * 60, 9 * 60 + 30, "work"),
        seg("b", 9 * 60 + 30, 10 * 60, "idle", null),
      ],
      DAY_START,
      60 * MIN,
    );
    expect(m[9].activeMs).toBe(30 * MIN);
    expect(m[9].coveredMs).toBe(HOUR);
    expect(m[9].focus).toBe(1);
    expect(m[9].allIdle).toBe(false);
  });

  it("a mixed idle+work bucket still names the working category", () => {
    const m = bucketMetrics(
      [
        seg("a", 9 * 60, 9 * 60 + 40, "work"),
        seg("b", 9 * 60 + 40, 10 * 60, "idle", null),
      ],
      DAY_START,
      60 * MIN,
    );
    expect(m[9].dominantCategory).toBe("work");
    expect(m[9].allIdle).toBe(false);
  });

  it("keeps segment order stable so switch counting is deterministic", () => {
    const m = bucketMetrics(
      [
        seg("late", 640, 660, "browsing", "chrome.exe"),
        seg("early", 600, 620, "work", "Code.exe"),
        seg("mid", 620, 640, "communication", "WeChat.exe"),
      ],
      DAY_START,
      60 * MIN,
    );
    // 早→中→晚 = Code→WeChat→chrome = 2 次
    expect(m[10].switches).toBe(2);
  });
});

describe("色阶分档", () => {
  it("focus maps monotonically to 1..5", () => {
    expect(focusStep(0)).toBe(1);
    expect(focusStep(0.4)).toBe(2);
    expect(focusStep(0.6)).toBe(3);
    expect(focusStep(0.8)).toBe(4);
    expect(focusStep(1)).toBe(5);
    // 单调：专注度越高档位越高，不会出现倒挂
    for (let f = 0; f <= 1.0001; f += 0.02) {
      expect(focusStep(f + 0.02)).toBeGreaterThanOrEqual(focusStep(f));
    }
  });

  it("switch count uses absolute thresholds, not a per-day maximum", () => {
    // 相对刻度会让安静的日子到处都显示成"很碎"
    expect(switchStep(0)).toBe(1);
    expect(switchStep(2)).toBe(2);
    expect(switchStep(5)).toBe(3);
    expect(switchStep(10)).toBe(4);
    expect(switchStep(11)).toBe(5);
    expect(switchStep(999)).toBe(5);
  });

  it("clamps out-of-range steps to the scale", () => {
    expect(scaleColor(0)).toBe("var(--color-scale-1)");
    expect(scaleColor(99)).toBe("var(--color-scale-5)");
    expect(scaleColor(3)).toBe("var(--color-scale-3)");
  });

  it("has one legend entry per scale step", () => {
    expect(FOCUS_LEGEND.length).toBe(5);
    expect(SWITCH_LEGEND.length).toBe(5);
  });
});

// --- 重叠段去重（2026-10-13）---

describe("重叠段不被数两遍", () => {
  const seg = (id: string, startAt: number, endAt: number, category: Category, application: string) =>
    ({ id, startAt, endAt, category, application, confidence: 1, classifier: "rule", classifierVersion: "1", evidenceEventIds: [] }) as Segment;

  it("重叠部分只计一次，且归给先开始的段", () => {
    const H = 3_600_000;
    const dayStart = 0;
    const width = 12 * H; // 12h 桶，两个段都在里面
    const segs = [
      seg("a", dayStart, dayStart + 2 * H, "work", "Code.exe"),
      seg("b", dayStart + H, dayStart + 3 * H, "browsing", "msedge.exe"),
    ];
    const [b] = bucketMetrics(segs, dayStart, width);
    // a 占 [0, 2h)，b 只拿到 [2h, 3h)
    expect(b.activeMs).toBe(3 * H);
    expect(b.activeMs).toBe(3 * H); // a 占 2h、b 只拿 1h
  });

  it("专注度不会被重叠的分母抬高", () => {
    const H = 3_600_000;
    const dayStart = 0;
    const width = 12 * H;
    const segs = [
      seg("a", dayStart, dayStart + 2 * H, "work", "Code.exe"),
      seg("b", dayStart + H, dayStart + 3 * H, "browsing", "msedge.exe"),
    ];
    const [b] = bucketMetrics(segs, dayStart, width);
    // 去重后：work 2h / active 3h = 0.667。
    // 不去重的话 active 会被算成 4h → 0.5，看起来像「被摊薄了」。
    expect(b.focus).toBeCloseTo(2 / 3, 5);
  });

  it("首尾相接不是重叠，不受影响", () => {
    const H = 3_600_000;
    const segs = [
      seg("a", 0, 2 * H, "work", "Code.exe"),
      seg("b", 2 * H, 3 * H, "work", "Code.exe"),
    ];
    const [b] = bucketMetrics(segs, 0, 12 * H);
    expect(b.activeMs).toBe(3 * H);
  });

  it("被完全覆盖的段不贡献时长", () => {
    const H = 3_600_000;
    const segs = [
      seg("outer", 0, 3 * H, "work", "Code.exe"),
      seg("inner", H, 2 * H, "browsing", "msedge.exe"),
    ];
    const [b] = bucketMetrics(segs, 0, 12 * H);
    expect(b.activeMs).toBe(3 * H);
    expect(b.dominantCategory).toBe("work");
    expect(b.segmentCount).toBe(2); // 段数仍如实计数 —— 时间线上看得见两条
  });
});


// --- 兜底必须可见（2026-10-13）---

describe("重叠去重不静默", () => {
  const seg = (id: string, startAt: number, endAt: number, category: Category, application: string) =>
    ({ id, startAt, endAt, category, application, confidence: 1, classifier: "rule", classifierVersion: "1", evidenceEventIds: [] }) as Segment;
  const H = 3_600_000;

  let errSpy: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    errSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    // 汇报器是模块级状态（按分钟节流），必须重置
    __resetOverlapReportForTest();
  });
  afterEach(() => errSpy.mockRestore());

  it("重叠被去重时必须报出来 —— 静默会让引擎侧的问题隐形", () => {
    const segs = [
      seg("a", 0, 3 * H, "work", "Code.exe"),
      seg("b", H, 2 * H, "browsing", "msedge.exe"), // 完全落在 a 之内 → 被整段盖住
    ];
    bucketMetrics(segs, 0, 12 * H);

    // 汇报器是模块级节流状态；单独测它（见下面的节流测试），
    // 这里只断言「数字仍然是去重后的正确值」
    expect(errSpy).toHaveBeenCalled();
    const msg = String(errSpy.mock.calls[0][0]);
    expect(msg).toContain("重叠");       // 说清是什么问题
    expect(msg).toContain("引擎");       // 说清该去哪儿查
    expect(msg).toMatch(/ms/);          // 带量，能判断严重程度
  });

  it("没有重叠时安静 —— 报对了但天天报就成了噪音", () => {
    const segs = [
      seg("a", 0, 2 * H, "work", "Code.exe"),
      seg("b", 2 * H, 3 * H, "work", "Code.exe"), // 首尾相接，不算重叠
    ];
    bucketMetrics(segs, 0, 12 * H);

    expect(errSpy).not.toHaveBeenCalled();
  });
});

describe("重叠汇报要节流", () => {
  const H = 3_600_000;
  const seg = (id: string, s: number, e: number, c: Category, a: string) =>
    ({ id, startAt: s, endAt: e, category: c, application: a, confidence: 1, classifier: "rule", classifierVersion: "1", evidenceEventIds: [] }) as Segment;
  const OVERLAPPING = [
    seg("a", 0, 3 * H, "work", "Code.exe"),
    seg("b", H, 2 * H, "browsing", "msedge.exe"),
  ];

  let errSpy: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    errSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    __resetOverlapReportForTest();
  });
  afterEach(() => errSpy.mockRestore());

  it("一小时内只报一次 —— 5 秒轮询会把同一批重叠刷出几十行", () => {
    // 模拟前端的 5 秒轮询：连续重算 20 次
    for (let i = 0; i < 20; i++) {
      bucketMetrics(OVERLAPPING, 0, 12 * H);
    }
    expect(
      errSpy.mock.calls.length,
      `20 次重算应只报 1 行，实际报了 ${errSpy.mock.calls.length} 行 —— 控制台被自己的告警淹没就没人看了`,
    ).toBe(1);
  });

  it("汇报带累计量，不只是「又出问题了」", () => {
    for (let i = 0; i < 3; i++) {
      bucketMetrics(OVERLAPPING, 0, 12 * H);
    }
    expect(errSpy).toHaveBeenCalledTimes(1);
    const msg = String(errSpy.mock.calls[0][0]);
    expect(msg).toContain("累计");
    expect(msg).toContain("重叠");   // 说清是什么问题
    expect(msg).toContain("重放");   // 说清怎么办
  });
});

// --- 专注度的笼统口径（2026-10-13）---
//
// 用户拍板：不做细分。「边做边走神」与「看视频」分成不同档是未来的方向，
// 现在只要一类事务聚在一起，边界清晰。
//
// 规则只有一条：
//   这一桶里，没在动的时间算不算分母？
//   - 算 → 全程在动 = 100%，摸鱼与看视频同为满档（笼统，但诚实）
//   - 不算 → 分母只剩「有输入」的部分
// 选后者，因为它至少把「人不在」（锁屏、走开、长时间无输入）从满档里摘出去了 ——
// 而那是最该被摘出去的一种。

describe("专注度的笼统口径", () => {
  const H = 3_600_000;
  const mk = (
    id: string, s: number, e: number, c: Category, activeMs: number | null, app = "Code.exe",
  ) => ({
    id, startAt: s, endAt: e, category: c, application: app,
    confidence: 1, classifier: "rule", classifierVersion: "1",
    evidenceEventIds: [], activeMs,
  }) as Segment;

  it("全程在动 → 100%", () => {
    const [b] = bucketMetrics([mk("a", 0, H, "work", H)], 0, 12 * H);
    expect(b.focus).toBeCloseTo(1, 5);
  });

  it("人不在（一段时间完全没动）→ 摘出去，不占分母", () => {
    // 1 小时的段，其中只有 20 分钟在动
    const [b] = bucketMetrics([mk("a", 0, H, "work", 20 * 60_000)], 0, 12 * H);
    expect(b.focus, "有输入的 20 分钟里全是 work → 仍然 100%").toBeCloseTo(1, 5);
    // 但 inputMs 记下了「只有 20 分钟在动」这件事
    expect(b.inputMs).toBe(20 * 60_000);
  });

  it("全程没动 → 0，不画色", () => {
    const [b] = bucketMetrics([mk("a", 0, H, "work", 0)], 0, 12 * H);
    expect(b.focus).toBe(0);
  });

  it("历史数据（无 activeMs）→ 退回旧口径，不假装知道", () => {
    const [b] = bucketMetrics([mk("old", 0, H, "work", null)], 0, 12 * H);
    expect(b.inputMs, "没有这个量").toBeNull();
    expect(b.focus, "用旧口径算，不要显示成 0").toBeCloseTo(1, 5);
  });
});

// --- readout：这一格有没有可读的数（2026-10-17）---
//
// 这个判断原来在 SegmentTimeline 里写了四遍，判据各不相同（导航用段数、
// 涂色用段数+两个 0、读屏用 activeMs、浮层再拼一次）。现在由 metrics 一处给出，
// 组件只读结论。下面的四条边界就是那四种状态。

describe("readout", () => {
  const withInput = (id: string, startMin: number, endMin: number, activeMs?: number): Segment => ({
    ...seg(id, startMin, endMin),
    activeMs,
  });

  it("四种状态互斥且齐全", () => {
    const m = bucketMetrics(
      [
        withInput("hist", 0, 60),                    // 历史数据：没有 activeMs
        { ...seg("idle-only", 120, 180, "idle") },     // 全空闲
        withInput("no-input", 240, 300, 0),          // 有活动、没输入
        withInput("active", 360, 420, 30 * MIN),     // 有活动有输入
      ],
      DAY_START,
      60 * MIN,
    );
    const at = (h: number) => m[h].readout;
    expect(at(0), "inputMs 为 null（历史数据）仍算可读").toBe("value");
    expect(at(2), "只有空闲段").toBe("idle");
    expect(at(4), "有活动但 inputMs 恰好 0").toBe("no-input");
    expect(at(6)).toBe("value");
    expect(at(8), "桶里一段都没有").toBe("empty");
  });

  it("全空闲桶的 idle 与「没段」的 empty 必须分开 —— 前者点得动、读得出话", () => {
    const [idleBucket] = bucketMetrics([{ ...seg("i", 0, 60, "idle"), activeMs: 0 }], DAY_START, DAY);
    expect(idleBucket.readout).toBe("idle");
    expect(idleBucket.segmentCount).toBe(1);
    const [emptyBucket] = bucketMetrics([], DAY_START, DAY);
    expect(emptyBucket.readout).toBe("empty");
    expect(emptyBucket.segmentCount).toBe(0);
  });
});
