import type { DonutSlice } from "../types";

interface DonutChartProps {
  donut: DonutSlice[];
  totalMs: number;
}

/**
 * 圆环的档位 -> 颜色。**类别色**，不是连续量色阶。
 *
 * 蓝/橙/灰是 `--color-cat-*`，紫阶 `--color-scale-*` 是连续量的
 * （MASTER §2.5）。混用会让「蓝 = work」这条已建立的语义失效。
 */
const ARC_COLOR: Record<string, string> = {
  work: "var(--color-cat-work)",
  browsing: "var(--color-cat-browsing)",
  idle: "var(--color-cat-idle)",
  unknown: "var(--color-cat-unknown)",
};

const ARC_LABEL: Record<string, string> = {
  work: "工作",
  browsing: "浏览",
  idle: "空闲",
  unknown: "未分类",
};

/** 后端漏发 / 老库里的未知档位回落到「未分类」，不崩也不丢颜色。 */
const colorOf = (key: string) => ARC_COLOR[key] ?? ARC_COLOR.unknown;
const labelOf = (key: string) => ARC_LABEL[key] ?? key;

const R = 28;
const C = 2 * Math.PI * R;

/**
 * 固定四档的圆环。手写 SVG 弧段，不引 d3 —— 四个 arc 用
 * `stroke-dasharray` 就够了（spec §1.2、§6）。
 *
 * 环是完整的 360°：`unknown` 那一档同时兜住「规则没覆盖」和
 * 学习/娱乐/社交/生活（后端已并档）。
 */
export default function DonutChart({ donut, totalMs }: DonutChartProps) {
  const safeTotal = Math.max(totalMs, 0);
  // totalMs 为 0 时全部 0 弧段，环空着而不是除以 0
  const fracs = donut.map((d) => (safeTotal > 0 ? Math.max(d.ms, 0) / safeTotal : 0));
  let acc = 0;

  return (
    <div className="flex items-center gap-4">
      <svg viewBox="0 0 64 64" width="64" height="64" className="shrink-0">
        {fracs.map((frac, i) => {
          const dash = frac * C;
          const el = (
            <circle
              key={donut[i].key}
              data-arc=""
              cx="32" cy="32" r={R} fill="none"
              stroke={colorOf(donut[i].key)}
              strokeWidth="9"
              strokeDasharray={`${dash} ${C - dash}`}
              strokeDashoffset={-acc * C}
              transform="rotate(-90 32 32)"
            />
          );
          acc += frac;
          return el;
        })}
        {/* 环心写监控总时长（spec §6）——比例之外再给一个绝对数。
            颜色用已有 token 的内联 fill：不新增 class（theme.css 里没有
            就等于没样式，SVG 默认黑字在深色底上等于隐形），也不新增 token。 */}
        <text
          x="32" y="30" textAnchor="middle"
          fill="var(--color-ink)" fontSize="10" fontWeight="600"
        >
          {fmtHours(totalMs)}
        </text>
        <text
          x="32" y="38" textAnchor="middle"
          fill="var(--color-ink-faint)" fontSize="6"
        >
          监控
        </text>
      </svg>

      {/* 图例。中间那一列是**比例条**，不是空气。
          原本是 `grid-cols-[9px_1fr_auto_auto]`，1fr 撑的是「名称」列，
          于是卡片一宽，所有余量都变成标签和数字之间的空白 —— 卡右侧看着空，
          根子在这。现在这一列画条：既填掉空白，又把比例直接读出来，
          还和监控页「当日汇总」的分类行是同一套（两个页面长得像而不是各说各话）。 */}
      <ul className="grid flex-1 gap-1 text-xs">
        {donut.map((d) => {
          const pct = safeTotal > 0 ? Math.round((Math.max(d.ms, 0) / safeTotal) * 100) : 0;
          return (
            <li
              key={d.key}
              data-legend=""
              className="grid grid-cols-[9px_auto_1fr_auto_auto] items-center gap-2"
            >
              <span aria-hidden className="size-2 rounded-sm"
                    style={{ background: colorOf(d.key) }} />
              <span className="text-ink">{labelOf(d.key)}</span>
              <span className="h-1.5 min-w-6 overflow-hidden rounded-full bg-surface-2">
                <span
                  className="block h-full rounded-full"
                  style={{ width: `${Math.max(pct, pct > 0 ? 2 : 0)}%`, background: colorOf(d.key) }}
                />
              </span>
              <span className="tnum text-ink-faint">{fmt(d.ms)}</span>
              <span className="tnum w-9 text-right text-ink-faint">{pct}%</span>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

/** 环心读数：一律按小时，一位小数。 */
function fmtHours(ms: number): string {
  return `${Math.round((Math.max(ms, 0) / 3_600_000) * 10) / 10}h`;
}

/** 图例读数：小时一位小数，不足 1 小时按分钟。 */
function fmt(ms: number): string {
  const h = Math.max(ms, 0) / 3_600_000;
  return h >= 1 ? `${Math.round(h * 10) / 10}h` : `${Math.round(ms / 60_000)}m`;
}
