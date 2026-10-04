import { describe, it, expect } from "vitest";
import {
  bucketStart,
  formatDuration,
  GRANULARITIES,
  DEFAULT_GRANULARITY,
  summarize,
} from "./bucket";
import type { Category, Segment } from "../types";

const MIN = 60_000;
const HOUR = 60 * MIN;

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
