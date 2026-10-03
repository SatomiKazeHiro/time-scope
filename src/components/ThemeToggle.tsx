import { Monitor, Moon, Sun } from "lucide-react";
import { THEME_LABEL, type Theme } from "../design/useTheme";

const ICON = { system: Monitor, light: Sun, dark: Moon } as const;

/**
 * 主题切换：一次点击走到下一个（跟随系统 → 浅色 → 深色 → 跟随系统）。
 *
 * 图标表示**当前**状态，不是点击后的结果 —— 三态循环里"下一步"会随当前值变，
 * 用一个固定图标（只显示下一步）反而更难预测。
 */
export default function ThemeToggle({
  theme,
  onCycle,
}: {
  theme: Theme;
  onCycle: () => void;
}) {
  const Icon = ICON[theme];
  return (
    <button
      type="button"
      onClick={onCycle}
      title={`主题：${THEME_LABEL[theme]}`}
      aria-label={`主题：${THEME_LABEL[theme]}，点击切换`}
      className="flex size-7 cursor-pointer items-center justify-center rounded-sm text-ink-muted transition-colors duration-[--duration-fast] hover:bg-surface-2 hover:text-ink"
    >
      <Icon size={14} aria-hidden />
    </button>
  );
}
