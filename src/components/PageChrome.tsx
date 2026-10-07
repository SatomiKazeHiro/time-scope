import { CircleAlert } from "lucide-react";
import type { ReactNode } from "react";

/**
 * 两个页面共用的外壳零件。
 *
 * 之前监控页和汇总页各写一份页头和错误提示，class 抄着抄着就分叉了 ——
 * 汇总页的错误提示少了 `CircleAlert` 图标就是这么来的。骨架只有一处，
 * 分叉就无处发生。
 */

/** 页头：应用名 + 该页自己的内容。跨行行为两页一致（窄窗口下换行而不是裁切）。 */
export function PageHeader({ children }: { children?: ReactNode }) {
  return (
    <div className="flex shrink-0 flex-wrap items-center gap-x-2 gap-y-2">
      {/* 应用名降级成安静标识：托盘里已经常驻，20px 的 h1 是抢戏。
          whitespace-nowrap 必需：不加的话窄窗口下会被逐字折成竖排。 */}
      <h1 className="m-0 shrink-0 text-label font-semibold tracking-wide whitespace-nowrap text-ink-muted">
        Time Scope
      </h1>
      {children}
    </div>
  );
}

/** 页头里的竖分隔线：把"这是哪个应用"和"这一页在看什么"分开。 */
export function HeaderDivider() {
  return <span aria-hidden className="mx-1 hidden h-5 w-px shrink-0 bg-line sm:block" />;
}

/**
 * 范围读数。**不是控件。**
 *
 * 曾经它是个 `<button aria-disabled="true">`，带着边框、底色和 padding，
 * 和旁边真能点的「回到今天」几乎同款 —— 长得能点却点不动。
 * 范围本来由热力图上的点击驱动，这里只是**念出当前选的是什么**，
 * 所以读数就该长得像读数。
 *
 * 选中的那一天/周/月比面板标题里的日期范围更贴近指标区（那里是数字），
 * 所以这个位置留着是有用的，只是别再伪装成控件。
 */
export function RangeReadout({ label }: { label: string }) {
  return (
    <span
      data-testid="range-chip"
      className="text-sm whitespace-nowrap text-ink-muted"
      title="在热力图上点某天 / 周 / 月可以切换范围"
    >
      <span className="text-ink-faint">范围</span> {label}
    </span>
  );
}

/** 加载失败。两页共用同一份文案与样式 —— 之前汇总页漏了警告图标。 */
export function PageAlert({ children }: { children?: ReactNode }) {
  return (
    <div
      role="alert"
      className="flex items-start gap-2 rounded-md border border-line bg-surface-1 p-3 text-sm text-ink"
    >
      <CircleAlert size={15} className="mt-0.5 shrink-0 text-state-critical" aria-hidden />
      <span>
        {children ?? (
          <>
            加载失败。请确认后端已启动（<code className="tnum">pnpm tauri dev</code>）。
          </>
        )}
      </span>
    </div>
  );
}