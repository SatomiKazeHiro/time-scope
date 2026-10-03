import { describe, it, expect, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import ThemeToggle from "./ThemeToggle";
import { useTheme } from "../design/useTheme";

const KEY = "time-scope.theme";

/** 把 hook 套一层空壳组件，才能在测试里点按钮触发状态变化。 */
function Harness() {
  const { theme, cycle } = useTheme();
  return <ThemeToggle theme={theme} onCycle={cycle} />;
}

function currentTheme(): string {
  return document.documentElement.dataset.theme ?? "";
}

beforeEach(() => {
  localStorage.clear();
  delete document.documentElement.dataset.theme;
});

describe("主题切换", () => {
  it("默认跟随系统", () => {
    render(<Harness />);
    expect(currentTheme()).toBe("system");
    expect(screen.getByRole("button").getAttribute("aria-label")).toMatch(/跟随系统/);
  });

  it("点一次切到下一个，循环回系统", () => {
    render(<Harness />);
    const btn = screen.getByRole("button");

    fireEvent.click(btn);
    expect(currentTheme()).toBe("light");
    fireEvent.click(btn);
    expect(currentTheme()).toBe("dark");
    fireEvent.click(btn);
    expect(currentTheme()).toBe("system");
  });

  it("记住选择，下次启动直接恢复", () => {
    const first = render(<Harness />);
    fireEvent.click(screen.getByRole("button")); // system → light
    fireEvent.click(screen.getByRole("button")); // light → dark
    expect(localStorage.getItem(KEY)).toBe("dark");
    first.unmount();

    render(<Harness />);
    expect(currentTheme()).toBe("dark");
  });

  it("存了非法值时退回跟随系统，而不是渲染出一个坏主题", () => {
    localStorage.setItem(KEY, "chartreuse");
    render(<Harness />);
    expect(currentTheme()).toBe("system");
  });

  it("可访问名描述当前状态，并说明点击会切换", () => {
    render(<Harness />);
    const btn = screen.getByRole("button");
    expect(btn.getAttribute("aria-label")).toBe("主题：跟随系统，点击切换");
    fireEvent.click(btn);
    expect(screen.getByRole("button").getAttribute("aria-label")).toBe("主题：浅色，点击切换");
  });
});
