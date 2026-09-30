import { useCallback, useEffect, useMemo, useState } from "react";
import Timeline from "./components/Timeline";
import EventDetail from "./components/EventDetail";
import { getEvents, shiftDate, todayString, type StoredEvent } from "./types";

type Status = "loading" | "ok" | "error";

export default function App() {
  const [date, setDate] = useState(todayString());
  const [events, setEvents] = useState<StoredEvent[]>([]);
  const [selected, setSelected] = useState<StoredEvent | null>(null);
  const [status, setStatus] = useState<Status>("loading");

  const reload = useCallback((d: string) => {
    let cancelled = false;
    setStatus("loading");
    getEvents(d)
      .then((evts) => {
        if (cancelled) return;
        setEvents(evts);
        setStatus("ok");
      })
      .catch(() => {
        if (!cancelled) setStatus("error");
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => reload(date), [date, reload]);

  // 该日 00:00 的本地毫秒；与 Rust 侧 day_range_ms 用同一个本地时区口径
  const dayStartMs = useMemo(() => {
    const [y, m, d] = date.split("-").map(Number);
    return new Date(y, m - 1, d).getTime();
  }, [date]);

  const isToday = date === todayString();

  return (
    <main style={{ padding: 16, fontFamily: "system-ui, sans-serif" }}>
      <h1 style={{ fontSize: 20, margin: "0 0 12px" }}>Time Scope</h1>

      <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 12 }}>
        <button onClick={() => setDate(shiftDate(date, -1))}>← 前一天</button>
        <input type="date" value={date} onChange={(e) => setDate(e.target.value)} />
        <button onClick={() => setDate(shiftDate(date, 1))}>后一天 →</button>
        {!isToday && (
          <button onClick={() => setDate(todayString())}>回到今天</button>
        )}
        <span style={{ color: "#666", marginLeft: "auto" }}>
          {status === "ok" && `${events.length} 条事件`}
        </span>
      </div>

      {status === "loading" && <p style={{ color: "#666" }}>加载中…</p>}
      {status === "error" && (
        <p role="alert" style={{ color: "#c00" }}>
          加载失败。请确认后端已启动（`pnpm tauri dev`）。
        </p>
      )}
      {status === "ok" && (
        <>
          {events.length === 0 ? (
            <p style={{ color: "#666" }}>
              这一天还没有采集到事件。应用需要运行并切换过窗口。
            </p>
          ) : (
            <Timeline
              events={events}
              dayStartMs={dayStartMs}
              onSelect={setSelected}
            />
          )}
          <EventDetail event={selected} />
        </>
      )}
    </main>
  );
}
