import { describe, it, expect } from "vitest";
import {
  bucketSegments,
  bucketStart,
  formatDuration,
  GRANULARITIES,
  DEFAULT_GRANULARITY,
  summarize,
} from "./bucket";
import type { Category, Segment } from "../types";

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 86_400_000;

function seg(
  id: string,
  start: number,
  end: number,
  category: Category = "work",
): Segment {
  return {
    id,
    startAt: start,
    endAt: end,
    category,
    application: "Code.exe",
    confidence: 0.9,
    classifier: "rule",
    classifierVersion: "rules:15",
    evidenceEventIds: [],
  };
}

describe("bucketStart", () => {
  it("floors to the interval boundary", () => {
    expect(bucketStart(HOUR + 12_345, HOUR)).toBe(HOUR);
    expect(bucketStart(HOUR - 1, HOUR)).toBe(0);
    expect(bucketStart(0, HOUR)).toBe(0);
  });

  it("works for 10/30/60/120 minute intervals", () => {
    for (const m of GRANULARITIES) {
      const iv = m * MIN;
      expect(bucketStart(iv * 3 + 1, iv)).toBe(iv * 3);
      expect(bucketStart(iv * 3 - 1, iv)).toBe(iv * 2);
    }
  });

  it("handles negative timestamps without throwing", () => {
    expect(bucketStart(-1, HOUR)).toBe(-HOUR);
    expect(bucketStart(-1, 1)).toBe(-1);
  });

  it("guards against a zero interval", () => {
    // 除零会得到 NaN/Infinity，静默传下去会让整个时间线崩掉
    expect(bucketStart(HOUR, 0)).toBe(HOUR);
  });
});

describe("bucketSegments", () => {
  it("returns one bucket per interval, ascending", () => {
    const bs = bucketSegments([seg("a", 0, HOUR)], HOUR);
    expect(bs.length).toBe(1);
    expect(bs[0].start).toBe(0);
    expect(bs[0].end).toBe(HOUR);
  });

  it("puts a segment into every bucket it spans", () => {
    const bs = bucketSegments([seg("a", 0, HOUR * 2 + 30 * MIN)], HOUR);
    // 跨 2.5 小时的段应落进 3 个小时桶
    expect(bs.length).toBe(3);
    expect(bs.every((b) => b.segments.some((x) => x.id === "a"))).toBe(true);
  });

  it("a segment starting exactly on a boundary lands in the later bucket", () => {
    const bs = bucketSegments([seg("a", HOUR, HOUR * 2)], HOUR);
    expect(bs[0].start).toBe(HOUR);
    expect(bs.length).toBe(1);
  });

  it("a segment ending exactly on a boundary does not touch the next bucket", () => {
    const bs = bucketSegments([seg("a", 0, HOUR)], HOUR);
    expect(bs.length).toBe(1);
    expect(bs[0].start).toBe(0);
  });

  it("empty input yields empty buckets", () => {
    expect(bucketSegments([], HOUR)).toEqual([]);
  });

  it("buckets stay ascending even when input is unsorted", () => {
    const bs = bucketSegments(
      [seg("b", HOUR * 2, HOUR * 3), seg("a", 0, HOUR)],
      HOUR,
    );
    const starts = bs.map((b) => b.start);
    expect(starts).toEqual([...starts].sort((x, y) => x - y));
  });

  it("switching granularity never changes which buckets a segment touches", () => {
    const s = seg("a", 0, HOUR * 2 + 30 * MIN);
    for (const m of GRANULARITIES) {
      const bs = bucketSegments([s], m * MIN);
      expect(bs.length).toBeGreaterThan(0);
      expect(bs[bs.length - 1].end).toBeGreaterThan(s.startAt);
      expect(bs[0].start).toBeLessThanOrEqual(s.startAt);
    }
  });

  it("segments are clamped into the requested day", () => {
    // 跨零点的段（或时区差导致的越界）不该产生负下标的桶
    const bs = bucketSegments([seg("a", -HOUR, DAY + HOUR)], HOUR);
    expect(bs[0].start).toBeGreaterThanOrEqual(0);
    expect(bs[bs.length - 1].start).toBeLessThan(DAY);
  });

  it("a zero-length segment still lands in exactly one bucket", () => {
    const bs = bucketSegments([seg("a", HOUR, HOUR)], HOUR);
    expect(bs.length).toBe(1);
  });
});

describe("summarize", () => {
  it("totals duration per category, largest first", () => {
    const rows = summarize(
      [
        seg("a", 0, HOUR, "work"),
        seg("b", HOUR, HOUR * 2, "browsing"),
        seg("c", HOUR * 2, HOUR * 2 + 30 * MIN, "work"),
      ],
      0,
    );
    expect(rows[0].category).toBe("work");
    expect(rows[0].durationMs).toBe(HOUR + 30 * MIN);
    expect(rows[1].category).toBe("browsing");
    expect(rows[1].durationMs).toBe(HOUR);
  });

  it("reports a ratio that sums to 1", () => {
    const rows = summarize(
      [seg("a", 0, HOUR, "work"), seg("b", HOUR, HOUR * 3, "idle")],
      0,
    );
    const total = rows.reduce((a, r) => a + r.ratio, 0);
    expect(total).toBeCloseTo(1, 6);
  });

  it("clamps segments to the requested day", () => {
    const rows = summarize([seg("a", 0, HOUR * 30, "work")], 0);
    // 应被截到一天
    expect(rows[0].durationMs).toBe(HOUR * 24);
  });

  it("shifts the window when the day starts later", () => {
    // dayStart 非 0（真实场景：用户选的不是 epoch 那天）
    const base = 1_700_000_000_000;
    const rows = summarize([seg("a", base, base + HOUR, "work")], base);
    expect(rows[0].durationMs).toBe(HOUR);
  });

  it("empty input yields no rows and no NaN", () => {
    expect(summarize([], 0)).toEqual([]);
  });

  it("ignores zero and negative length segments", () => {
    const rows = summarize([seg("a", HOUR, HOUR, "work")], 0);
    expect(rows).toEqual([]);
  });
});

describe("constants", () => {
  it("default granularity is 30 minutes per spec", () => {
    expect(DEFAULT_GRANULARITY).toBe(30);
    expect(GRANULARITIES).toEqual([10, 30, 60, 120]);
  });
});

describe("formatDuration", () => {
  it("renders minutes below an hour", () => {
    expect(formatDuration(30 * MIN)).toBe("30 分");
  });

  it("renders whole hours without minutes", () => {
    expect(formatDuration(HOUR)).toBe("1 时");
  });

  it("renders hours and minutes", () => {
    expect(formatDuration(HOUR + 30 * MIN)).toBe("1 时 30 分");
  });

  it("renders zero", () => {
    expect(formatDuration(0)).toBe("0 分");
  });
});
