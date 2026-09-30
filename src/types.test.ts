import { describe, it, expect } from "vitest";
import { colorFor, parsePayload, shiftDate, todayString } from "./types";

describe("shiftDate", () => {
  it("rolls forward across a month boundary", () => {
    expect(shiftDate("2026-01-31", 1)).toBe("2026-02-01");
  });

  it("rolls backward across a month boundary", () => {
    expect(shiftDate("2026-03-01", -1)).toBe("2026-02-28");
  });

  it("rolls across a year boundary", () => {
    expect(shiftDate("2026-12-31", 1)).toBe("2027-01-01");
    expect(shiftDate("2027-01-01", -1)).toBe("2026-12-31");
  });

  it("handles a leap day", () => {
    expect(shiftDate("2028-02-28", 1)).toBe("2028-02-29");
    expect(shiftDate("2028-02-29", 1)).toBe("2028-03-01");
    // 2027 不是闰年
    expect(shiftDate("2027-02-28", 1)).toBe("2027-03-01");
  });

  it("is a no-op for zero days", () => {
    expect(shiftDate("2026-10-01", 0)).toBe("2026-10-01");
  });
});

describe("todayString", () => {
  it("zero-pads month and day", () => {
    expect(todayString(new Date(2026, 0, 5))).toBe("2026-01-05");
    expect(todayString(new Date(2026, 11, 31))).toBe("2026-12-31");
  });
});

describe("colorFor", () => {
  it("gives a distinct color per event type", () => {
    const types = [
      "window_focus",
      "window_title_change",
      "system_idle",
      "system_resume",
      "session_lock",
      "session_unlock",
      "input_heartbeat",
    ];
    const colors = types.map(colorFor);
    expect(new Set(colors).size).toBe(types.length);
  });

  it("falls back for unknown types instead of returning undefined", () => {
    expect(colorFor("brand_new_type")).toBe("#bdbdbd");
  });
});

describe("parsePayload", () => {
  it("parses valid JSON", () => {
    expect(parsePayload<{ a: number }>('{"a":1}')).toEqual({ a: 1 });
  });

  it("returns null for malformed JSON instead of throwing", () => {
    // 一条坏 Event 不该让整页白屏
    expect(parsePayload("{not json")).toBeNull();
    expect(parsePayload("")).toBeNull();
  });
});
