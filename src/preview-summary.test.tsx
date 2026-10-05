/**
 * 视觉检查用：把汇总页渲染成静态 HTML，供 headless Chrome 截图。
 *
 * 不是测试，也不进 CI —— 它存在的理由是**校验器只管颜色不管排版**：
 * 标签碰撞、几何溢出、面板比例这些问题必须用眼睛看。
 *
 * 跑法：pnpm build && pnpm vitest run src/preview-summary.test.tsx
 * 产物：.preview/summary.html（自行用 headless Chrome 打开截图）
 */
import { describe, it, vi, beforeAll } from "vitest";
import { render, waitFor, fireEvent } from "@testing-library/react";
import { writeFileSync, mkdirSync, readdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import SummaryPage from "./views/SummaryPage";
import type { DailyCalendar, MergedTitle, Summary } from "./types";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

const CSS_BUNDLE = existsSync("dist/assets")
  ? readdirSync("dist/assets").find((f: string) => f.endsWith(".css"))
  : undefined;

/**
 * 造**最坏情况**的数据（STATUS §4.6：用 mock 数据验收 UI 的教训）——
 * 5 个月、碎片为主、含 8 天完全没活动、含一整段连续空白。
 *
 * 用干净数据是看不出来的：14 个整块的漂亮数据在真实场景里不可读。
 */
function worstCase(): DailyCalendar {
  const r = (() => {
    let s = 20261005;
    return () => ((s = (s * 1664525 + 1013904223) % 4294967296), s / 4294967296);
  })();

  const first = new Date(2026, 4, 18);   // 5-18，周一
  const last = new Date(2026, 9, 4);    // 10-04
  const days = [];
  const d = new Date(first);
  while (d <= last) {
    const iso = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
    // 8 天完全没活动（出差 / 忘了开），另有几天只开了一小会儿
    const skip = r() < 0.055;
    const workday = d.getDay() >= 1 && d.getDay() <= 5;
    const hours = skip ? 0 : workday ? 7 + r() * 7 : 1.5 + r() * 3.5;
    days.push({ date: iso, totalMs: Math.round(hours * 3_600_000) });
    d.setDate(d.getDate() + 1);
  }
  return { first: days[0].date, last: days[days.length - 1].date, days };
}

const CAL = worstCase();

const TITLES: MergedTitle[] = [
  { title: "无标题", hits: 7086, redacted: false },
  { title: "New Tab", hits: 3722, redacted: false },
  { title: "项目与 uv Python 管理", hits: 2074, redacted: false },
  { title: "DeepSeek 开放平台", hits: 1287, redacted: false },
  { title: "客户 42 号项目 - 交接文档 - Visual Studio Code", hits: 1205, redacted: true },
  { title: "了解该项目概况 - DeepSeek Harness", hits: 1213, redacted: false },
  { title: "⏳ 待处理 · 了解该项目概况 - DeepSeek Harness", hits: 1046, redacted: false },
  { title: "Kimi Code - 搭载 Kimi K3 的 AI 编程 Agent 与 CLI 工具", hits: 982, redacted: false },
  { title: "registry.ts - time-scope - Visual Studio Code", hits: 744, redacted: false },
  { title: "2026 年度个人所得税专项附加扣除填报 - 国家税务总局", hits: 402, redacted: false },
];

function summaryFor(): Summary {
  // 真的按 CAL 逐日累加，而不是拍几个数 —— 色阶和圆环要反映这批数据的形状
  let total = 0;
  const hourly = new Array(24).fill(0);
  for (const day of CAL.days) {
    total += day.totalMs;
    const startHour = 9 + Math.floor((day.totalMs / 3_600_000) % 6);
    for (let i = 0; i < Math.round(day.totalMs / 3_600_000); i++) {
      hourly[(startHour + i) % 24] += 3_600_000;
    }
  }
  const work = Math.round(total * 0.105);
  const browse = Math.round(total * 0.374);
  const idle = Math.round(total * 0.139);
  return {
    totalMs: total,
    activeMs: total - idle,
    idleMs: idle,
    segmentCount: 458,
    switchCount: 12,
    hourlyMs: hourly,
    donut: [
      { key: "work", ms: work },
      { key: "browsing", ms: browse },
      { key: "idle", ms: idle },
      { key: "unknown", ms: total - work - browse - idle },
    ],
    topApps: [
      { name: "msedge.exe", ms: 87_200_000 },
      { name: "WindowsTerminal.exe", ms: 67_200_000 },
      { name: "explorer.exe", ms: 32_700_000 },
      { name: "Code.exe", ms: 20_500_000 },
      { name: "msedgewebview2.exe", ms: 9_600_000 },
    ],
  };
}

const SUM = summaryFor();

beforeAll(() => {
  invoke.mockImplementation((cmd: string) => {
    if (cmd === "get_daily_calendar") return Promise.resolve(CAL);
    if (cmd === "get_summary") return Promise.resolve(SUM);
    if (cmd === "get_top_titles") return Promise.resolve(TITLES);
    return Promise.resolve(null);
  });
});

describe("summary preview", () => {
  it.skipIf(!CSS_BUNDLE)("dumps the summary page to .preview/ for both themes", async () => {
    mkdirSync(".preview", { recursive: true });

    const { container, unmount } = render(<SummaryPage />);
    // 等热力图真的画出来
    await waitFor(() =>
      expect(container.querySelectorAll("[data-date]").length).toBeGreaterThan(0),
    );
    // 选中一格，让框进画面
    const cell = container.querySelector("[data-date='2026-09-15']");
    if (cell) fireEvent.click(cell);
    await waitFor(() => expect(container.querySelector("[data-frame]")).toBeTruthy());

    for (const theme of ["dark", "light"]) {
      const html = `<!doctype html>
<html lang="zh-CN" data-theme="${theme}"><head><meta charset="utf-8">
<link rel="stylesheet" href="../dist/assets/${CSS_BUNDLE}">
<style>html,body{margin:0;width:1200px}</style>
</head><body><div class="flex min-h-screen bg-surface-0 p-4">${container.innerHTML}</div></body></html>`;
      writeFileSync(join(".preview", `summary-${theme}.html`), html);
    }
    unmount();
  });
});
