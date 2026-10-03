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
  /** 同色相内的明度级：亮 = 该层级的"主"活动，暗 = 次要变体 */
  step: "bright" | "dim";
  /**
   * 段内直标的文字颜色；null = 这档色块上打不了标签。
   *
   * 亮档配深字（5.27–5.64:1），暗档理论上配浅字，但 study / life 两档的浅字
   * 只有 4.47 / 4.41:1，压在 4.5 线下；而为了拉高浅字对比把它们调深，对底色的
   * 标记对比又会跌破 3:1。蓝相在这条窄区间里无解。
   * 所以这两档不直标，身份交给悬停浮层、常驻图例和选中读数 ——
   * 这就是 dataviz 说的 selective direct labels。
   */
  labelInk: "dark" | "light" | null;
}

export const CATEGORY_META: Record<Category, CategoryMeta> = {
  work: {
    label: "工作",
    varName: "--color-cat-work",
    tier: "focused",
    step: "bright",
    labelInk: "dark",
  },
  study: {
    label: "学习",
    varName: "--color-cat-study",
    tier: "focused",
    step: "dim",
    labelInk: null,
  },
  browsing: {
    label: "浏览",
    varName: "--color-cat-browsing",
    tier: "consuming",
    step: "bright",
    labelInk: "dark",
  },
  entertainment: {
    label: "娱乐",
    varName: "--color-cat-entertainment",
    tier: "consuming",
    step: "dim",
    labelInk: "light",
  },
  communication: {
    label: "沟通",
    varName: "--color-cat-communication",
    tier: "social",
    step: "bright",
    labelInk: "dark",
  },
  life: {
    label: "生活",
    varName: "--color-cat-life",
    tier: "social",
    step: "dim",
    labelInk: null,
  },
  idle: {
    label: "空闲",
    varName: "--color-cat-idle",
    tier: "absent",
    step: "bright",
    labelInk: "light",
  },
  unknown: {
    label: "未分类",
    varName: "--color-cat-unknown",
    tier: "absent",
    step: "dim",
    labelInk: "light",
  },
};

/** 后端新增了 category 而前端没跟上时的兜底样式。 */
const FALLBACK: CategoryMeta = {
  label: "未分类",
  varName: "--color-cat-unknown",
  tier: "absent",
  step: "dim",
  labelInk: "light",
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

export function labelForCategory(c: string): string {
  return metaForCategory(c).label;
}
