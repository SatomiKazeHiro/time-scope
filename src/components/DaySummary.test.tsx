import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
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
    expect(screen.getByText("work")).toBeTruthy();
    expect(screen.getByText("browsing")).toBeTruthy();
    // 两行都是 1 小时，所以用 getAllByText
    expect(screen.getAllByText("1 时").length).toBe(2);
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
