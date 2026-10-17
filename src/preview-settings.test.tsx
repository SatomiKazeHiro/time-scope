/**
 * 设置页视觉预览 —— 两种主题各一张，外加「正在归类」展开态。
 *
 * 为什么单独一个文件：预览*.test.tsx 只覆盖监控页与汇总页，
 * 设置页改了却**没有截图可比**，就只能靠读 diff 猜排版对不对 ——
 * 而 STATUS §2 明说「校验器只管颜色不管排版」，标签碰撞、面板比例
 * 这类问题只有眼睛能看出来。补上这一份。
 *
 * 不是断言，是产物：跑完去 `.preview/` 看图。
 */
import { describe, it, vi, beforeEach } from "vitest";
import { render } from "@testing-library/react";
import { writeFileSync, mkdirSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import SettingsPage from "./components/SettingsPage";
import ConfigPanel from "./components/ConfigPanel";

const ROWS = [
  { application: "DBeaver.exe", totalMs: 5.2 * 3600_000, shareOfUncovered: 0.41, segmentCount: 23 },
  { application: "Zed.exe", totalMs: 3.1 * 3600_000, shareOfUncovered: 0.24, segmentCount: 14 },
  { application: "FinalShell.exe", totalMs: 2.4 * 3600_000, shareOfUncovered: 0.19, segmentCount: 31 },
  { application: "HBuilderX.exe", totalMs: 1.2 * 3600_000, shareOfUncovered: 0.1, segmentCount: 9 },
  { application: "obsidian.exe", totalMs: 0.8 * 3600_000, shareOfUncovered: 0.06, segmentCount: 6 },
];

const CFG = {
  idleThresholdS: 300,
  gracePeriodS: 60,
  minSegmentDurationS: 30,
  heartbeatEveryS: 10,
  eventsRetentionDays: 365,
  autostart: false,
  closeBehavior: "ask",
  needsRestart: {
    idle_threshold_s: true,
    min_segment_duration_s: true,
    heartbeat_every_s: true,
    events_retention_days: false,
    autostart: true,
    close_behavior: false,
  },
};

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

const OUT = ".preview";

/**
 * 预览要的是**产物**里的 CSS，不是源码 ——
 * Tailwind v4 的 token 与工具类要 `pnpm build` 之后才在 dist 里。
 * 与其余三个 preview 测试同款：找不到就空字符串（预览图没样式，
 * 总比整个测试文件失败强）。
 */
const CSS = (() => {
  try {
    const dir = "dist/assets";
    const f = readdirSync(dir).find((n) => n.endsWith(".css"));
    return f ? readFileSync(join(dir, f), "utf8") : "";
  } catch {
    return "";
  }
})();

function stub() {
  invoke.mockImplementation((cmd: string) => {
    if (cmd === "get_uncovered_programs") return Promise.resolve(ROWS);
    if (cmd === "get_config") return Promise.resolve(CFG);
    return Promise.resolve(null);
  });
}

beforeEach(() => {
  invoke.mockReset();
  stub();
  mkdirSync(OUT, { recursive: true });
});

function html(theme: string, body: string): string {
  return `<!doctype html><html lang="zh-CN" data-theme="${theme}"><head><meta charset="utf-8">
<style>${CSS}</style>
<style>body{margin:0;background:var(--color-surface-0);padding:16px;width:1168px;box-sizing:border-box}</style>
</head><body>${body}</body></html>`;
}

describe("设置页预览", () => {
  // **一次只挂一个面板**。两个面板同时挂载时都会动态 import 同一个被
  // mock 的模块，vitest 的 `vi.mock` 对**同时**发起的重复 import 会让
  // 第二个拿到 undefined —— 于是有一半内容渲染成错误态，四张图长得
  // 一模一样，看着像「页面坏了」。分开挂就避开它。
  //
  // 这是桩的问题不是产品的：真实构建里同一个模块由打包器缓存，
  // 第二次 import 拿的是同一个已解析的命名空间。

  for (const theme of ["dark", "light"]) {
    it(`${theme} · 规则覆盖`, async () => {
      const { container } = render(<SettingsPage rulesOnly />);
      await new Promise((r) => setTimeout(r, 400));
      writeFileSync(join(OUT, `settings-list-${theme}.html`), html(theme, container.innerHTML));
    });

    it(`${theme} · 行为参数`, async () => {
      const { container } = render(<ConfigPanel />);
      await new Promise((r) => setTimeout(r, 400));
      writeFileSync(join(OUT, `settings-config-${theme}.html`), html(theme, container.innerHTML));
    });
  }

  it("dark · 归类展开态（检查分段控件会不会撑破行）", async () => {
    const { container } = render(<SettingsPage rulesOnly />);
    await new Promise((r) => setTimeout(r, 400));
    const btn = container.querySelector<HTMLButtonElement>(
      'button[aria-label^="给 DBeaver"]',
    );
    btn?.click();
    await new Promise((r) => setTimeout(r, 200));
    writeFileSync(join(OUT, "settings-picker-dark.html"), html("dark", container.innerHTML));
  });

  it("dark · 全部覆盖的空状态", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "get_uncovered_programs") return Promise.resolve([]);
      if (cmd === "get_config") return Promise.resolve(CFG);
      return Promise.resolve(null);
    });
    const { container } = render(<SettingsPage rulesOnly />);
    await new Promise((r) => setTimeout(r, 400));
    writeFileSync(join(OUT, "settings-empty-dark.html"), html("dark", container.innerHTML));
  });

  it("light · 写回失败（错误条长什么样）", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "get_uncovered_programs") return Promise.resolve(ROWS);
      if (cmd === "get_config") return Promise.resolve(CFG);
      if (cmd === "set_config")
        return Promise.reject("config.toml 已是无效 TOML，请手工修好");
      return Promise.resolve(null);
    });
    const { container } = render(<ConfigPanel />);
    await new Promise((r) => setTimeout(r, 400));
    const input = container.querySelector<HTMLInputElement>(
      'input[aria-label*="空闲"]',
    );
    if (input) {
      const setter = Object.getOwnPropertyDescriptor(
        window.HTMLInputElement.prototype,
        "value",
      )?.set;
      setter?.call(input, "120");
      input.dispatchEvent(new Event("input", { bubbles: true }));
      await new Promise((r) => setTimeout(r, 100));
      container.querySelector<HTMLButtonElement>("button")?.click();
    }
    await new Promise((r) => setTimeout(r, 400));
    writeFileSync(join(OUT, "settings-error-light.html"), html("light", container.innerHTML));
  });
});