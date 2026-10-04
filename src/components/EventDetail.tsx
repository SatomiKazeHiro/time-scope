import { useEffect, useState, type ReactNode } from "react";
import { AppWindow, Clock, Gauge, Layers, ShieldCheck } from "lucide-react";
import { parsePayload, type Segment, type SegmentTitle } from "../types";
import { formatDuration } from "../lib/bucket";
import { colorForCategory, metaForCategory } from "../design/categories";

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
    return (
      <p className="text-sm text-ink-faint">
        点击时间线上的色块查看详情。
      </p>
    );
  }

  const start = new Date(segment.startAt).toLocaleTimeString();
  const end = new Date(segment.endAt).toLocaleTimeString();
  const anyRedacted = (titles ?? []).some((t) => t.redacted);
  const meta = metaForCategory(segment.category);

  return (
    <section aria-label="段详情" className="panel p-4">
      <div className="mb-3 flex items-center gap-2">
        <span
          className="size-2.5 rounded-[2px] shadow-[inset_0_0_0_1px_var(--color-line-strong)]"
          style={{ background: colorForCategory(segment.category) }}
          aria-hidden
        />
        <h2 className="text-md font-semibold text-ink">{meta.label}</h2>
        {/* 原始 key 留着，方便和 rules.toml 对规则 */}
        <code className="rounded-sm bg-surface-2 px-1.5 py-0.5 text-micro text-ink-faint">
          {segment.category}
        </code>
      </div>

      <dl className="m-0 grid grid-cols-[auto_1fr] items-baseline gap-x-4 gap-y-2">
        <Field icon={<Clock size={13} aria-hidden />} label="时间">
          <span className="tnum">
            {start} – {end}
          </span>
          <span className="text-ink-faint">（{formatDuration(segment.endAt - segment.startAt)}）</span>
        </Field>

        <Field icon={<AppWindow size={13} aria-hidden />} label="应用">
          {segment.application ?? "（未知）"}
        </Field>

        <TitlesRow titles={titles} failed={titlesFailed} hasEvidence={evidenceIds.length > 0} />

        <Field icon={<Gauge size={13} aria-hidden />} label="置信度">
          <span className="tnum">{segment.confidence.toFixed(2)}</span>
        </Field>

        <Field icon={<Layers size={13} aria-hidden />} label="分类依据">
          {segment.classifier === "rule" ? `规则 ${segment.classifierVersion}` : segment.classifier}
        </Field>

        <Field icon={<Layers size={13} aria-hidden />} label="证据">
          {evidenceIds.length > 0
            ? `${evidenceIds.length} 条事件支撑`
            : "无（该段尚未落库）"}
        </Field>
      </dl>

      {anyRedacted && (
        <p className="mt-3 flex gap-2 rounded-md border border-line bg-surface-2 p-2.5 text-micro text-ink-muted">
          <ShieldCheck size={14} className="mt-0.5 shrink-0 text-state-warning" aria-hidden />
          <span>
            标有「已脱敏」的标题里，命中的部分在
            <strong className="text-ink">写入数据库之前</strong>就被替换成了
            <code className="tnum mx-0.5"> [redacted] </code>
            ，原文从未落盘。在{" "}
            <code className="tnum">%APPDATA%\time-scope\rules.toml</code> 的{" "}
            <code>[[redact]]</code> 里调整规则，重启后对新数据生效。
          </span>
        </p>
      )}
    </section>
  );
}

function Field({
  icon,
  label,
  children,
}: {
  icon: ReactNode;
  label: string;
  children: ReactNode;
}) {
  return (
    <>
      <dt className="flex items-center gap-1.5 text-label text-ink-faint">
        <span className="text-ink-ghost">{icon}</span>
        {label}
      </dt>
      <dd className="m-0 min-w-0 text-sm break-words text-ink">{children}</dd>
    </>
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
  // 列表限高滚动，但不给提示的话用户只会看到内容被截断，不知道下面还有
  const hint = titles !== null && !failed && titles.length > 1 ? titles.length : 0;
  return (
    <Field icon={<Layers size={13} aria-hidden />} label="窗口标题">
      <TitleList titles={titles} failed={failed} hasEvidence={hasEvidence} />
      {hint > 0 && (
        <span className="tnum mt-1 block text-micro text-ink-ghost">{hint} 条 · 可滚动</span>
      )}
    </Field>
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
    return <span className="text-ink-faint">（标题读取失败，其余信息不受影响）</span>;
  }
  if (titles === null) {
    return hasEvidence ? (
      <span className="text-ink-faint">读取中…</span>
    ) : (
      <span className="text-ink-faint">（该段尚未落库，还没有标题）</span>
    );
  }
  if (titles.length === 0) {
    return <span className="text-ink-faint">（这段没有带标题的窗口事件）</span>;
  }
  return (
    <ul className="m-0 flex max-h-40 list-none flex-col gap-1 overflow-y-auto p-0 pr-1">
      {titles.map((t) => (
        <li key={t.title} className="flex flex-wrap items-baseline gap-1.5">
          <span className="min-w-0 break-all">{t.title}</span>
          {t.redacted && (
            <span
              title="该标题的一部分在入库前被替换为 [redacted]"
              className="inline-flex shrink-0 items-center gap-1 rounded-sm border border-line-strong bg-surface-2 px-1.5 py-px text-micro text-state-warning"
            >
              <ShieldCheck size={10} aria-hidden />
              已脱敏
            </span>
          )}
          {t.count > 1 && (
            <span className="tnum shrink-0 text-micro text-ink-ghost">× {t.count}</span>
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
  if (!event) return <p className="text-sm text-ink-faint">点击时间线上的色块查看详情。</p>;
  const p = parsePayload<import("../types").WindowFocusPayload>(event.payload);
  return (
    <div className="mt-3">
      <h2 className="text-md font-semibold text-ink">{event.type}</h2>
      <p className="m-0 text-sm text-ink-muted">
        {p ? `${p.process_name} — ${p.window_title ?? "（无标题）"}` : "无法解析"}
      </p>
    </div>
  );
}
