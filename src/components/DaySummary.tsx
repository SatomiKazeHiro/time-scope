import { formatDuration, summarize } from "../lib/bucket";
import { colorForCategory } from "./SegmentTimeline";
import type { Segment } from "../types";

interface Props {
  segments: Segment[];
  dayStartMs: number;
}

/** 当日汇总：各类别时长与占比（spec §10）。数据源是 `get_segments` 的原始段，后端不另给 summary。 */
export default function DaySummary({ segments, dayStartMs }: Props) {
  const rows = summarize(segments, dayStartMs);
  if (rows.length === 0) return null;

  return (
    <section aria-label="当日汇总" style={{ marginTop: 16 }}>
      <h2 style={{ fontSize: 14, margin: "0 0 8px" }}>当日汇总</h2>
      <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
        {rows.map((r) => (
          <li
            key={r.category}
            style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 4 }}
          >
            <span style={{ width: 110, color: "#555" }}>{r.category}</span>
            <div
              style={{
                flex: 1,
                height: 10,
                background: "#eceff1",
                borderRadius: 5,
                overflow: "hidden",
              }}
            >
              <div
                style={{
                  width: `${Math.max(Math.round(r.ratio * 100), 1)}%`,
                  height: "100%",
                  background: colorForCategory(r.category),
                }}
              />
            </div>
            <span style={{ width: 90, textAlign: "right", color: "#666" }}>
              {formatDuration(r.durationMs)}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}
