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

/** 造一天：早上写代码，中午学习，下午沟通，晚上娱乐，夹着空闲和长段未分类。 */
function day(): Segment[] {
  const at = (h: number, m = 0) => {
    const d = new Date();
    d.setHours(h, m, 0, 0);
    return d.getTime();
  };
  let n = 0;
  const s = (
    sh: number,
    sm: number,
    eh: number,
    em: number,
    category: Segment["category"],
    application: string | null,
  ): Segment => ({
    id: `s${n++}`,
    startAt: at(sh, sm),
    endAt: at(eh, em),
    category,
    application,
    confidence: 0.9,
    classifier: "rule",
    classifierVersion: "rules:15",
    evidenceEventIds: ["e1", "e2", "e3"],
  });

  return [
    s(0, 0, 7, 20, "idle", null), // 睡觉：长段空闲，最考验中性色够不够退后
    s(7, 20, 7, 45, "life", " explorer.exe"),
    s(7, 45, 9, 0, "work", "Code.exe"), // 整段 work：段内直标要打得下
    s(9, 0, 9, 4, "communication", "WeChat.exe"),
    s(9, 4, 12, 30, "work", "Code.exe"),
    s(12, 30, 13, 30, "life", " explorer.exe"),
    s(13, 30, 17, 0, "work", "Code.exe"),
    s(17, 0, 18, 30, "browsing", " chrome.exe"), // browsing
    s(18, 30, 20, 0, "entertainment", " bilibili.exe"),
    s(20, 0, 21, 0, "communication", "WeChat.exe"),
    s(21, 0, 22, 0, "study", "Code.exe"), // study：打不了直标的那一档
    s(22, 0, 22, 1, "unknown", null),
    s(22, 1, 23, 0, "work", "Code.exe"),
    s(23, 0, 24, 0, "idle", null),
  ];
}

beforeAll(() => {
  invoke.mockResolvedValue(day());
});

describe("preview", () => {
  it.skipIf(!CSS_BUNDLE)("dumps the real UI to .preview/ for both themes", async () => {
    const { container } = render(<App />);
    await waitFor(() => expect(container.querySelectorAll("svg[role='img']").length).toBeGreaterThan(0));

    // 选中一段，让详情面板也进画面
    const svg = screen.getByRole("img", { name: "24h 活动时间线" });
    fireEvent.click(svg.querySelectorAll("rect")[5]);

    mkdirSync(".preview", { recursive: true });
    for (const theme of ["dark", "light"]) {
      // data-theme 落在 <html> 上（useTheme 写的是 documentElement），
      // 静态页面里得手动带上，否则截出来两张都是默认的深色。
      const html = `<!doctype html>
<html lang="zh-CN" data-theme="${theme}"><head><meta charset="utf-8">
<link rel="stylesheet" href="../dist/assets/${CSS_BUNDLE}">
<style>body{width:1200px;height:700px;overflow:hidden}</style>
</head><body>${container.innerHTML}</body></html>`;
      writeFileSync(join(".preview", `preview-${theme}.html`), html);
    }
  });
});
