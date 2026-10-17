import { describe, it, expect } from "vitest";
import { clipToUncovered } from "./clip";

const span = (id: string, startAt: number, endAt: number) => ({ id, startAt, endAt });

describe("clipToUncovered", () => {
  it("重叠部分归给先开始的段", () => {
    const { kept } = clipToUncovered([span("a", 0, 2), span("b", 1, 3)]);
    expect(kept.map((k) => [k.seg.id, k.start, k.end])).toEqual([
      ["a", 0, 2],
      ["b", 2, 3],
    ]);
  });

  it("首尾相接不是重叠，两段都计满", () => {
    const { kept, hiddenCount, hiddenMs } = clipToUncovered([span("a", 0, 2), span("b", 2, 3)]);
    expect(kept).toHaveLength(2);
    expect(hiddenCount).toBe(0);
    expect(hiddenMs).toBe(0);
  });

  it("整段被盖住时算「藏起来」，并把量报出来", () => {
    const { kept, hiddenCount, hiddenMs } = clipToUncovered([span("outer", 0, 3), span("inner", 1, 2)]);
    expect(kept).toHaveLength(1);
    expect(hiddenCount).toBe(1);
    expect(hiddenMs).toBe(1);
  });

  it("入参无序也按开始时间裁", () => {
    const { kept } = clipToUncovered([span("late", 1, 3), span("early", 0, 2)]);
    expect(kept.map((k) => k.seg.id)).toEqual(["early", "late"]);
  });

  it("同一时刻开始的段按入参顺序裁（结果可预测，不靠排序实现细节）", () => {
    const { kept } = clipToUncovered([span("first", 0, 3), span("second", 0, 1)]);
    expect(kept.map((k) => k.seg.id)).toEqual(["first"]);
  });

  it("零长与倒挂的段不占时间，也不制造重叠", () => {
    const { kept, hiddenCount } = clipToUncovered([span("zzz", 5, 5), span("bad", 9, 4)]);
    expect(kept).toHaveLength(0);
    expect(hiddenCount).toBe(2);
  });
});
