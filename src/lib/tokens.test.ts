import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { GAP } from "./summary";

/**
 * 每个 var(--x) 引用都必须能在 theme.css 里找到定义。
 *
 * 不存在的 var() 会让声明在**计算值阶段**失效：border 的颜色写错，
 * 整条 border 声明被丢掉，元素看着像「样式没生效」而不是「写错了」。
 * 这已经犯过两次（环心、选中描边），所以用测试守住。
 *
 * 顺带守住贡献墙描边的**几何**：那两条 box-shadow 住在 theme.css
 * （内描边与选中外环必须能叠加），所以几何也只能对着样式表断言。
 */
const THEME = readFileSync("src/styles/theme.css", "utf8");

/** 运行时由 JS 设上去的自定义属性，不在 theme.css 里。 */
const RUNTIME = new Set(["--cw", "--gap"]);

function sources(): string[] {
  const out: string[] = [];
  const walk = (dir: string) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, e.name);
      if (e.isDirectory()) walk(p);
      else if (/\.tsx?$/.test(e.name) && !/\.test\./.test(e.name)) out.push(p);
    }
  };
  walk("src");
  return out;
}

describe("CSS token 引用", () => {
  it("所有 var(--x) 都在 theme.css 里定义过", () => {
    const bad: string[] = [];
    for (const f of sources()) {
      const src = readFileSync(f, "utf8");
      for (const m of src.matchAll(/var\((--[a-zA-Z0-9-]+)\)/g)) {
        const name = m[1];
        if (RUNTIME.has(name)) continue;
        if (!new RegExp(`\s*${name}\s*:`).test(THEME)) bad.push(`${f}: ${name}`);
      }
    }
    expect(bad, `theme.css 未定义：\n${bad.join("\n")}`).toEqual([]);
  });
});

describe("贡献墙描边（.wall-cell）", () => {
  it("选中外环的半径 = 半格间隙，相邻选中格才能连成一片", () => {
    // 间隙 3px 时外环向外扩 1.5px，两侧相邻的环正好相接 ——
    // 这是「不用跨格矩形也能框出一整片」的全部机制。
    // 写成 GAP/2 而不是硬编码 1.5，间隙一改这里就红。
    expect(THEME).toMatch(
      new RegExp(`\\.wall-cell\\[data-in-range\\][^{]*\\{[^}]*0 0 0 ${GAP / 2}px var\\(--color-ink\\)`),
    );
  });

  it("外环与内描边是两条 box-shadow，可叠加", () => {
    // 只有一条 box-shadow 的话，「空档 + 选中」会二选一：
    // 外环把那 1px 内描边覆盖掉，格子就看不出是空档还是没渲染。
    expect(THEME).toMatch(
      /box-shadow:\s*var\(--wall-ring\),\s*var\(--wall-stroke\)/,
    );
    for (const tone of ["empty", "uninstalled"]) {
      expect(THEME).toMatch(new RegExp(`\\.wall-cell\\[data-tone="${tone}"\\]`));
    }
  });

  it("框外不压暗：没有给 .wall-cell 加任何 opacity", () => {
    // 最低档卡在 ordinal 的 2:1 底线上（2.29 / 2.07），实测压到
    // α=0.85 就掉到 1.99:1 —— 聚光灯方案会破线，所以只用描边（MASTER §2.5）。
    const wallBlocks = [...THEME.matchAll(/\.wall-cell[^{]*\{[^}]*\}/g)].map((m) => m[0]);
    expect(wallBlocks.length).toBeGreaterThan(0);
    for (const b of wallBlocks) expect(b).not.toContain("opacity");
  });
});
