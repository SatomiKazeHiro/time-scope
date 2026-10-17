/** Rust `StoredSegment` 的镜像。Rust 侧是 camelCase 序列化。 */
export interface ActivitySegment {
  id: string;
  startAt: number;
  endAt: number;
  category: Category;
  application: string | null;
  confidence: number;
  classifier: string;
  classifierVersion: string;
  evidenceEventIds: string[];
  /**
   * 这段里用户确实有输入的时长（毫秒），2026-10-13 新增。
   *
   * 专注度的分母（`focus = 主导类别时长 / 有输入时长`）。这样「人不在」
   * （锁屏 / 长时间无输入）不会被读成「高度专注」。
   *
   * `null` = 2026-10-13 之前的历史数据，没有这个量；`0` = 这段确实
   * 没有输入。两者必须分开 —— 都当 0 会让历史日期看起来像「完全没人用电脑」。
   */
  activeMs?: number | null;
}

export type Category =
  | "work" | "study" | "entertainment" | "communication"
  | "browsing" | "life" | "idle" | "unknown";

/** 一条去重后的窗口标题。后端已判定 redacted，前端不硬编码占位符。 */
export interface SegmentTitle {
  title: string;
  /** 该标题是否经过脱敏（含有占位符） */
  redacted: boolean;
  /** 在证据里出现了多少次 */
  count: number;
}

/** ActivitySegment 的短别名，前端组件里用起来更顺。 */
export type Segment = ActivitySegment;

export async function getSegments(date: string): Promise<ActivitySegment[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<ActivitySegment[]>("get_segments", { date });
}

/**
 * 按 evidence id 反查某个段的窗口标题。
 *
 * 标题不在段上而在 events 表里，所以这一条是懒加载的（点开段详情才调）。
 * 它原来住在 `components/EventDetail.tsx`，是 `types.ts` 之外唯一一个 IPC 封装 ——
 * IPC 边界集中在一个文件里，改后端命令名时只需要搜一处。
 */
export async function getSegmentTitles(eventIds: string[]): Promise<SegmentTitle[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<SegmentTitle[]>("get_segment_titles", { eventIds });
}

export function todayString(d: Date = new Date()): string {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** 在 YYYY-MM-DD 上加减天数（走 Date 的本地时区解析，避开时区偏移）。 */
export function shiftDate(date: string, days: number): string {
  const [y, m, d] = date.split("-").map(Number);
  const dt = new Date(y, m - 1, d);
  dt.setDate(dt.getDate() + days);
  return todayString(dt);
}

function pad(n: number): string {
  return String(n).padStart(2, "0");
}

/* ---------- 汇总页（spec 2026-10-05） ---------- */

/** 热力图的一格：某一天的监控总时长（含 idle）。 */
export interface DayCell {
  date: string;
  totalMs: number;
}

/**
 * 热力图的数据源。`days` 只含有记录的日期 ——
 * 空缺日期由 `lib/summary.ts` 的 `fillDays` 补齐（后端的 `GROUP BY`
 * 会跳过没开机的那天，直接用它的结果排格子会整片错位）。
 */
export interface DailyCalendar {
  first: string;
  last: string;
  days: DayCell[];
}

/** 圆环的档位。后端恒返回这四项、顺序固定。 */
export type DonutKey = "work" | "browsing" | "idle" | "unknown";

export interface DonutSlice {
  key: DonutKey;
  ms: number;
}

export interface AppSlice {
  name: string;
  ms: number;
}

/** 顶部指标。`from` / `to` 都是 `YYYY-MM-DD` 的闭区间。 */
export interface Summary {
  totalMs: number;
  activeMs: number;
  idleMs: number;
  segmentCount: number;

  /** 24 个桶，按本地小时切。只含非 idle 时长。 */
  hourlyMs: number[];
  donut: DonutSlice[];
  topApps: AppSlice[];
}

/** 归一化后的窗口标题。`redacted` 由后端判定，前端不硬编码占位符。 */
export interface MergedTitle {
  title: string;
  hits: number;
  redacted: boolean;
}

/** 热力图的数据。无参数 —— 热力图永远渲染全部数据，不受选中范围影响。 */
export async function getDailyCalendar(): Promise<DailyCalendar | null> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<DailyCalendar | null>("get_daily_calendar");
}

export async function getSummary(from: string, to: string): Promise<Summary> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<Summary>("get_summary", { from, to });
}

export async function getTopTitles(
  from: string,
  to: string,
  limit = 10,
): Promise<MergedTitle[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<MergedTitle[]>("get_top_titles", { from, to, limit });
}

/**
 * 一个「有记录但没规则覆盖」的程序。
 *
 * `shareOfUncovered` 的分母是未覆盖总时长，不是全天 ——
 * 用户打开设置页要回答的是「我漏配的时间里有多少给了它」。
 * 与后端 `uncovered_programs` 的注释同款。
 */
export interface UncoveredProgram {
  application: string;
  totalMs: number;
  shareOfUncovered: number;
  segmentCount: number;
}

export async function getUncoveredPrograms(days = 30): Promise<UncoveredProgram[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<UncoveredProgram[]>("get_uncovered_programs", { days });
}

/** 给一个程序加分类规则。立即生效，不需要重启。 */
export async function addRule(application: string, category: Category): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke<void>("add_rule", { application, category });
}

/** `config.toml` 的前端镜像。字段与 Rust 的 `AppConfig` 一一对应。 */
export interface AppConfig {
  idleThresholdS: number;
  minSegmentDurationS: number;
  heartbeatEveryS: number;
  autostart: boolean;
  closeBehavior: string;
  eventsRetentionDays: number;
  /**
   * 每个 `config.toml` 键改完是否要重启，键用 toml 里的原名。
   *
   * 后端给（`get_config` 一并返回），前端不自己列 —— 复制一份表就多一处
   * 会跟实现对不上的地方，而那种对不上表现为「界面说不用重启、实际要重启」。
   * 2026-10-13 那次界面说谎就是两份表一致地错造成的。
   */
  needsRestart: Record<string, boolean>;
}

export async function getConfig(): Promise<AppConfig> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<AppConfig>("get_config");
}

/**
 * 改一个配置项，返回是否需要重启才生效。
 *
 * 这个判定由后端给出（依据是参数绑在哪里），前端不自己猜 ——
 * 复制一份表就多一处会跟实现对不上的地方，而那种对不上
 * 表现为「界面说不用重启、实际要重启」，最难查。
 */
export async function setConfig(key: string, value: string): Promise<boolean> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<boolean>("set_config", { key, value });
}
