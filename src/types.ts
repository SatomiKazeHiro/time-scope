// Rust `StoredEvent` 的 TS 镜像。
// Rust 侧字段 `type_` 带 `#[serde(rename = "type")]`，所以 JSON 里是 `type`。

export interface StoredEvent {
  id: string;
  /** Unix 毫秒 */
  timestamp: number;
  type: EventType;
  /** `EventType` 变体的 JSON */
  payload: string;
}

export type EventType =
  | "window_focus"
  | "window_title_change"
  | "system_idle"
  | "system_resume"
  | "session_lock"
  | "session_unlock"
  | "input_heartbeat";

export interface WindowFocusPayload {
  process_name: string;
  window_title: string | null;
  exe_path: string | null;
}

export async function getEvents(date: string): Promise<StoredEvent[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<StoredEvent[]>("get_events", { date });
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

/** 事件类型 -> 展示色。骨架阶段按事件类型着色，分类色属 engine task。 */
export const EVENT_COLOR: Record<EventType, string> = {
  window_focus: "#4c8dff",
  window_title_change: "#7fb0ff",
  system_idle: "#9aa0a6",
  system_resume: "#66bb6a",
  session_lock: "#8e24aa",
  session_unlock: "#ab47bc",
  input_heartbeat: "#26a69a",
};

export function colorFor(type: string): string {
  return EVENT_COLOR[type as EventType] ?? "#bdbdbd";
}

/** 安全解析 payload：坏 JSON 不该让整页崩。 */
export function parsePayload<T>(raw: string): T | null {
  try {
    return JSON.parse(raw) as T;
  } catch {
    return null;
  }
}

function pad(n: number): string {
  return String(n).padStart(2, "0");
}
