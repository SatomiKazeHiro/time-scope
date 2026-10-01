import { useEffect, useState } from "react";
import { parsePayload, type Segment, type SegmentTitle } from "../types";
import { formatDuration } from "../lib/bucket";
import { colorForCategory } from "./SegmentTimeline";

/**
 * 选中某个 ActivitySegment 后的详情（spec §10）。
 *
 * **关于窗口标题**：
 * 标题不在段上，而是存在 events 表里（一个段可能对应几十条事件、几十个标题）。
 * 点开时按 evidence id 反查，`redacted` 标记由**后端**判定——前端不硬编码
 * `[redacted]` 这个占位符，否则改了占位符就会出现"标记失效但数据仍脱敏"的怪状态。
 */
export default function SegmentDetail({ segment }: { segment: Segment | null }) {
  const [titles, setTitles] = useState<SegmentTitle[] | null>(null);
  const [titlesFailed, setTitlesFailed] = useState(false);

  const evidenceIds = segment?.evidenceEventIds ?? [];
  const key = segment?.id ?? "";

  useEffect(() => {
    if (!segment || evidenceIds.length === 0) {
      setTitles(null);
      setTitlesFailed(false);
      return;
    }
    let cancelled = false;
    setTitles(null);
    setTitlesFailed(false);
    getSegmentTitles(evidenceIds)
      .then((t) => {
        if (!cancelled) setTitles(t);
      })
      .catch(() => {
        if (!cancelled) setTitlesFailed(true);
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  if (!segment) {
    return <p style={{ color: "#666" }}>点击时间线上的色块查看详情。</p>;
  }

  const start = new Date(segment.startAt).toLocaleTimeString();
  const end = new Date(segment.endAt).toLocaleTimeString();
  const anyRedacted = (titles ?? []).some((t) => t.redacted);

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

        <TitlesRow titles={titles} failed={titlesFailed} hasEvidence={evidenceIds.length > 0} />

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
          {evidenceIds.length > 0
            ? `${evidenceIds.length} 条事件支撑`
            : "无（该段尚未落库）"}
        </dd>
      </dl>

      {anyRedacted && (
        <p style={{ margin: "8px 0 0", color: "#8a6d3b", fontSize: 12 }}>
          标有「已脱敏」的标题里，命中的部分在
          <strong>写入数据库之前</strong>就被替换成了
          <code>[redacted]</code>，原文从未落盘。
          在 <code>%APPDATA%\time-scope\rules.toml</code> 的{" "}
          <code>[[redact]]</code> 里调整规则，重启后对新数据生效。
        </p>
      )}
    </div>
  );
}

function TitlesRow({
  titles,
  failed,
  hasEvidence,
}: {
  titles: SegmentTitle[] | null;
  failed: boolean;
  hasEvidence: boolean;
}) {
  return (
    <>
      <dt style={{ color: "#666" }}>窗口标题</dt>
      <dd style={{ margin: 0, minWidth: 0 }}>
        <TitleList titles={titles} failed={failed} hasEvidence={hasEvidence} />
      </dd>
    </>
  );
}

function TitleList({
  titles,
  failed,
  hasEvidence,
}: {
  titles: SegmentTitle[] | null;
  failed: boolean;
  hasEvidence: boolean;
}) {
  if (failed) {
    return <span style={{ color: "#999" }}>（标题读取失败，其余信息不受影响）</span>;
  }
  if (titles === null) {
    return hasEvidence ? (
      <span style={{ color: "#999" }}>读取中…</span>
    ) : (
      <span style={{ color: "#999" }}>（该段尚未落库，还没有标题）</span>
    );
  }
  if (titles.length === 0) {
    return <span style={{ color: "#999" }}>（这段没有带标题的窗口事件）</span>;
  }
  return (
    <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
      {titles.map((t) => (
        <li
          key={t.title}
          style={{
            display: "flex",
            alignItems: "baseline",
            gap: 6,
            marginBottom: 2,
            wordBreak: "break-all",
          }}
        >
          <span>{t.title}</span>
          {t.redacted && (
            <span
              title="该标题的一部分在入库前被替换为 [redacted]"
              style={{
                flex: "none",
                fontSize: 11,
                padding: "0 5px",
                borderRadius: 3,
                border: "1px solid #d9b36c",
                background: "#fdf3e0",
                color: "#8a6d3b",
              }}
            >
              已脱敏
            </span>
          )}
          {t.count > 1 && (
            <span style={{ flex: "none", color: "#999", fontSize: 11 }}>× {t.count}</span>
          )}
        </li>
      ))}
    </ul>
  );
}

export async function getSegmentTitles(eventIds: string[]): Promise<SegmentTitle[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<SegmentTitle[]>("get_segment_titles", { eventIds });
}

/** @deprecated 旧的事件详情面板。UI 改用 SegmentDetail 后随之退役。 */
export function EventDetail({ event }: { event: import("../types").StoredEvent | null }) {
  if (!event) return <p style={{ color: "#666" }}>点击时间线上的色块查看详情。</p>;
  const p = parsePayload<import("../types").WindowFocusPayload>(event.payload);
  return (
    <div style={{ marginTop: 12 }}>
      <h2 style={{ fontSize: 15, margin: "0 0 6px" }}>{event.type}</h2>
      <p style={{ margin: 0 }}>
        {p ? `${p.process_name} — ${p.window_title ?? "（无标题）"}` : "无法解析"}
      </p>
    </div>
  );
}
