import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import RankedList from "./RankedList";

const ITEMS = [
  { label: "无标题", value: "7086", ratio: 100 },
  { label: "New Tab", value: "3722", ratio: 53 },
];

describe("RankedList", () => {
  it("按给定顺序渲染，不自己重排", () => {
    render(<RankedList title="窗口标题 Top" items={ITEMS} emptyHint="没有数据" />);
    const items = screen.getAllByRole("listitem");
    expect(items[0].textContent).toContain("无标题");
    expect(items[1].textContent).toContain("New Tab");
  });

  it("空列表显示提示而不是空白面板", () => {
    // Review Focus #5 的反面：空是因为还没数据，不该是一块什么都没有的面板
    render(<RankedList title="窗口标题 Top" items={[]} emptyHint="没有数据" />);
    expect(screen.getByText("没有数据")).toBeTruthy();
    expect(screen.queryAllByRole("listitem")).toHaveLength(0);
  });

  it("note 渲染在标签后面", () => {
    render(
      <RankedList
        title="窗口标题 Top"
        items={[{ label: "客户 42 - [redacted] - Code", value: "120", ratio: 100, note: "已脱敏" }]}
        emptyHint="没有数据"
      />,
    );
    expect(screen.getByText("已脱敏")).toBeTruthy();
  });

  it("ratio 决定条的长度", () => {
    const { container } = render(
      <RankedList title="应用 Top" items={ITEMS} emptyHint="没有数据" />,
    );
    const bars = container.querySelectorAll("[data-bar]");
    expect(bars).toHaveLength(2);
    expect((bars[0] as HTMLElement).style.width).toBe("100%");
    expect((bars[1] as HTMLElement).style.width).toBe("53%");
  });

  it("超长标签截断而不是撑破面板", () => {
    const { container } = render(
      <RankedList
        title="窗口标题 Top"
        items={[{ label: "x".repeat(300), value: "1", ratio: 10 }]}
        emptyHint="没有数据"
      />,
    );
    const label = container.querySelector(".truncate");
    expect(label).toBeTruthy();
    expect(label!.getAttribute("title")).toHaveLength(300);
  });

  it("标题单独一行", () => {
    render(<RankedList title="应用 Top" items={ITEMS} emptyHint="没有数据" />);
    expect(screen.getByRole("heading", { name: "应用 Top" })).toBeTruthy();
  });
});
