/**
 * 视觉检查用：把真实组件渲染成静态 HTML，供 headless Chrome 截图。
 *
 * 不是测试，也不进 CI —— 它存在的理由是校验器只管颜色不管排版，
 * 标签碰撞、几何溢出、面板比例这些问题必须用眼睛看。
 *
 * 跑法：pnpm vitest run src/preview.test.tsx
 * 产物：.preview/preview.html（需自行套上 dist 的 CSS 再截图）
 */
import { describe, it, vi, beforeAll } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { writeFileSync, mkdirSync, readdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import App from "./App";
import type { Segment } from "./types";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

/**
 * 这个文件会被 `pnpm test` 收进去，但 CI 上没有 dist —— 那时它是纯负担。
 * 显式跳过并说明原因，好过让 readdirSync 在流水线里抛错。
 * 本地要看效果：先 pnpm build，再 pnpm test。
 */
const CSS_BUNDLE = existsSync("dist/assets")
  ? readdirSync("dist/assets").find((f: string) => f.endsWith(".css"))
  : undefined;

/**
 * 造一天：碎片为主，夹杂长段专注 —— 真实采集就是这个形状。
 *
 * 之前用的是 14 个整块的干净数据，专注度/切换次数在那种数据下几乎全是满档，
 * 看不出这两个指标是干什么的。碎片数据才有真实的分布。
 */
function day(): Segment[] {
  const r = (() => {
    let s = 20261004;
    return () => ((s = (s * 1664525 + 1013904223) % 4294967296), s / 4294967296);
  })();
  const at = (min: number) => {
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    return d.getTime() + min * 60_000;
  };

  // 凌晨到早上是睡眠（长 idle），8:00 起床后碎片化，
  // 下午有两段长专注，中午午饭 + 通勤。
  const out: Segment[] = [];
  let n = 0;
  const push = (from: number, to: number, category: Segment["category"], application: string | null) => {
    out.push({
      id: `s${n++}`,
      startAt: at(from),
      endAt: at(to),
      category,
      application,
      confidence: 0.9,
      classifier: "rule",
      classifierVersion: "rules:15",
      evidenceEventIds: ["e1", "e2", "e3"],
    });
  };

  push(0, 7 * 60 + 40, "idle", null); // 睡眠
  // 起床后：碎片化的上午
  const apps: [Segment["category"], string][] = [
    ["work", "Code.exe"],
    ["browsing", "chrome.exe"],
    ["communication", "WeChat.exe"],
    ["work", "Code.exe"],
    ["life", "explorer.exe"],
  ];
  let t = 7 * 60 + 40;
  const morningEnd = 12 * 60;
  while (t < morningEnd - 6) {
    const dur = 2 + Math.floor(r() * 9);
    const end = Math.min(t + dur, morningEnd - 3);
    const [cat, app] = apps[Math.floor(r() * apps.length)];
    push(t, end, cat, app);
    t = end + (r() < 0.3 ? 4 : 1); // 有时留个小空档
  }
  push(t, 13 * 60 + 10, "life", "explorer.exe"); // 午饭

  // 下午：两段长专注 —— 这正是专注度模式要显出来的东西
  push(13 * 60 + 20, 15 * 60 + 40, "work", "Code.exe"); // 2h20m 连着做
  push(15 * 60 + 40, 16 * 60, "life", "explorer.exe"); // 歇一会
  push(16 * 60, 18 * 60 + 30, "work", "Code.exe"); // 又一段 2h30m

  // 傍晚：碎片化
  const evening: [Segment["category"], string][] = [
    ["browsing", "chrome.exe"],
    ["communication", "WeChat.exe"],
    ["entertainment", "bilibili.exe"],
    ["life", "explorer.exe"],
  ];
  let e = 18 * 60 + 30;
  while (e < 23 * 60) {
    const dur = 2 + Math.floor(r() * 8);
    const end = Math.min(e + dur, 23 * 60);
    const [cat, app] = evening[Math.floor(r() * evening.length)];
    push(e, end, cat, app);
    e = end + (r() < 0.35 ? 3 : 1);
  }
  push(23 * 60, 24 * 60, "idle", null);
  return out;
}

/** 造一屏很长的窗口标题：验证详情面板不会被撑开、标题区自己滚。 */
const MANY_TITLES = Array.from({ length: 24 }, (_, i) => ({
  title: `第 ${i + 1} 条 · ${"一段比较长的窗口标题内容".repeat(2)}_${i}.ts - 某个项目 - Visual Studio Code`,
  redacted: i % 5 === 0,
  count: (i % 7) + 1,
}));

beforeAll(() => {
  invoke.mockImplementation((cmd: string) =>
    cmd === "get_segment_titles" ? Promise.resolve(MANY_TITLES) : Promise.resolve(day()),
  );
});

describe("preview", () => {
  it.skipIf(!CSS_BUNDLE)("dumps the real UI to .preview/ for both themes and all 3 metrics", async () => {
    mkdirSync(".preview", { recursive: true });

    for (const metric of ["category", "focus", "switch"] as const) {
      const { container, unmount } = render(<App />);
      await waitFor(() =>
        expect(container.querySelectorAll("svg[role='img']").length).toBeGreaterThan(0),
      );

      // 切到要看的指标模式
      if (metric !== "category") {
        fireEvent.click(screen.getByRole("button", { name: metric === "focus" ? "专注度" : "切换次数" }));
      }

      // 选中一段，让详情面板也进画面
      const svg = screen.getByRole("img", { name: "24h 活动时间线" });
      fireEvent.click(svg.querySelectorAll("rect")[12] || svg.querySelectorAll("rect")[0]);
      // 等标题真的加载出来，否则 dump 出来的是「读取中…」，截不出滚动效果
      await waitFor(() => expect(screen.getByText(/条 · 可滚动/)).toBeTruthy());

      // 悬浮提示是 hover 态，静态页面出不来。侧边栏那条伸到栏外右侧，
      // 会被祖先的 overflow 裁掉 —— 所以把「设置」那条强制点亮，截图验它没被切。
      const settings = container.querySelector('button[aria-label="设置"]');
      const tip = settings?.querySelector('span[role="tooltip"]') as HTMLElement | null;
      if (tip) {
        tip.classList.remove("opacity-0");
        tip.classList.add("opacity-100");
      }

      for (const theme of ["dark", "light"]) {
        // data-theme 落在 <html> 上（useTheme 写的是 documentElement），
        // 静态页面里得手动带上，否则截出来两张都是默认的深色。
        const html = `<!doctype html>
<html lang="zh-CN" data-theme="${theme}"><head><meta charset="utf-8">
<link rel="stylesheet" href="../dist/assets/${CSS_BUNDLE}">
<style>html,body{margin:0}</style>
</head><body>${container.innerHTML}</body></html>`;
        writeFileSync(join(".preview", `preview-${metric}-${theme}.html`), html);
      }
      unmount();
    }
  });
});
