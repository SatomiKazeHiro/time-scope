/**
 * 类别色板校验。改 design-system/time-scope/MASTER.md §2 或
 * src/styles/theme.css 里的 --color-cat-* 之后必须重跑：
 *
 *   node design-system/time-scope/verify-palette.mjs
 *
 * 退出码非 0 = 有硬失败，别提交。
 *
 * **为什么需要这个文件**：八个类别各占一个色相过不了色觉安全闸 ——
 * 最优组合下最差一对在红色盲下 ΔE 仅 2.7，等同同色。改色板时很容易
 * 不知不觉又滑回"八个独立色相"这个看起来更直观的方案，所以把它做成可执行的检查。
 *
 * 校验逻辑来自 dataviz skill 的 validate_palette.js（那是一份随 skill 分发的
 * 脚本，不在本仓库）。找不到它时本脚本会明确报错，而不是悄悄跳过。
 */
import { readFileSync, existsSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

/** 找 dataviz skill 的 validate_palette.js。路径带版本和哈希，所以要搜。 */
function findValidator() {
  if (process.env.DATAVIZ_SKILL_DIR) {
    const f = join(process.env.DATAVIZ_SKILL_DIR, "scripts", "validate_palette.js");
    if (existsSync(f)) return f;
  }

  const roots = [
    join(process.env.LOCALAPPDATA ?? "", "Temp/claude/bundled-skills"),
    join(process.env.USERPROFILE ?? "", ".claude/skills"),
  ];
  // 实际结构是 <root>/<版本>/<哈希>/dataviz/scripts/validate_palette.js
  for (const root of roots) {
    if (!existsSync(root)) continue;
    for (const version of readdirSync(root)) {
      for (const level2 of [version, ...safeReaddir(join(root, version))]) {
        const f = join(root, version, level2, "dataviz", "scripts", "validate_palette.js");
        if (existsSync(f)) return f;
      }
    }
  }
  return null;
}

function safeReaddir(dir) {
  try {
    return readdirSync(dir);
  } catch {
    return [];
  }
}

const validatorPath = findValidator();
let validate, validateOrdinal, contrast;
if (validatorPath) {
  ({ validate, validateOrdinal, contrast } = await import(`file://${validatorPath}`));
}

if (!validate) {
  console.error(
    "找不到 dataviz skill 的 validate_palette.js。\n" +
      "它随 skill 一起分发，不在本仓库里。装上 dataviz skill 后重跑，或手动指定：\n" +
      "  set DATAVIZ_SKILL_DIR=<dataviz skill 的目录>\n",
  );
  process.exit(2);
}

const here = dirname(fileURLToPath(import.meta.url));
const theme = readFileSync(join(here, "..", "..", "src", "styles", "theme.css"), "utf8");

/** 色值真相在 theme.css，这里读出来而不是再抄一份 —— 抄的会和源码漂移。 */
function token(name) {
  const m = theme.match(new RegExp(`${name}:\\s*(#[0-9a-fA-F]{6})`));
  if (!m) throw new Error(`theme.css 里找不到 ${name}`);
  return m[1];
}

const SURFACE = token("--color-surface-1");

/** 三个层级色相 —— 时间线上任意两类都可能相邻，所以按 all-pairs 校验。 */
const TIERS = {
  focused: [token("--color-cat-work"), token("--color-cat-study")],
  consuming: [token("--color-cat-browsing"), token("--color-cat-entertainment")],
  social: [token("--color-cat-communication"), token("--color-cat-life")],
};

let failed = 0;
const line = (s) => console.log(s);

line("══ 1. 层级色相之间（all-pairs）══");
const hues = Object.values(TIERS).map((p) => p[0]);
const r = validate(hues, { mode: "dark", surface: SURFACE, pairs: "all" });
for (const [name, state, msg] of r.report) {
  line(`  [${String(state).toUpperCase()}] ${name}: ${msg}`);
}
if (!r.ok) failed++;
line(`  => ${r.ok ? "通过" : "失败"}`);

line("\n══ 2. 层级内明度阶（ordinal）══");
for (const [tier, pair] of Object.entries(TIERS)) {
  const rr = validateOrdinal(pair, { mode: "dark", surface: SURFACE });
  line(`  ${tier} ${pair.join(" → ")}: ${rr.ok ? "通过" : "失败"}`);
  for (const [n, st, m] of rr.report) line(`    [${String(st).toUpperCase()}] ${n}: ${m}`);
  if (!rr.ok) failed++;
}

line("\n══ 3. 每个类别对底色的对比度（色块需 ≥3:1）══");
for (const [tier, pair] of Object.entries(TIERS)) {
  for (const [i, hex] of pair.entries()) {
    const c = contrast(hex, SURFACE);
    const ok = c >= 3;
    if (!ok) failed++;
    line(`  ${tier}[${i}] ${hex}  ${c.toFixed(2)}:1  ${ok ? "ok" : "不足 3:1"}`);
  }
}

line("\n══ 4. 中性色（idle / unknown，刻意低于 3:1 让「无活动」退后）══");
for (const n of ["--color-cat-idle", "--color-cat-unknown"]) {
  const hex = token(n);
  line(`  ${n} ${hex}  ${contrast(hex, SURFACE).toFixed(2)}:1  （设计如此，非失败）`);
}

line("\n══ 5. 文字色（正文需 ≥4.5:1）══");
for (const n of ["--color-ink", "--color-ink-muted", "--color-ink-faint"]) {
  const hex = token(n);
  const c = contrast(hex, SURFACE);
  const ok = c >= 4.5;
  if (!ok) failed++;
  line(`  ${n} ${hex}  ${c.toFixed(2)}:1  ${ok ? "ok" : "不足 4.5:1"}`);
}

line(`\n${failed === 0 ? "全部通过" : `${failed} 项失败 —— 不要提交`}`);
process.exit(failed === 0 ? 0 : 1);
