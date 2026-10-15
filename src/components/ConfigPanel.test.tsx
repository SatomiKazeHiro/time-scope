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
  // 后端给的判定。角标读它，前端自己不再列一份。
  needsRestart: {
    idle_threshold_s: true,
    grace_period_s: true,
    min_segment_duration_s: true,
    heartbeat_every_s: true,
    events_retention_days: false,
    autostart: true,
    close_behavior: false,
  },
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

    // 五个：四个绑在采集线程/EngineConfig 上，加一个开机自启 ——
    // 它的托盘勾选态与系统注册表都是启动时定的，set_config 碰不到。
    // 不标的话用户会以为界面坏了 —— 这是「读数就该长得像读数」的延伸。
    const marks = Array.from(document.querySelectorAll("dt")).filter((dt) =>
      dt.textContent?.includes("重启后生效"),
    );
    expect(marks.length).toBe(5);
  });

  it("热生效的项不标重启 —— 标了就成了假警告", async () => {
    render(<ConfigPanel />);
    await waitFor(() => expect(screen.getByDisplayValue("365")).toBeTruthy());

    const marked = Array.from(document.querySelectorAll("dt"))
      .filter((dt) => dt.textContent?.includes("重启后生效"))
      .map((dt) => dt.textContent ?? "");
    // 保留期挂在 get_segments 上现读
    expect(marked.some((t) => t.includes("事件保留天数"))).toBe(false);
    // 关窗行为：关窗拦截现读 config.toml（residency::effective_close_behavior），
    // 设完下一次点 ✕ 就照新的走 —— 标「重启后生效」就成了假警告
    expect(marked.some((t) => t.includes("关窗"))).toBe(false);
    // 开机自启相反：Residency 的启动快照，改完不重启不生效
    expect(marked.some((t) => t.includes("开机自启"))).toBe(true);
  });

  it("角标读后端给的判定，不读前端自己那份表", async () => {
    // 2026-10-15 那次界面说谎的机制就是前端硬编码了一份 `restart`，
    // 与后端**一致地错**。这里反过来改后端的答案，角标必须跟着动 ——
    // 前端要是还留着自己的副本，这条会红。
    stub({ ...CFG, needsRestart: { ...CFG.needsRestart, close_behavior: true } });
    render(<ConfigPanel />);
    await waitFor(() => expect(screen.getByDisplayValue("365")).toBeTruthy());

    const marked = Array.from(document.querySelectorAll("dt"))
      .filter((dt) => dt.textContent?.includes("重启后生效"))
      .map((dt) => dt.textContent ?? "");
    expect(marked.some((t) => t.includes("关窗"))).toBe(true);
  });

  it("判定里缺一个键时按「要重启」算，不默默当成已生效", async () => {
    const { close_behavior: _dropped, ...rest } = CFG.needsRestart;
    void _dropped;
    stub({ ...CFG, needsRestart: rest });
    render(<ConfigPanel />);
    await waitFor(() => expect(screen.getByDisplayValue("365")).toBeTruthy());

    const marked = Array.from(document.querySelectorAll("dt"))
      .filter((dt) => dt.textContent?.includes("重启后生效"))
      .map((dt) => dt.textContent ?? "");
    expect(marked.some((t) => t.includes("关窗"))).toBe(true);
  });

  it("没改动时保存按钮是禁用的", async () => {
    render(<ConfigPanel />);
    await waitFor(() => expect(screen.getByDisplayValue("300")).toBeTruthy());

    // 草稿全部来自刚读回来的值，一个字没改就不该让人白按一次
    expect((screen.getByRole("button", { name: "保存" }) as HTMLButtonElement).disabled).toBe(
      true,
    );
  });

  it("只改一项就只写那一项，不顺手重写另外六行", async () => {
    render(<ConfigPanel />);
    await waitFor(() => expect(screen.getByDisplayValue("300")).toBeTruthy());

    fireEvent.change(screen.getByLabelText(/无输入多少秒/), { target: { value: "120" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => {
      const calls = invoke.mock.calls.filter((c) => c[0] === "set_config");
      expect(calls).toHaveLength(1);
      expect(calls[0][1]).toEqual({ key: "idle_threshold_s", value: "120" });
    });
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

  it("下拉框给中文，写回 config.toml 的仍是原值", async () => {
    render(<ConfigPanel />);
    await waitFor(() => expect(screen.getByDisplayValue("365")).toBeTruthy());

    // 界面上是给用户读的中文，磁盘上仍是 ask/minimize/quit —— 改回英文
    // 界面也「能用」，所以得钉住
    const select = screen.getByLabelText(/关窗时/) as HTMLSelectElement;
    expect(Array.from(select.options).map((o) => o.textContent)).toEqual([
      "每次都问",
      "最小化到托盘",
      "退出",
    ]);
    expect(Array.from(select.options).map((o) => o.value)).toEqual([
      "ask",
      "minimize",
      "quit",
    ]);
  });

  it("选中中文选项，写回的是 toml 里的键值", async () => {
    render(<ConfigPanel />);
    await waitFor(() => expect(screen.getByDisplayValue("365")).toBeTruthy());

    fireEvent.change(screen.getByLabelText(/关窗时/), { target: { value: "minimize" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => {
      const call = invoke.mock.calls.find((c) => c[1]?.key === "close_behavior");
      expect(call?.[1]).toEqual({ key: "close_behavior", value: "minimize" });
    });
  });
});