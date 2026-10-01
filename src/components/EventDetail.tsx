import { parsePayload, type Segment } from "../types";
import { formatDuration } from "../lib/bucket";
import { colorForCategory } from "./SegmentTimeline";

/** 选中某个 ActivitySegment 后的详情（spec §10：起止时间、应用、类别、置信度、evidence 摘要）。 */
export default function SegmentDetail({ segment }: { segment: Segment | null }) {
  if (!segment) {
    return (
      <p style={{ color: "#666" }}>
        点击时间线上的色块查看详情。
      </p>
    );
  }

  const start = new Date(segment.startAt).toLocaleTimeString();
  const end = new Date(segment.endAt).toLocaleTimeString();
  const evidence = segment.evidenceEventIds.length;

  return (
    <div style={{ marginTop: 12 }}>
      <h2 style={{ fontSize: 15, margin: "0 0 6px" }}>
        <span
          aria-hidden
          style={{
            display: "inline-block",
            width: 10,
            height: 10,
            borderRadius: 2,
            marginRight: 6,
            background: colorForCategory(segment.category),
          }}
        />
        {segment.category}
      </h2>

      <dl
        style={{
          margin: 0,
          display: "grid",
          gridTemplateColumns: "auto 1fr",
          gap: "2px 12px",
        }}
      >
        <dt style={{ color: "#666" }}>时间</dt>
        <dd style={{ margin: 0 }}>
          {start} – {end}（{formatDuration(segment.endAt - segment.startAt)}）
        </dd>

        <dt style={{ color: "#666" }}>应用</dt>
        <dd style={{ margin: 0 }}>{segment.application ?? "（未知）"}</dd>

        <dt style={{ color: "#666" }}>置信度</dt>
        <dd style={{ margin: 0 }}>{segment.confidence.toFixed(2)}</dd>

        <dt style={{ color: "#666" }}>分类依据</dt>
        <dd style={{ margin: 0 }}>
          {segment.classifier === "rule"
            ? `规则 ${segment.classifierVersion}`
            : segment.classifier}
        </dd>

        <dt style={{ color: "#666" }}>证据</dt>
        <dd style={{ margin: 0 }}>
          {evidence > 0 ? `${evidence} 条事件支撑` : "无（该段尚未落库）"}
        </dd>
      </dl>
    </div>
  );
}

/** @deprecated 旧的事件详情面板。Task 10 起 UI 改用 SegmentDetail，随之退役。 */
export function EventDetail({ event }: { event: import("../types").StoredEvent | null }) {
  if (!event) return <p style={{ color: "#666" }}>点击时间线上的色块查看详情。</p>;
  const p = parsePayload<import("../types").WindowFocusPayload>(event.payload);
  return (
    <div style={{ marginTop: 12 }}>
      <h2 style={{ fontSize: 15, margin: "0 0 6px" }}>{event.type}</h2>
      <p style={{ margin: 0 }}>{p ? `${p.process_name} — ${p.window_title ?? "（无标题）"}` : "无法解析"}</p>
    </div>
  );
}
