import { parsePayload, type StoredEvent, type WindowFocusPayload } from "../types";

const TYPE_LABEL: Record<string, string> = {
  window_focus: "切换到窗口",
  window_title_change: "窗口标题变化",
  system_idle: "开始空闲",
  system_resume: "恢复活动",
  session_lock: "锁屏",
  session_unlock: "解锁",
  input_heartbeat: "输入心跳",
};

export default function EventDetail({ event }: { event: StoredEvent | null }) {
  if (!event) {
    return <p style={{ color: "#666" }}>点击时间线上的色块查看详情。</p>;
  }

  const when = new Date(event.timestamp).toLocaleString();
  const label = TYPE_LABEL[event.type] ?? event.type;

  return (
    <div style={{ marginTop: 12 }}>
      <h2 style={{ fontSize: 15, margin: "0 0 6px" }}>{label}</h2>
      <p style={{ margin: "0 0 4px", color: "#555" }}>{when}</p>

      {event.type === "window_focus" || event.type === "window_title_change" ? (
        <WindowDetail raw={event.payload} />
      ) : (
        <HeartbeatDetail raw={event.payload} />
      )}
    </div>
  );
}

function WindowDetail({ raw }: { raw: string }) {
  const p = parsePayload<WindowFocusPayload>(raw);
  if (!p) {
    return <p style={{ color: "#c00" }}>无法解析该事件的内容。</p>;
  }
  return (
    <dl style={{ margin: 0, display: "grid", gridTemplateColumns: "auto 1fr", gap: "2px 12px" }}>
      <dt style={{ color: "#666" }}>进程</dt>
      <dd style={{ margin: 0 }}>{p.process_name}</dd>
      <dt style={{ color: "#666" }}>标题</dt>
      {/* null 标题必须显示成占位而不是空白（Review Focus #4） */}
      <dd style={{ margin: 0 }}>{p.window_title ?? "（无标题）"}</dd>
      {p.exe_path && (
        <>
          <dt style={{ color: "#666" }}>路径</dt>
          <dd style={{ margin: 0, wordBreak: "break-all" }}>{p.exe_path}</dd>
        </>
      )}
    </dl>
  );
}

function HeartbeatDetail({ raw }: { raw: string }) {
  const p = parsePayload<{ active_seconds?: number }>(raw);
  const secs = p?.active_seconds;
  return (
    <p style={{ margin: 0 }}>
      {typeof secs === "number" ? `本窗口内活跃 ${secs} 秒` : "—"}
    </p>
  );
}
