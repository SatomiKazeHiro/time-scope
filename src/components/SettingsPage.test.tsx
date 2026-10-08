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

/** 默认桩：返回两行未覆盖数据。 */
function stub() {
  invoke.mockImplementation((cmd: string) => {
    if (cmd === "get_uncovered_programs") return Promise.resolve(ROWS);
    if (cmd === "add_rule") return Promise.resolve(null);
    return Promise.resolve(null);
  });
}

beforeEach(() => {
  invoke.mockReset();
  stub();
});

describe("SettingsPage · 规则覆盖", () => {
  it("列出没被规则覆盖的程序，按耗时从多到少", async () => {
    render(<SettingsPage />);

    await waitFor(() => expect(screen.getByText("DBeaver.exe")).toBeTruthy());
    const names = screen.getAllByRole("listitem").map((li) => li.textContent ?? "");
    expect(names[0]).toContain("DBeaver.exe");
    expect(names[1]).toContain("Zed.exe");
  });

  it("没列全时说明统计窗口，不让用户以为是全部历史", async () => {
    render(<SettingsPage />);
    await waitFor(() => expect(screen.getByText("DBeaver.exe")).toBeTruthy());
    // 「最近 30 天」是这个数字的分母 —— 不写出来，38% 就不知道是谁的 38%
    expect(screen.getByText(/最近 30 天/)).toBeTruthy();
  });

  it("全部覆盖时给出好消息的空状态，而不是空白", async () => {
    invoke.mockImplementation((cmd: string) =>
      cmd === "get_uncovered_programs" ? Promise.resolve([]) : Promise.resolve(null),
    );
    render(<SettingsPage />);

    // 「都覆盖了」是用户想看到的那句话；空白会被读成「还没加载出来」
    await waitFor(() => expect(screen.getByText(/全部都有规则覆盖/)).toBeTruthy());
  });

  it("给程序选一个类别后写回 rules.toml", async () => {
    render(<SettingsPage />);
    await waitFor(() => expect(screen.getByText("DBeaver.exe")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: /DBeaver\.exe/ }));
    fireEvent.click(screen.getByRole("button", { name: "学习" }));

    await waitFor(() => {
      const call = invoke.mock.calls.find((c) => c[0] === "add_rule");
      expect(call).toBeTruthy();
      expect(call?.[1]).toEqual({ application: "DBeaver.exe", category: "study" });
    });
  });

  it("idle 与 unknown 不能被选为目标 —— 它们不是类别", async () => {
    render(<SettingsPage />);
    await waitFor(() => expect(screen.getByText("DBeaver.exe")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: /DBeaver\.exe/ }));

    // 后端也会挡，但界面不该先给出点不动的选项
    expect(screen.queryByRole("button", { name: "空闲" })).toBeNull();
    expect(screen.queryByRole("button", { name: "未分类" })).toBeNull();
    expect(screen.getByRole("button", { name: "工作" })).toBeTruthy();
  });

  it("写回失败时把原因说出来，并保留那一行让人重试", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "get_uncovered_programs") return Promise.resolve(ROWS);
      if (cmd === "add_rule")
        return Promise.reject("`DBeaver.exe` 已有一条规则（category = work）。");
      return Promise.resolve(null);
    });
    render(<SettingsPage />);
    await waitFor(() => expect(screen.getByText("DBeaver.exe")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: /DBeaver\.exe/ }));
    fireEvent.click(screen.getByRole("button", { name: "工作" }));

    // 失败必须可见：静默失败会让人以为配好了
    await waitFor(() => expect(screen.getByText(/已有一条规则/)).toBeTruthy());
  });

  it("读取失败时也给提示，不留空白面板", async () => {
    invoke.mockImplementation((cmd: string) =>
      cmd === "get_uncovered_programs"
        ? Promise.reject("读不到 rules.toml")
        : Promise.resolve(null),
    );
    render(<SettingsPage />);

    await waitFor(() => expect(screen.getByText(/读不到 rules\.toml/)).toBeTruthy());
  });
});