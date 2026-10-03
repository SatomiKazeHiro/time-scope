import { useCallback, useEffect, useState } from "react";

/**
 * 主题：跟随系统 / 浅色 / 深色。
 *
 * 解析**完全交给 CSS**：这里只把用户的选择写成 `<html data-theme="...">`，
 * 实际切换靠 theme.css 里的媒体查询和 `[data-theme]` 覆盖块。
 * 所以系统主题在运行中变化时不需要 JS 监听 —— 媒体查询自己会跟上。
 *
 * 为什么不在 index.html 里放一段内联脚本先设好（那样能避免闪一下）：
 * 生产 CSP 是 `script-src 'self'`，内联脚本会被拦。见 theme.css 里
 * 浅色 token 写两遍的注释。
 */
export type Theme = "system" | "light" | "dark";

const STORAGE_KEY = "time-scope.theme";

/** 循环顺序：从"什么都不选"开始，依次试具体值，最后回到跟随系统。 */
const CYCLE: Theme[] = ["system", "light", "dark"];

export const THEME_LABEL: Record<Theme, string> = {
  system: "跟随系统",
  light: "浅色",
  dark: "深色",
};

function read(): Theme {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    return v === "light" || v === "dark" || v === "system" ? v : "system";
  } catch {
    // localStorage 在某些 WebView 上下文里可能被禁；主题只是偏好，失败就退回默认
    return "system";
  }
}

export function useTheme() {
  const [theme, setTheme] = useState<Theme>(read);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    try {
      localStorage.setItem(STORAGE_KEY, theme);
    } catch {
      /* 存不住不影响使用，本次会话内仍然生效 */
    }
  }, [theme]);

  const cycle = useCallback(() => {
    setTheme((cur) => CYCLE[(CYCLE.indexOf(cur) + 1) % CYCLE.length]);
  }, []);

  return { theme, cycle };
}
