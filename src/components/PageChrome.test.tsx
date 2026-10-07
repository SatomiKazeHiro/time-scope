import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import {
  HeaderDivider,
  PageAlert,
  PageHeader,
  RangeReadout,
} from "./PageChrome";

describe("PageChrome", () => {
  it("页头写的是**当前页名**，不是应用名", () => {
    // 应用名在托盘和窗口标题栏已经常驻，左上角留给「你在哪一页」更有用
    render(<PageHeader title="监控采集">其余控件</PageHeader>);
    const h1 = screen.getByRole("heading", { level: 1 });
    expect(h1.textContent).toBe("监控采集");
    expect(h1.textContent).not.toMatch(/Time Scope|时迹/);
    // 窄窗口下没有它，中文会被逐字折成竖排
    expect(h1.className).toContain("whitespace-nowrap");
  });

  it("页名由各页传入，不写死在组件里", () => {
    const { unmount } = render(<PageHeader title="汇总" />);
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("汇总");
    unmount();
  });

  it("页头能换行 —— 窄窗口下溢出比换行更糟", () => {
    render(<PageHeader title="监控采集">其余控件</PageHeader>);
    expect(screen.getByRole("heading", { level: 1 }).parentElement!.className).toContain(
      "flex-wrap",
    );
  });

  it("分隔线是装饰，藏起来", () => {
    const { container } = render(<HeaderDivider />);
    const el = container.firstElementChild as HTMLElement;
    expect(el.getAttribute("aria-hidden")).toBe("true");
  });

  it("范围读数不是控件", () => {
    // 曾经是个 <button aria-disabled>，带着边框底色，和旁边真能点的按钮同款，
    // 长得能点却点不动。读数就该长得像读数。
    render(<RangeReadout label="近一年" />);
    const chip = screen.getByTestId("range-chip");
    expect(chip.tagName).not.toBe("BUTTON");
    expect(chip.className).not.toMatch(/border|bg-surface|cursor-/);
    expect(chip.textContent).toContain("近一年");
  });

  it("错误提示带警告图标 —— 之前只有监控页有，汇总页漏了", () => {
    const { container } = render(<PageAlert />);
    expect(screen.getByRole("alert")).toBeTruthy();
    // CircleAlert 渲染成 svg
    expect(container.querySelector("svg")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain("加载失败");
  });
});