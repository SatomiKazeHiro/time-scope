import { describe, it, expect } from "vitest";
import { render, screen, within } from "@testing-library/react";
import DaySummary from "./DaySummary";
import GranularityPicker from "./GranularityPicker";
import type { Segment } from "../types";

const HOUR = 3_600_000;

function seg(id: string, start: number, end: number, category: Segment["category"]): Segment {
  return {
    id, startAt: start, endAt: end, category,
    application: "Code.exe", confidence: 0.9,
    classifier: "rule", classifierVersion: "rules:15", evidenceEventIds: ["e1"],
  };
}

describe("DaySummary", () => {
  it("renders one row per category with its duration", () => {
    render(
      <DaySummary
        segments={[seg("a", 0, HOUR, "work"), seg("b", HOUR, HOUR * 2, "browsing")]}
        dayStartMs={0}
      />,
    );
    const rows = screen.getAllByRole("listitem");
    expect(rows.length).toBe(2);
    // 界面上显示中文类名
    expect(screen.getByText("工作")).toBeTruthy();
    expect(screen.getByText("浏览")).toBeTruthy();
    // 两行都是 1 小时，所以用 getAllByText
    expect(screen.getAllByText("1 时").length).toBe(2);
  });

  it("keeps the raw category key reachable so rules.toml stays cross-referenceable", () => {
    // 中文名是给人看的，原始 key 是拿去和 rules.toml 的规则对账的 —— 两个都要在。
    render(<DaySummary segments={[seg("a", 0, HOUR, "work")]} dayStartMs={0} />);
    expect(screen.getByTitle("work")).toBeTruthy();
  });

  it("renders nothing when there is no activity", () => {
    const { container } = render(<DaySummary segments={[]} dayStartMs={0} />);
    expect(container.querySelector("section")).toBeNull();
  });

  it("labels the section for screen readers", () => {
    render(<DaySummary segments={[seg("a", 0, HOUR, "work")]} dayStartMs={0} />);
    expect(screen.getByLabelText("当日汇总")).toBeTruthy();
  });
});

describe("DaySummary 活跃/空闲比（spec §10）", () => {
  // 同一个百分比在「活跃/空闲比」和下方「分类占比列」里都会出现，
  // 所以按 aria-label 限定到比值块里查。
  const ratio = () => screen.getByLabelText("活跃与空闲占比");

  it("splits the day into active and idle", () => {
    render(
      <DaySummary
        segments={[
          seg("a", 0, 6 * HOUR, "work"),
          seg("b", 6 * HOUR, 8 * HOUR, "idle"),
        ]}
        dayStartMs={0}
      />,
    );
    expect(within(ratio()).getByText("75%")).toBeTruthy();
    expect(within(ratio()).getByText("活跃 6 时 · 空闲 2 时")).toBeTruthy();
  });

  it("counts unknown as active, not idle", () => {
    // unknown 是「没分类出是什么」，不是「没在做事」。
    // 把它算成空闲会低估人实际在用电脑的时间。
    render(
      <DaySummary
        segments={[
          seg("a", 0, 3 * HOUR, "unknown"),
          seg("b", 3 * HOUR, 4 * HOUR, "idle"),
        ]}
        dayStartMs={0}
      />,
    );
    expect(within(ratio()).getByText("75%")).toBeTruthy();
  });

  it("reports 100% rather than NaN on a day with no idle at all", () => {
    render(<DaySummary segments={[seg("a", 0, HOUR, "work")]} dayStartMs={0} />);
    expect(within(ratio()).getByText("100%")).toBeTruthy();
    expect(within(ratio()).getByText("活跃 1 时 · 空闲 0 分")).toBeTruthy();
  });

  it("reports 0% on a day that is entirely idle", () => {
    render(<DaySummary segments={[seg("a", 0, 4 * HOUR, "idle")]} dayStartMs={0} />);
    expect(within(ratio()).getByText("0%")).toBeTruthy();
  });
});

describe("GranularityPicker", () => {
  it("offers exactly the four granularities from the spec", () => {
    render(<GranularityPicker value={30} onChange={() => {}} />);
    const buttons = screen.getAllByRole("button");
    expect(buttons.map((b) => b.textContent)).toEqual(["10分", "30分", "60分", "120分"]);
  });

  it("marks the active granularity as pressed", () => {
    render(<GranularityPicker value={60} onChange={() => {}} />);
    const pressed = screen.getAllByRole("button").filter(
      (b) => b.getAttribute("aria-pressed") === "true",
    );
    expect(pressed.length).toBe(1);
    expect(pressed[0].textContent).toBe("60分");
  });

  it("reports the picked granularity", () => {
    let picked: number | null = null;
    render(
      <GranularityPicker
        value={30}
        onChange={(m) => { picked = m; }}
      />,
    );
    screen.getByText("120分").click();
    expect(picked).toBe(120);
  });
});
