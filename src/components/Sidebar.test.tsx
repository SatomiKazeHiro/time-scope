import { describe, it, expect, beforeEach } from "vitest";
import { useState } from "react";
import { render, screen, fireEvent } from "@testing-library/react";
import Sidebar, { type View } from "./Sidebar";
import SettingsPage from "./SettingsPage";
import { THEME_LABEL } from "../design/useTheme";

/** 真状态版本：点哪边 `view` 就变哪边，跟 App 里用的是一个结构。 */
function Harness() {
  const [view, setView] = useState<View>("monitor");
  return (
    <>
      <Sidebar view={view} onViewChange={setView} />
      {view === "settings" ? (
        <SettingsPage />
      ) : view === "summary" ? (
        <p>汇总页正文</p>
      ) : (
        <p>页面正文</p>
      )}
    </>
  );
}

beforeEach(() => {
  localStorage.clear();
  delete document.documentElement.dataset.theme;
});

describe("Sidebar", () => {
  it("上下两层：上层两个导航，下层两个工具", () => {
    const { container } = render(<Harness />);
    const nav = container.querySelector("nav")!;
    expect(nav.getAttribute("aria-label")).toBe("主导航");
    expect(screen.getAllByRole("button")).toHaveLength(4);
  });

  it("汇总排在监控采集上面", () => {
    render(<Harness />);
    const labels = screen
      .getAllByRole("button")
      .map((b) => b.getAttribute("aria-label"));
    expect(labels.indexOf("汇总")).toBeGreaterThanOrEqual(0);
    expect(labels.indexOf("汇总")).toBeLessThan(labels.indexOf("监控采集"));
  });

  it("点汇总切到 summary 视图", () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "汇总" }));
    expect(screen.getByText("汇总页正文")).toBeTruthy();
  });

  it("按钮本身只有图标，文字在悬浮提示里", () => {
    render(<Harness />);
    const btn = screen.getByRole("button", { name: "监控采集" });
    // 提示挂在按钮内、left-full 伸到栏外右侧
    const tip = btn.querySelector('span[role="tooltip"]')!;
    expect(tip.textContent).toBe("监控采集");
    expect(tip.className).toContain("left-full");
    // 提示不可接收指针，否则它盖在内容上时会挡住后面的点击
    expect(tip.className).toContain("pointer-events-none");
  });

  it("侧边栏自身不能裁掉悬浮提示", () => {
    // 提示伸到栏外，任何祖先的 overflow 都会把它切掉一半
    const { container } = render(<Harness />);
    expect(container.querySelector("nav")!.className).not.toMatch(/overflow-(hidden|auto|scroll)/);
  });

  it("点设置切到设置页，再点监控采集切回来", () => {
    // section 的 role 是 region，和侧边栏那个 button 区分得开
    const settingsPage = () => screen.queryByRole("region", { name: "设置" });
    render(<Harness />);
    expect(screen.getByText("页面正文")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    expect(settingsPage()).toBeTruthy();
    expect(screen.queryByText("页面正文")).toBeNull();
    expect(screen.getByRole("button", { name: "设置" }).getAttribute("aria-current")).toBe("page");

    fireEvent.click(screen.getByRole("button", { name: "监控采集" }));
    expect(screen.getByRole("button", { name: "监控采集" }).getAttribute("aria-current")).toBe(
      "page",
    );
    expect(screen.getByText("页面正文")).toBeTruthy();
    expect(settingsPage()).toBeNull();
  });

  it("工具项没有当前页语义", () => {
    render(<Harness />);
    // 主题是个动作不是目的地，标 aria-current="page" 会误导读屏
    expect(screen.getByRole("button", { name: /^主题：/ }).getAttribute("aria-current")).toBeNull();
  });

  it("主题按钮点一下换一态，提示跟着变", () => {
    render(<Harness />);
    const first = THEME_LABEL.system;
    expect(screen.getByRole("button", { name: `主题：${first}` })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: `主题：${first}` }));
    const second = THEME_LABEL.light;
    expect(screen.getByRole("button", { name: `主题：${second}` })).toBeTruthy();
  });
});
