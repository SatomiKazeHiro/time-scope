import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

/**
 * 每个 var(--x) 引用都必须能在 theme.css 里找到定义。
 *
 * 不存在的 var() 会让声明在**计算值阶段**失效：border 的颜色写错，
 * 整条 border 声明被丢掉，元素看着像「样式没生效」而不是「写错了」。
 * 这已经犯过两次（环心、选中描边），所以用测试守住。
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
