/**
 * 类别色板校验。改 design-system/time-scope/MASTER.md §2 或
 * src/styles/theme.css 里的色值之后必须重跑：
 *
 *   node design-system/time-scope/verify-palette.mjs
 *
 * 退出码非 0 = 有硬失败，别提交。
 *
 * **为什么需要这个文件**：八个类别各占一个色相过不了色觉安全闸 ——
 * 最优组合下最差一对在红色盲下 ΔE 仅 2.7，等同同色。改色板时很容易
 * 不知不觉又滑回"八个独立色相"这个看起来更直观的方案，所以把它做成可执行的检查。
 *
 * 干三件事：
 *   1. 校验深浅两套色板
 *   2. 校验两套的文字色对比度
 *   3. **断言浅色 token 的两份副本完全一致**（见下方 LIGHT_BLOCKS 的说明）
 *
 * 校验逻辑来自 dataviz skill 的 validate_palette.js（随 skill 分发，不在本仓库）。
 * 找不到时本脚本会明确报错，而不是悄悄跳过。
 */
import { readFileSync, existsSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

function findValidator() {
  if (process.env.DATAVIZ_SKILL_DIR) {
    const f = join(process.env.DATAVIZ_SKILL_DIR, "scripts", "validate_palette.js");
    if (existsSync(f)) return f;
  }
  const roots = [
    join(process.env.LOCALAPPDATA ?? "", "Temp/claude/bundled-skills"),
    join(process.env.USERPROFILE ?? "", ".claude/skills"),
  ];
  for (const root of roots) {
    if (!existsSync(root)) continue;
    for (const version of readdirSync(root)) {
      let nested = [];
      try {
        nested = readdirSync(join(root, version));
      } catch {
        /* 不是目录 */
      }
      for (const level2 of [version, ...nested]) {
        const f = join(root, version, level2, "dataviz", "scripts", "validate_palette.js");
        if (existsSync(f)) return f;
      }
    }
  }
  return null;
}

const validatorPath = findValidator();
let validate, validateOrdinal, contrast;
if (validatorPath) ({ validate, validateOrdinal, contrast } = await import(`file://${validatorPath}`));

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

/* ── 从 theme.css 取值。色值真相在这里，本脚本不硬编码任何 hex ────── */

/** 取第 n 个 `{...}` 块（从 openIdx 处的 `{` 开始配对）。 */
function blockAt(src, openIdx) {
  let depth = 0;
  for (let i = openIdx; i < src.length; i++) {
    if (src[i] === "{") depth++;
    else if (src[i] === "}" && --depth === 0) return src.slice(openIdx + 1, i);
  }
  throw new Error("CSS 大括号不配对");
}

/** 块内的 `--name: value;` 声明表。 */
function decls(src) {
  const out = new Map();
  for (const m of src.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) out.set(m[1], m[2].trim());
  return out;
}

const DARK = decls(blockAt(theme, theme.indexOf("@theme", theme.indexOf("@theme") + 1) + 5));
const dark = (name) => {
  const v = DARK.get(name);
  if (!v) throw new Error(`theme.css 的 @theme 里找不到 ${name}`);
  return v;
};

/**
 * 浅色 token 写了两份：媒体查询里一份（跟随系统），`[data-theme="light"]` 里一份
 * （用户点按钮选死浅色）。生产 CSP 是 `script-src 'self'`，不能用内联脚本先解析
 * 主题来消重 —— 那样要么有闪烁，要么要动 CSP。所以保留重复，换一条机器检查兜底：
 * 下面会断言两份逐字节一致。
 */
const mediaIdx = theme.indexOf("@media (prefers-color-scheme: light)");
const mediaBlock = blockAt(theme, theme.indexOf("{", mediaIdx));
const LIGHT_FROM_MEDIA = decls(blockAt(mediaBlock, mediaBlock.indexOf("{")));
const LIGHT_FROM_ATTR = decls(blockAt(theme, theme.indexOf("{", theme.indexOf(':root[data-theme="light"]'))));
const light = (name) => {
  const v = LIGHT_FROM_ATTR.get(name);
  if (!v) throw new Error(`theme.css 的浅色块里找不到 ${name}`);
  return v;
};

let failed = 0;
const line = (s) => console.log(s);
const fail = (s) => {
  line(`  ✗ ${s}`);
  failed++;
};

/* ── 0. 两份浅色副本必须逐条一致 ─────────────────────────────── */
line("══ 0. 浅色 token 两份副本是否同步 ══");
const onlyMedia = [...LIGHT_FROM_MEDIA.keys()].filter((k) => !LIGHT_FROM_ATTR.has(k));
const onlyAttr = [...LIGHT_FROM_ATTR.keys()].filter((k) => !LIGHT_FROM_MEDIA.has(k));
const mismatched = [...LIGHT_FROM_MEDIA.keys()].filter(
  (k) => LIGHT_FROM_ATTR.has(k) && LIGHT_FROM_MEDIA.get(k) !== LIGHT_FROM_ATTR.get(k),
);
if (onlyMedia.length) fail(`只在媒体查询里声明：${onlyMedia.join(", ")}`);
if (onlyAttr.length) fail(`只在 [data-theme="light"] 里声明：${onlyAttr.join(", ")}`);
for (const k of mismatched) {
  fail(`${k} 两份不一致：媒体查询 ${LIGHT_FROM_MEDIA.get(k)} vs 属性 ${LIGHT_FROM_ATTR.get(k)}`);
}
if (!failed) line(`  ✓ ${LIGHT_FROM_MEDIA.size} 个 token 两份完全一致`);

/* ── 每个主题跑一遍同样的判据 ─────────────────────────────────── */
const THEMES = [
  { name: "深色", mode: "dark", get: dark },
  { name: "浅色", mode: "light", get: light },
];

for (const T of THEMES) {
  const g = T.get;
  const SURFACE = g("--color-surface-1");

  line(`\n════ ${T.name}（底色 surface-1 = ${SURFACE}）════`);

  const TIERS = {
    专注: [g("--color-cat-work"), g("--color-cat-study")],
    消耗: [g("--color-cat-browsing"), g("--color-cat-entertainment")],
    社交: [g("--color-cat-communication"), g("--color-cat-life")],
  };

  line("── 1. 层级色相之间（all-pairs：时间线上任意两类都可能相邻）──");
  const r = validate(Object.values(TIERS).map((p) => p[0]), {
    mode: T.mode,
    surface: SURFACE,
    pairs: "all",
  });
  for (const [n, s, m] of r.report) line(`  [${String(s).toUpperCase()}] ${n}: ${m}`);
  if (!r.ok) fail(`${T.name}：层级色相 all-pairs 未通过`);

  line("── 2. 层级内明度阶（ordinal）──");
  for (const [tier, pair] of Object.entries(TIERS)) {
    // 次档在浅色底下比主档浅（靠近白），深色底下比主档暗；统一按 亮→暗 排序
    const ordered =
      contrast(pair[0], SURFACE) >= contrast(pair[1], SURFACE) ? pair : [pair[1], pair[0]];
    const rr = validateOrdinal(ordered, { mode: T.mode, surface: SURFACE });
    line(`  ${tier} ${ordered.join(" → ")}: ${rr.ok ? "通过" : "失败"}`);
    for (const [n, s, m] of rr.report) {
      if (!String(s).toUpperCase().startsWith("TRUE") && String(s) !== "pass") {
        line(`      [${String(s).toUpperCase()}] ${n}: ${m}`);
      }
    }
    if (!rr.ok) fail(`${T.name}：${tier} 明度阶未通过`);
  }

  line("── 3. 类别色块对底色（标记需 ≥3:1）──");
  for (const [tier, pair] of Object.entries(TIERS)) {
    for (const [i, hex] of pair.entries()) {
      const c = contrast(hex, SURFACE);
      if (c < 3) fail(`${T.name}：${tier}[${i}] ${hex} 仅 ${c.toFixed(2)}:1`);
    }
  }
  if (!failed) line("  ✓ 6/6 达标");

  line("── 4. 中性色 idle / unknown（刻意低于 3:1，让「无活动」退后）──");
  for (const n of ["--color-cat-idle", "--color-cat-unknown"]) {
    const hex = g(n);
    line(`  ${n} ${hex}  ${contrast(hex, SURFACE).toFixed(2)}:1  （设计如此）`);
  }

  line("── 5. 文字色（正文需 ≥4.5:1）──");
  for (const n of ["--color-ink", "--color-ink-muted", "--color-ink-faint"]) {
    const hex = g(n);
    const c = contrast(hex, SURFACE);
    line(`  ${n} ${hex}  ${c.toFixed(2)}:1  ${c >= 4.5 ? "ok" : "不足 4.5:1"}`);
    if (c < 4.5) fail(`${T.name}：${n} 仅 ${c.toFixed(2)}:1`);
  }

  line("── 6. 状态色 ──");
  // 门槛按**实际用途**定，不是一刀切：
  //   good / critical 只当图标色（非文字）→ WCAG 3:1
  //   warning 当「已脱敏」角标的文字色 → WCAG 4.5:1
  // 改用途时要同步改这里的门槛，否则检查会名不副实。
  for (const [n, need, why] of [
    ["--color-state-good", 3, "仅图标"],
    ["--color-state-critical", 3, "仅图标"],
    ["--color-state-warning", 4.5, "当文字"],
  ]) {
    const hex = g(n);
    const c = contrast(hex, SURFACE);
    line(`  ${n} ${hex}  ${c.toFixed(2)}:1  ${c >= need ? "ok" : `不足 ${need}:1`}  （${why}）`);
    if (c < need) fail(`${T.name}：${n} 仅 ${c.toFixed(2)}:1（${why}，需 ${need}:1）`);
  }

  line("── 7. 段内直标配字（只有 inlineLabel=true 的类别才真的用）──");
  for (const [name, tok] of [
    ["work", "--color-cat-work"],
    ["browsing", "--color-cat-browsing"],
    ["entertainment", "--color-cat-entertainment"],
    ["communication", "--color-cat-communication"],
    ["idle", "--color-cat-idle"],
    ["unknown", "--color-cat-unknown"],
  ]) {
    const fill = g(tok);
    const ink = g(`--on-cat-${tok.replace("--color-cat-", "")}`);
    const c = contrast(fill, ink);
    line(`  ${name.padEnd(14)} ${fill} 上的 ${ink}  ${c.toFixed(2)}:1  ${c >= 4.5 ? "ok" : "不足 4.5:1"}`);
    if (c < 4.5) fail(`${T.name}：${name} 段内直标配字仅 ${c.toFixed(2)}:1`);
  }
}

/* ── 顺序色阶（专注度 / 切换次数）────────────────────────────────
   色阶是单色相的顺序量，判据和类别色不同：
   - 相邻档必须分得开（ΔL ≥ 0.06），否则中间三档在图上糊成一片
   - 最低档必须和「没有活动的桶」分得开 —— 那是不画的，露的是轨道色
   - 最高档必须醒目，否则"值很大"看起来和"值一般"一样
   这一组以前是手算 WCAG 公式验的，写在这里是为了让改色阶也走同一条流水线。 */
for (const T of THEMES) {
  const g = T.get;
  const SURFACE = g("--color-surface-1");
  const RAIL = g("--color-surface-2");
  // 浅色底上低端是浅色，ordinal 要求「亮 → 暗」，所以按对底色的对比度排
  const steps = [1, 2, 3, 4, 5].map((i) => g(`--color-scale-${i}`));
  const ordered = [...steps].sort((a, b) => contrast(b, SURFACE) - contrast(a, SURFACE));

  line(`\n════ 顺序色阶 · ${T.name} ════`);
  const rr = validateOrdinal(ordered, { mode: T.mode, surface: SURFACE });
  for (const [n, st, m] of rr.report) line(`  [${String(st).toUpperCase()}] ${n}: ${m}`);
  if (!rr.ok) fail(`${T.name}：顺序色阶的明度阶未通过`);

  const lowestVsRail = contrast(steps[0], RAIL);
  const highestVsSurface = contrast(steps[4], SURFACE);
  line(
    `  最低档 vs 轨道色(无数据) ${lowestVsRail.toFixed(2)}:1  ${lowestVsRail >= 1.5 ? "ok" : "不足 1.5:1 —— 会和『没活动』混淆"}`,
  );
  if (lowestVsRail < 1.5) {
    fail(
      `${T.name}：色阶最低档与"没有活动的桶"只差 ${lowestVsRail.toFixed(2)}:1，两者会读成同一件事`,
    );
  }
  line(
    `  最高档 vs 底色 ${highestVsSurface.toFixed(2)}:1  ${highestVsSurface >= 3 ? "ok" : "不足 3:1 —— 值最大时也不醒目"}`,
  );
  if (highestVsSurface < 3) fail(`${T.name}：色阶最高档仅 ${highestVsSurface.toFixed(2)}:1`);

  // 色阶 vs 类别色：两者互斥、从不同时出现，但记下最差一对，
  // 有人提议"两个模式画进一行"时能立刻看到代价（MASTER §2.5）
  const catColors = [
    g("--color-cat-work"),
    g("--color-cat-study"),
    g("--color-cat-browsing"),
    g("--color-cat-entertainment"),
    g("--color-cat-communication"),
    g("--color-cat-life"),
  ];
  const mixed = validate([...steps, ...catColors], {
    mode: T.mode,
    surface: SURFACE,
    pairs: "all",
  });
  const nRow = mixed.report.find((r) => r[0] === "Normal-vision floor");
  const cRow = mixed.report.find((r) => r[0] === "CVD separation");
  line(`  色阶 + 类别 混在一起的参照值（两者互斥，不作为闸门）：`);
  line(`    常色视觉最差一对   ${String(nRow[2]).replace(/.*worst all-pairs /, "")}`);
  line(`    色觉障碍最差一对   ${String(cRow[2]).replace(/.*worst all-pairs /, "")}`);
}

line(`\n${failed === 0 ? "全部通过" : `${failed} 项失败 —— 不要提交`}`);
process.exit(failed === 0 ? 0 : 1);