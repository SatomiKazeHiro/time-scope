import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import ConfigPanel from "./ConfigPanel";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

const CFG = {
  idleThresholdS: 300,
  gracePeriodS: 60,
  minSegmentDurationS: 30,
  heartbeatEveryS: 10,
  autostart: false,
  closeBehavior: "ask",
  eventsRetentionDays: 365,
};

function stub(cfg: unknown = CFG, needsRestart = true) {
  invoke.mockImplementation((cmd: string) => {
    if (cmd === "get_config") return Promise.resolve(cfg);
    if (cmd === "set_config") return Promise.resolve(needsRestart);
    return Promise.resolve(null);
  });
}

beforeEach(() => {
  invoke.mockReset();
  stub();
});

describe("ConfigPanel", () => {
  it("把配置读出来显示", async () => {
    render(<ConfigPanel />);
    await waitFor(() => expect(screen.getByDisplayValue("300")).toBeTruthy());
  });

  it("要重启才生效的项旁边写明，不让用户对着没反应的控件猜", async () => {
    render(<ConfigPanel />);
    await waitFor(() => expect(screen.getByDisplayValue("300")).toBeTruthy());

    // 这四项绑在采集线程与 EngineConfig 上，改完进程内改不了。
    // 不标的话用户会以为界面坏了 —— 这是「读数就该长得像读数」的延伸。
    // 只数 `<dt>` 里的角标：顶部那段说明文字里也有「重启后生效」四个字。
    const marks = Array.from(document.querySelectorAll("dt")).filter((dt) =>
      dt.textContent?.includes("重启后生效"),
    );
    expect(marks.length).toBe(4);
  });

  it("热生效的项不标重启 —— 标了就成了假警告", async () => {
    render(<ConfigPanel />);
    await waitFor(() => expect(screen.getByDisplayValue("365")).toBeTruthy());

    // 保留期 / 自启 / 关窗行为是现读现用的，标「重启后生效」是骗人
    const marked = Array.from(document.querySelectorAll("dt"))
      .filter((dt) => dt.textContent?.includes("重启后生效"))
      .map((dt) => dt.textContent ?? "");
    expect(marked.some((t) => t.includes("事件保留天数"))).toBe(false);
    expect(marked.some((t) => t.includes("关窗"))).toBe(false);
  });

  it("改一个要重启的项后弹提示，而不是静默保存", async () => {
    render(<ConfigPanel />);
    await waitFor(() => expect(screen.getByDisplayValue("300")).toBeTruthy());

    const input = screen.getByLabelText(/无输入多少秒/);
    fireEvent.change(input, { target: { value: "120" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    // 提示条是 role=status，和字段旁的角标不是同一个东西
    await waitFor(() => expect(screen.getByRole("status")).toBeTruthy());
    expect(screen.getByRole("status").textContent).toContain("退出再启动");
    const call = invoke.mock.calls.find((c) => c[0] === "set_config");
    expect(call?.[1]).toEqual({ key: "idle_threshold_s", value: "120" });
  });

  it("改热生效的项不弹重启提示", async () => {
    stub(CFG, false);
    render(<ConfigPanel />);
    await waitFor(() => expect(screen.getByDisplayValue("365")).toBeTruthy());

    const input = screen.getByLabelText(/事件保留天数/);
    fireEvent.change(input, { target: { value: "90" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    // 后端说不用重启就不显示那句 —— 提示来自后端的事实，不是前端猜的
    await waitFor(() => expect(screen.getByRole("status")).toBeTruthy());
    expect(screen.getByRole("status").textContent).toContain("立刻生效");
    expect(screen.getByRole("status").textContent).not.toContain("退出再启动");
  });

  it("写回失败时说清原因，并保留用户刚填的值", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "get_config") return Promise.resolve(CFG);
      if (cmd === "set_config")
        return Promise.reject("config.toml 已是无效 TOML，请手工修好");
      return Promise.resolve(null);
    });
    render(<ConfigPanel />);
    await waitFor(() => expect(screen.getByDisplayValue("300")).toBeTruthy());

    const input = screen.getByLabelText(/无输入多少秒/);
    fireEvent.change(input, { target: { value: "120" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(screen.getByText(/已是无效 TOML/)).toBeTruthy());
    // 失败就把值退回磁盘上的 —— 否则用户以为存上了，下次进来又是旧值
    await waitFor(() => expect(screen.getByDisplayValue("300")).toBeTruthy());
  });

  it("读取失败时给提示，不留空白面板", async () => {
    invoke.mockImplementation((cmd: string) =>
      cmd === "get_config" ? Promise.reject("读不到 config.toml") : Promise.resolve(null),
    );
    render(<ConfigPanel />);
    await waitFor(() => expect(screen.getByText(/读不到 config\.toml/)).toBeTruthy());
  });

  it("每一项都有一句说明它管什么", async () => {
    render(<ConfigPanel />);
    await waitFor(() => expect(screen.getByDisplayValue("300")).toBeTruthy());

    // 数字框没有标签就只是一串数字，用户不知道调它会发生什么
    expect(screen.getByLabelText(/无输入多少秒/)).toBeTruthy();
    expect(screen.getByLabelText(/事件保留天数/)).toBeTruthy();
    expect(screen.getByLabelText(/关窗时/)).toBeTruthy();
  });
});