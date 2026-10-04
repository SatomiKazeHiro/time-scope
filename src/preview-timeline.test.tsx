/**
 * 时间线专项预览：只渲染 SegmentTimeline，用碎片化的真实数据。
 *
 * 目的是复现"看着不明显"的场景 —— 之前那份 14 个整块的 mock 过于干净，
 * 真实的采集结果是几十上百个短段交替，1–3 分钟的段连成锯齿。
 *
 * 现在看三档：类别（不切片）/ 专注度 / 切换次数。粒度在类别模式下只管刻度尺，
 * 切段是指标模式的事，所以对比的重点从"粒度切几刀"变成了"同一份数据三种读法"。
 *
 * 跑：pnpm build && pnpm vitest run src/preview-timeline.test.tsx
 */
import { describe, it, expect } from "vitest";
import { render } from "@testing-library/react";
import { writeFileSync, mkdirSync, readdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import SegmentTimeline, { METRIC_LABEL, type MetricMode } from "./components/SegmentTimeline";
import { ScaleLegend } from "./components/MetricPicker";
import type { Category, Segment } from "./types";

const CSS_BUNDLE = existsSync("dist/assets")
  ? readdirSync("dist/assets").find((f: string) => f.endsWith(".css"))
  : undefined;

/** 确定性伪随机：同一份数据每次渲染一致，截图才好对比。 */
function rng(seed: number) {
  let s = seed;
  return () => {
    s = (s * 1664525 + 1013904223) % 4294967296;
    return s / 4294967296;
  };
}

/** 造一天：碎片为主 + 两段长专注 —— 真实采集就是这个形状。 */
function fragmentedDay(): Segment[] {
  const r = rng(20261003);
  const at = (min: number) => {
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    return d.getTime() + min * 60_000;
  };
  const out: Segment[] = [];
  let n = 0;
  const push = (from: number, to: number, category: Category, application: string | null) => {
    out.push({
      id: `f${n++}`,
      startAt: at(from),
      endAt: at(to),
      category,
      application,
      confidence: 0.9,
      classifier: "rule",
      classifierVersion: "rules:15",
      evidenceEventIds: ["e1"],
    });
  };

  push(0, 7 * 60 + 40, "idle", null); // 睡眠
  const morning: [Category, string][] = [
    ["work", "Code.exe"],
    ["browsing", "chrome.exe"],
    ["communication", "WeChat.exe"],
    ["work", "Code.exe"],
    ["life", "explorer.exe"],
  ];
  let t = 7 * 60 + 40;
  while (t < 12 * 60 - 6) {
    const end = Math.min(t + 2 + Math.floor(r() * 9), 12 * 60 - 3);
    const [cat, app] = morning[Math.floor(r() * morning.length)];
    push(t, end, cat, app);
    t = end + (r() < 0.3 ? 4 : 1);
  }
  push(t, 13 * 60 + 10, "life", "explorer.exe"); // 午饭
  push(13 * 60 + 20, 15 * 60 + 40, "work", "Code.exe"); // 2h20m 连着做
  push(15 * 60 + 40, 16 * 60, "life", "explorer.exe");
  push(16 * 60, 18 * 60 + 30, "work", "Code.exe"); // 又一段 2h30m

  const evening: [Category, string][] = [
    ["browsing", "chrome.exe"],
    ["communication", "WeChat.exe"],
    ["entertainment", "bilibili.exe"],
    ["life", "explorer.exe"],
  ];
  let e = 18 * 60 + 30;
  while (e < 23 * 60) {
    const end = Math.min(e + 2 + Math.floor(r() * 8), 23 * 60);
    const [cat, app] = evening[Math.floor(r() * evening.length)];
    push(e, end, cat, app);
    e = end + (r() < 0.35 ? 3 : 1);
  }
  push(23 * 60, 24 * 60, "idle", null);
  return out;
}

const MODES: MetricMode[] = ["category", "focus", "switch"];

describe("timeline preview", () => {
  it.skipIf(!CSS_BUNDLE)("dumps one day across all three metrics, both themes", () => {
    const segs = fragmentedDay();
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    const dayStartMs = d.getTime();

    const { container } = render(
      <div style={{ background: "var(--color-surface-0)", padding: 16 }}>
        <p style={{ color: "var(--color-ink)", fontSize: 13, fontWeight: 600 }}>
          同一天（{segs.length} 段，最短{" "}
          {Math.round(Math.min(...segs.map((s) => s.endAt - s.startAt)) / 60000)} 分钟）的三种读法
        </p>
        {MODES.map((metric) => (
          <div key={metric} style={{ marginTop: 24 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 8 }}>
              <span style={{ color: "var(--color-ink-muted)", fontSize: 12, fontWeight: 600 }}>
                {METRIC_LABEL[metric]}
              </span>
              <ScaleLegend metric={metric} />
            </div>
            <SegmentTimeline
              segments={segs}
              dayStartMs={dayStartMs}
              onSelect={() => {}}
              showNow
              intervalMs={30 * 60_000}
              metric={metric}
            />
          </div>
        ))}
      </div>,
    );
    // 类别模式一段一块，指标模式一桶一块；无论哪种都不该是 0
    expect(container.querySelectorAll("rect").length).toBeGreaterThan(0);

    mkdirSync(".preview", { recursive: true });
    for (const theme of ["dark", "light"]) {
      const html = `<!doctype html>
<html lang="zh-CN" data-theme="${theme}"><head><meta charset="utf-8">
<link rel="stylesheet" href="../dist/assets/${CSS_BUNDLE}">
<style>html,body{margin:0}</style>
</head><body>${container.innerHTML}</body></html>`;
      writeFileSync(join(".preview", `timeline-${theme}.html`), html);
    }
  });
});
