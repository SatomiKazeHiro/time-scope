import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import SettingsPage from "./SettingsPage";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

const ROWS = [
  {
    application: "DBeaver.exe",
    totalMs: 3 * 3600_000,
    shareOfUncovered: 0.6,
    segmentCount: 12,
  },
  { application: "Zed.exe", totalMs: 2 * 3600_000, shareOfUncovered: 0.4, segmentCount: 7 },
];

/**
 * 默认桩：返回两行未覆盖数据 + 一份完整配置。
 *
 * 配置也要给 —— 设置页同时挂了 ConfigPanel，它和规则区共用这一个 invoke 桩。
 * 缺了它 `get_config` 会拿到 null，ConfigPanel 里的 `toEditable(null)` 抛错，
 * 那个异常从共享的桩里冒出去，连规则区的加载一起带崩（症状是「DBeaver 找不到了」，
 * 与真正的病因隔着两层，很容易查错方向）。
 */
const CFG = {
  idleThresholdS: 300,
  gracePeriodS: 60,
  minSegmentDurationS: 30,
  heartbeatEveryS: 10,
  autostart: false,
  closeBehavior: "ask",
  eventsRetentionDays: 365,
  needsRestart: {
    idle_threshold_s: true,
    min_segment_duration_s: true,
    heartbeat_every_s: true,
    events_retention_days: false,
    autostart: true,
    close_behavior: false,
  },
};

function stub() {
  invoke.mockImplementation((cmd: string) => {
    if (cmd === "get_uncovered_programs") return Promise.resolve(ROWS);
    if (cmd === "get_config") return Promise.resolve(CFG);
    if (cmd === "add_rule") return Promise.resolve(null);
    if (cmd === "set_config") return Promise.resolve(true);
    return Promise.resolve(null);
  });
}

beforeEach(() => {
  invoke.mockReset();
  stub();
});

/**
 * 挂设置页并等两区都加载完。
 *
 * 设置页同时挂规则区与配置区，两边都在挂载时动态 import 同一个被 mock 的模块。
 * vitest 的 `vi.mock` 对**同时**发起的重复动态 import 会让第二次拿到 undefined
 * （症状是 `Cannot read properties of undefined (reading 'invoke')`，
 * 且表现得像「数据没加载出来」，与病因隔着好几层）。
 *
 * 这是测试桩的毛病，不是产品的：App 一次只挂一个视图（见 App.tsx 的三元），
 * 真实使用中不存在两个面板同时首次挂载的情形。
 * 这里把挂载拆成「先挂规则区 → 等它加载完 → 再挂配置区」，与真实时序一致。
 */
async function mountSettings() {
  const { rerender } = render(<SettingsPage rulesOnly />);
  await waitFor(() => expect(screen.getByText("DBeaver.exe")).toBeTruthy());
  rerender(<SettingsPage />);
  await waitFor(() => expect(screen.getByLabelText(/无输入多少秒/)).toBeTruthy());
}

describe("SettingsPage · 规则覆盖", () => {
  it("列出没被规则覆盖的程序，按耗时从多到少", async () => {
    await mountSettings();
    const names = screen.getAllByRole("listitem").map((li) => li.textContent ?? "");
    expect(names[0]).toContain("DBeaver.exe");
    expect(names[1]).toContain("Zed.exe");
  });

  it("没列全时说明统计窗口，不让用户以为是全部历史", async () => {
    await mountSettings();
    // 「最近 30 天」是这个数字的分母 —— 不写出来，38% 就不知道是谁的 38%
    expect(screen.getByText(/最近 30 天/)).toBeTruthy();
  });

  it("全部覆盖时给出好消息的空状态，而不是空白", async () => {
    invoke.mockImplementation((cmd: string) =>
      cmd === "get_uncovered_programs"
        ? Promise.resolve([])
        : cmd === "get_config"
          ? Promise.resolve(CFG)
          : Promise.resolve(null),
    );
    const { rerender } = render(<SettingsPage rulesOnly />);
    // 「都覆盖了」是用户想看到的那句话；空白会被读成「还没加载出来」
    await waitFor(() => expect(screen.getByText(/全部都有规则覆盖/)).toBeTruthy());
    rerender(<SettingsPage />);
  });

  it("给程序选一个类别后写回 rules.toml", async () => {
    await mountSettings();

    fireEvent.click(screen.getByRole("button", { name: /DBeaver\.exe/ }));
    fireEvent.click(screen.getByRole("button", { name: "学习" }));

    await waitFor(() => {
      const call = invoke.mock.calls.find((c) => c[0] === "add_rule");
      expect(call).toBeTruthy();
      expect(call?.[1]).toEqual({ application: "DBeaver.exe", category: "study" });
    });
  });

  it("idle 与 unknown 不能被选为目标 —— 它们不是类别", async () => {
    await mountSettings();
    fireEvent.click(screen.getByRole("button", { name: /DBeaver\.exe/ }));

    // 后端也会挡，但界面不该先给出点不动的选项
    expect(screen.queryByRole("button", { name: "空闲" })).toBeNull();
    expect(screen.queryByRole("button", { name: "未分类" })).toBeNull();
    expect(screen.getByRole("button", { name: "工作" })).toBeTruthy();
  });

  it("写回失败时把原因说出来，并保留那一行让人重试", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "get_uncovered_programs") return Promise.resolve(ROWS);
      if (cmd === "get_config") return Promise.resolve(CFG);
      if (cmd === "add_rule")
        return Promise.reject("`DBeaver.exe` 已有一条规则（category = work）。");
      return Promise.resolve(null);
    });
    await mountSettings();

    fireEvent.click(screen.getByRole("button", { name: /DBeaver\.exe/ }));
    fireEvent.click(screen.getByRole("button", { name: "工作" }));

    // 失败必须可见：静默失败会让人以为配好了
    await waitFor(() => expect(screen.getByText(/已有一条规则/)).toBeTruthy());
  });

  it("读取失败时也给提示，不留空白面板", async () => {
    invoke.mockImplementation((cmd: string) =>
      cmd === "get_uncovered_programs"
        ? Promise.reject("读不到 rules.toml")
        : cmd === "get_config"
          ? Promise.resolve(CFG)
          : Promise.resolve(null),
    );
    const { rerender } = render(<SettingsPage rulesOnly />);
    await waitFor(() => expect(screen.getByText(/读不到 rules\.toml/)).toBeTruthy());
    rerender(<SettingsPage />);
  });
});