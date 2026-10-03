import type { Category } from "../types";

/**
 * 类别的展示元数据。
 *
 * 这里**只放 CSS 变量名，不放色值** —— 色值唯一的真相在 `styles/theme.css`。
 * 之前色值在 TS 和组件里各存一份，改主题要记得改两处，迟早漏。
 *
 * 编码结构（详见 design-system/time-scope/MASTER.md §2）：
 * 色相只承载 3 个大层级，层级内用明度区分具体类别。八个类别各占一个色相
 * 在数学上过不了色觉安全闸 —— 最优组合下最差一对在红色盲下 ΔE 仅 2.7，
 * 等同同色。
 */
export type CategoryTier = "focused" | "consuming" | "social" | "absent";

/** 层级色相，界面用它做分组标题和筛选。 */
export const TIER_LABEL: Record<CategoryTier, string> = {
  focused: "专注",
  consuming: "消耗",
  social: "社交",
  absent: "无活动",
};

/** 层级的展示顺序：先看一天里真正在做什么，最后才是空白。 */
export const TIER_ORDER: CategoryTier[] = ["focused", "consuming", "social", "absent"];

export interface CategoryMeta {
  /** 中文名。界面上显示它；原始 key 仍留在 title 里，方便对照 rules.toml。 */
  label: string;
  /** 指向 theme.css 里的 CSS 变量（不含 var() 包裹） */
  varName: string;
  tier: CategoryTier;
  /** 同色相内的明度级：主 = 该层级里更显眼的那个，次 = 退后档 */
  step: "primary" | "secondary" | null;
  /**
   * 段内能不能打直标。**这是结构决策，与配色无关** —— 标签颜色由 CSS 的
   * `--on-cat-*` 随主题走，不在这里决定。
   *
   * study / life 在两个主题下都是 false：深色底下它们的浅字只有 4.47 /
   * 4.41:1，压在 4.5 线下；调深则对底色的标记对比跌破 3:1（蓝相无解）。
   * 让它随主题变化会导致"某些主题下多出标签"的界面结构漂移，所以一刀切。
   * 身份交给悬停浮层、常驻图例和选中读数三条通道。
   */
  inlineLabel: boolean;
}

export const CATEGORY_META: Record<Category, CategoryMeta> = {
  work: { label: "工作", varName: "--color-cat-work", tier: "focused", step: "primary", inlineLabel: true },
  study: { label: "学习", varName: "--color-cat-study", tier: "focused", step: "secondary", inlineLabel: false },
  browsing: {
    label: "浏览",
    varName: "--color-cat-browsing",
    tier: "consuming",
    step: "primary",
    inlineLabel: true,
  },
  entertainment: {
    label: "娱乐",
    varName: "--color-cat-entertainment",
    tier: "consuming",
    step: "secondary",
    inlineLabel: true,
  },
  communication: {
    label: "沟通",
    varName: "--color-cat-communication",
    tier: "social",
    step: "primary",
    inlineLabel: true,
  },
  life: { label: "生活", varName: "--color-cat-life", tier: "social", step: "secondary", inlineLabel: false },
  idle: { label: "空闲", varName: "--color-cat-idle", tier: "absent", step: null, inlineLabel: true },
  unknown: { label: "未分类", varName: "--color-cat-unknown", tier: "absent", step: null, inlineLabel: true },
};

/** 后端新增了 category 而前端没跟上时的兜底样式。 */
const FALLBACK: CategoryMeta = {
  label: "未分类",
  varName: "--color-cat-unknown",
  tier: "absent",
  step: null,
  inlineLabel: true,
};

export function metaForCategory(c: string): CategoryMeta {
  return CATEGORY_META[c as Category] ?? FALLBACK;
}

/**
 * 供 SVG `fill` 与内联 `background` 使用的颜色引用。
 * 返回 `var(--color-cat-*)` 而不是 hex，主题一改就跟着变。
 */
export function colorForCategory(c: string): string {
  return `var(${metaForCategory(c).varName})`;
}

/** 段内直标压在色块上的文字色。由 CSS 随主题决定，这里只给变量名。 */
export function labelInkFor(c: string): string {
  return `var(--on-cat-${metaForCategory(c).varName.replace("--color-cat-", "")})`;
}

export function labelForCategory(c: string): string {
  return metaForCategory(c).label;
}
