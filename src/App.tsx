import { useEffect, useMemo, useState } from "react";
import SegmentTimeline from "./components/SegmentTimeline";
import GranularityPicker from "./components/GranularityPicker";
import DaySummary from "./components/DaySummary";
import SegmentDetail from "./components/EventDetail";
import { bucketSegments, DEFAULT_GRANULARITY } from "./lib/bucket";
import { getSegments, shiftDate, todayString, type Segment } from "./types";

type Status = "loading" | "ok" | "error";

export default function App() {
  const [date, setDate] = useState(todayString());
  const [segments, setSegments] = useState<Segment[]>([]);
  const [selected, setSelected] = useState<Segment | null>(null);
  const [status, setStatus] = useState<Status>("loading");
  const [granularity, setGranularity] = useState<number>(DEFAULT_GRANULARITY);

  useEffect(() => {
    let cancelled = false;
    setStatus("loading");
    getSegments(date)
      .then((segs) => {
        if (cancelled) return;
        setSegments(segs);
        setSelected(null);
        setStatus("ok");
      })
      .catch(() => {
        if (!cancelled) setStatus("error");
      });
    return () => {
      cancelled = true;
    };
  }, [date]);

  // 该日 00:00 的本地毫秒；与 Rust 侧 day_range_ms 用同一个本地时区口径
  const dayStartMs = useMemo(() => {
    const [y, m, d] = date.split("-").map(Number);
    return new Date(y, m - 1, d).getTime();
  }, [date]);

  // spec §8.2：分桶在前端做，切换粒度不重查后端
  const buckets = useMemo(
    () => bucketSegments(segments, granularity * 60_000),
    [segments, granularity],
  );

  const isToday = date === todayString();

  return (
    <main style={{ padding: 16, fontFamily: "system-ui, sans-serif" }}>
      <h1 style={{ fontSize: 20, margin: "0 0 12px" }}>Time Scope</h1>

      <div
        style={{
          display: "flex",
          gap: 8,
          alignItems: "center",
          marginBottom: 12,
          flexWrap: "wrap",
        }}
      >
        <button type="button" onClick={() => setDate(shiftDate(date, -1))}>
          ← 前一天
        </button>
        <input type="date" value={date} onChange={(e) => setDate(e.target.value)} />
        <button type="button" onClick={() => setDate(shiftDate(date, 1))}>
          后一天 →
        </button>
        {!isToday && (
          <button type="button" onClick={() => setDate(todayString())}>
            回到今天
          </button>
        )}
        <span style={{ marginLeft: "auto", color: "#666" }}>
          {status === "ok" &&
            `${segments.length} 段 · ${buckets.length} 桶（${granularity} 分）`}
        </span>
      </div>

      {status === "loading" && <p style={{ color: "#666" }}>加载中…</p>}
      {status === "error" && (
        <p role="alert" style={{ color: "#c00" }}>
          加载失败。请确认后端已启动（<code>pnpm tauri dev</code>）。
        </p>
      )}

      {status === "ok" && (
        <>
          <div style={{ marginBottom: 8 }}>
            <GranularityPicker value={granularity} onChange={setGranularity} />
          </div>

          <SegmentTimeline
            segments={segments}
            dayStartMs={dayStartMs}
            onSelect={setSelected}
          />

          <DaySummary segments={segments} dayStartMs={dayStartMs} />

          <SegmentDetail segment={selected} />
        </>
      )}
    </main>
  );
}
