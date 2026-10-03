import { describe, it, expect } from "vitest";
import {
  bucketMetrics,
  focusStep,
  switchStep,
  scaleColor,
  FOCUS_LEGEND,
  SWITCH_LEGEND,
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
