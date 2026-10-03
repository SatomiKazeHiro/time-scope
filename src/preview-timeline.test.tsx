/**
 * 时间线专项预览：只渲染 SegmentTimeline，用碎片化的真实数据。
 *
 * 目的是复现"看着不明显"的场景 —— 之前那份 14 个整块的 mock 过于干净，
 * 真实的采集结果是几十上百个短段交替，1–3 分钟的段连成锯齿。
 *
 * 跑：pnpm build && pnpm vitest run src/preview-timeline.test.tsx
 */
import { describe, it, expect } from "vitest";
import { render } from "@testing-library/react";
import { writeFileSync, mkdirSync, readdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import SegmentTimeline from "./components/SegmentTimeline";
import { GRANULARITIES, sliceSegments } from "./lib/bucket";
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

/** 造一天：以 2–6 分钟的短段为主，夹杂少量长段，最贴近真实采集。 */
function fragmentedDay(): Segment[] {
  const r = rng(20261003);
  const at = (min: number) => {
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    return d.getTime() + min * 60_000;
  };
  const pool: Category[] = [
    "work", "work", "work", "browsing", "communication",
    "study", "life", "entertainment", "idle", "work",
  ];
  const out: Segment[] = [];
  let t = 0;
  let i = 0;
  while (t < 24 * 60) {
    // 多数 1–5 分钟，偶尔来一段长的
    const dur = r() < 0.82 ? 1 + Math.floor(r() * 5) : 8 + Math.floor(r() * 50);
    const end = Math.min(t + dur, 24 * 60);
    // 凌晨到早上更容易是 idle
    const cat = t < 7 * 60 && t > 22 * 60 ? "idle" : pool[Math.floor(r() * pool.length)];
    out.push({
      id: `f${i++}`,
      startAt: at(t),
      endAt: at(end),
      category: cat,
      application: "Code.exe",
      confidence: 0.9,
      classifier: "rule",
      classifierVersion: "rules:15",
      evidenceEventIds: ["e1"],
    });
    t = end;
  }
  return out;
}

describe("timeline preview", () => {
  it.skipIf(!CSS_BUNDLE)("dumps a fragmented day for both themes", () => {
    const segs = fragmentedDay();
    const dayStartMs = (() => {
      const d = new Date();
      d.setHours(0, 0, 0, 0);
      return d.getTime();
    })();

    /** 长时段对照：碎片日看不出粒度的差别，这一段才看得出。 */
    const at = (h: number) => {
      const d = new Date();
      d.setHours(0, 0, 0, 0);
      return d.getTime() + h * 3_600_000;
    };
    const longRun: Segment[] = [
      {
        id: "L1",
        startAt: at(9),
        endAt: at(15), // 连续 6 小时
        category: "work",
        application: "Code.exe",
        confidence: 0.95,
        classifier: "rule",
        classifierVersion: "rules:15",
        evidenceEventIds: ["e1"],
      },
      {
        id: "L2",
        startAt: at(16),
        endAt: at(17.5), // 90 分钟
        category: "browsing",
        application: "chrome.exe",
        confidence: 0.8,
        classifier: "rule",
        classifierVersion: "rules:15",
        evidenceEventIds: ["e2"],
      },
    ];

    const { container } = render(
      <div style={{ background: "var(--color-surface-0)", padding: 16 }}>
        {/* 顺序是有意的，而且 headless 截图只截首屏：
            ① 默认视图（碎片日 + 30 分）—— 用户每天真正看到的那一张
            ② 长时段四档对照 —— 粒度的差别只有在长时段上才看得出来 */}

        <p style={{ color: "var(--color-ink)", fontSize: 13, fontWeight: 600 }}>
          ① 默认视图：碎片日（{segs.length} 段，最短{" "}
          {Math.round(Math.min(...segs.map((s) => s.endAt - s.startAt)) / 60000)} 分钟）· 30 分粒度
        </p>
        <div style={{ marginTop: 12 }}>
          <SegmentTimeline
            segments={segs}
            dayStartMs={dayStartMs}
            onSelect={() => {}}
            showNow
            intervalMs={30 * 60_000}
          />
        </div>
        <p style={{ color: "var(--color-ink-muted)", fontSize: 12 }}>
          切出 {sliceSegments(segs, 30 * 60_000).length} 块。
          碎片日在四档粒度下差别不大 —— 段本来就短，切不切都那样。
        </p>

        <p
          style={{
            color: "var(--color-ink)",
            fontSize: 13,
            fontWeight: 600,
            marginTop: 40,
            borderTop: "1px solid var(--color-line)",
            paddingTop: 24,
          }}
        >
          ② 长时段对照：一段连续 6 小时的工作 + 一次 90 分钟的浏览
        </p>
        {GRANULARITIES.map((m) => (
          <div key={`long-${m}`} style={{ marginTop: 20 }}>
            <SegmentTimeline
              segments={longRun}
              dayStartMs={dayStartMs}
              onSelect={() => {}}
              intervalMs={m * 60_000}
            />
            <p style={{ color: "var(--color-ink-muted)", fontSize: 12 }}>
              {m} 分粒度 · 2 段 → {sliceSegments(longRun, m * 60_000).length} 块
            </p>
          </div>
        ))}
      </div>,
    );
    expect(container.querySelectorAll("rect").length).toBeGreaterThan(segs.length);

    // 悬停提示是 React state 触发的，静态页面里出不来。但"被 overflow-hidden
    // 裁掉"本来就是 CSS 布局问题，所以照着组件里的 class 手工塞一个进去，
    // 用截图确认它完整地浮在轨道**上方**而不是被切掉一半。
    const rail = container.querySelector(".group")!;
    rail.insertAdjacentHTML(
      "beforebegin",
      `<div class="relative" style="position:relative">
         <div role="tooltip" style="position:absolute;left:38%;top:-8px;z-index:20;
              transform:translate(-50%,-100%);
              border-radius:6px;border:1px solid var(--color-line-strong);
              background:var(--color-surface-3);padding:6px 10px;
              font-size:12px;white-space:nowrap;box-shadow:var(--shadow-pop)">
           <span class="mr-1.5 inline-block size-2 rounded-[2px] align-middle"
                 style="background:var(--color-cat-work)"></span>
           <span style="font-weight:500">工作</span>
           <span style="color:var(--color-ink-muted)"> · Code.exe</span>
           <div class="tnum mt-0.5" style="color:var(--color-ink-muted)">
             09:04 – 12:30 <span class="mx-1" style="color:var(--color-ink-ghost)">|</span> 3 时 26 分
           </div>
         </div>
       </div>`,
    );

    mkdirSync(".preview", { recursive: true });
    for (const theme of ["dark", "light"]) {
      const html = `<!doctype html>
<html lang="zh-CN" data-theme="${theme}"><head><meta charset="utf-8">
<link rel="stylesheet" href="../dist/assets/${CSS_BUNDLE}">
<style>body{width:1200px;margin:0;padding:40px 0}</style>
</head><body>${container.innerHTML}</body></html>`;
      writeFileSync(join(".preview", `timeline-${theme}.html`), html);
    }
  });
});
