import type { LucideIcon } from "lucide-react";
import { Activity, BarChart3, Monitor, Moon, Settings, Sun } from "lucide-react";
import { useTheme, THEME_LABEL } from "../design/useTheme";

/** 左侧导航目前有三个落点；设置页是空壳，等有内容再扩。 */
export type View = "summary" | "monitor" | "settings";

const THEME_ICON = { system: Monitor, light: Sun, dark: Moon } as const;

interface SidebarProps {
  view: View;
  onViewChange: (v: View) => void;
}

/**
 * 左侧图标栏。
 *
 * 上下分层：上层是导航（占了除底部以外的空间），下层是工具
 * （主题切换、设置）。**主题从页头搬到了这里** —— 它是应用级偏好，
 * 不该和「看哪一天、看哪个指标」这些视图控件挤在同一行工具栏里。
 *
 * 悬浮提示挂在按钮内部、`left-full` 伸到栏外右侧。侧边栏自身不能加
 * `overflow-hidden`，那会把提示切掉。
 */
export default function Sidebar({ view, onViewChange }: SidebarProps) {
  const { theme, cycle } = useTheme();
  const ThemeIcon = THEME_ICON[theme];

  return (
    <nav
      aria-label="主导航"
      /* sticky + self-start：内容超高时页面滚动，侧边栏不会跟着滚走 */
      className="sticky top-0 z-10 flex h-screen w-[72px] shrink-0 flex-col items-center gap-1 border-r border-line bg-surface-1 py-3"
    >
      {/* 汇总排在监控采集**上面**（spec §2.1）：它是更常用的回顾入口，
          监控采集是「现在正在发生什么」。 */}
      <SidebarItem
        icon={BarChart3}
        label="汇总"
        active={view === "summary"}
        onClick={() => onViewChange("summary")}
      />
      <SidebarItem
        icon={Activity}
        label="监控采集"
        active={view === "monitor"}
        onClick={() => onViewChange("monitor")}
      />

      {/* 下层：工具。mt-auto 把它推到底，其余空间留给上层导航。 */}
      <div className="mt-auto flex flex-col items-center gap-1">
        <div aria-hidden className="my-1 h-px w-6 bg-line" />
        <SidebarItem
          icon={ThemeIcon}
          label={`主题：${THEME_LABEL[theme]}`}
          onClick={cycle}
        />
        <SidebarItem
          icon={Settings}
          label="设置"
          active={view === "settings"}
          onClick={() => onViewChange("settings")}
        />
      </div>
    </nav>
  );
}

interface ItemProps {
  icon: LucideIcon;
  /** 悬浮提示的文案。有意义的名字，不是图标本身 */
  label: string;
  onClick: () => void;
  /** 当前所在页面。工具类按钮（主题）没有这个状态 */
  active?: boolean;
}

function SidebarItem({ icon: Icon, label, onClick, active }: ItemProps) {
  return (
    /* group 让提示跟着按钮的 hover 出没；pointer-events-none 保证
       提示压在内容上时不会挡住后面的点击 */
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      aria-current={active ? "page" : undefined}
      className={`group relative flex size-11 cursor-pointer items-center justify-center rounded-md transition-colors duration-[--duration-fast] ${
        active
          ? "bg-nav-active text-ink"
          : "text-ink-muted hover:bg-surface-2 hover:text-ink"
      }`}
    >
      <Icon size={19} aria-hidden />

      <span
        role="tooltip"
        className="pointer-events-none absolute top-1/2 left-full z-20 ml-2 -translate-y-1/2 rounded-md border border-line-strong bg-surface-3 px-2 py-1 text-xs whitespace-nowrap text-ink opacity-0 shadow-pop transition-opacity duration-[--duration-fast] group-hover:opacity-100 group-focus-visible:opacity-100"
      >
        {label}
      </span>
    </button>
  );
}
