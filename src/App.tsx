import { useEffect, useMemo, useState, type ReactNode } from "react";
import { CalendarDays, ChevronLeft, ChevronRight, CircleAlert, Radio } from "lucide-react";
import SegmentTimeline from "./components/SegmentTimeline";
import GranularityPicker from "./components/GranularityPicker";
import DaySummary from "./components/DaySummary";
import SegmentDetail from "./components/EventDetail";
import ThemeToggle from "./components/ThemeToggle";
import { useTheme } from "./design/useTheme";
import { bucketSegments, DEFAULT_GRANULARITY } from "./lib/bucket";
import { getSegments, shiftDate, todayString, type Segment } from "./types";

type Status = "loading" | "ok" | "error";

/** 自动刷新间隔。spec §9 的 `segment-updated` 推送尚未实现，先用轮询顶上。 */
const REFRESH_MS = 5_000;

export default function App() {
  const [date, setDate] = useState(todayString());
  const [segments, setSegments] = useState<Segment[]>([]);
  const [selected, setSelected] = useState<Segment | null>(null);
  const [status, setStatus] = useState<Status>("loading");
  const [granularity, setGranularity] = useState<number>(DEFAULT_GRANULARITY);
  const { theme, cycle } = useTheme();

  useEffect(() => {
    let cancelled = false;
    setStatus("loading");

    const load = () => {
      getSegments(date)
        .then((segs) => {
          if (cancelled) return;
          setSegments(segs);
          setStatus("ok");
        })
        .catch(() => {
          if (!cancelled) setStatus("error");
        });
    };

    load();
    // 后台引擎一直在产出新段，不轮询的话界面就是一张静止的图——
    // 看着像程序坏了。5s 足够跟手，又不至于给 DB 和 IPC 添压力。
    const timer = setInterval(load, REFRESH_MS);

    return () => {
      cancelled = true;
      clearInterval(timer);
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
    <div className="flex min-h-full flex-col gap-4 bg-surface-0 p-4">
      <header className="flex items-center gap-3">
        <h1 className="m-0 text-lg font-semibold tracking-tight text-ink">Time Scope</h1>
        <LiveBadge status={status} />
        <ThemeToggle theme={theme} onCycle={cycle} />
        <span className="ml-auto tnum text-sm text-ink-faint">
          {status === "ok" && `${segments.length} 段 · ${buckets.length} 桶（${granularity} 分）`}
        </span>
      </header>

      <Toolbar
        date={date}
        isToday={isToday}
        granularity={granularity}
        onDate={setDate}
        onGranularity={setGranularity}
      />

      {status === "loading" && <p className="text-sm text-ink-faint">加载中…</p>}

      {status === "error" && (
        <div
          role="alert"
          className="flex items-start gap-2 rounded-md border border-line bg-surface-1 p-3 text-sm text-ink"
        >
          <CircleAlert size={15} className="mt-0.5 shrink-0 text-state-critical" aria-hidden />
          <span>
            加载失败。请确认后端已启动（<code className="tnum">pnpm tauri dev</code>）。
          </span>
        </div>
      )}

      {status === "ok" && (
        <>
          <section className="panel p-4" aria-label="时间线">
            <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
              <h2 className="panel-title">24 小时时间线</h2>
              {selected && (
                <span className="text-micro text-ink-faint">
                  已选中一段 · 再次点击可取消
                </span>
              )}
            </div>
            <SegmentTimeline
              segments={segments}
              dayStartMs={dayStartMs}
              onSelect={(s) => setSelected((cur) => (cur?.id === s.id ? null : s))}
              selectedId={selected?.id ?? null}
              showNow={isToday}
            />
          </section>

          {/* 1200×700 的窗口里，汇总与详情并排比上下堆更省纵向空间 */}
          <div className="grid min-h-0 flex-1 grid-cols-1 gap-4 lg:grid-cols-2">
            <DaySummary segments={segments} dayStartMs={dayStartMs} />
            <SegmentDetail segment={selected} />
          </div>
        </>
      )}
    </div>
  );
}

/** 采集中指示。用状态色而不是某个类别色相 —— 状态色是保留语义。 */
function LiveBadge({ status }: { status: Status }) {
  const live = status === "ok";
  return (
    <span className="flex items-center gap-1.5 rounded-full border border-line bg-surface-1 px-2 py-0.5 text-micro text-ink-muted">
      <Radio
        size={11}
        aria-hidden
        className={live ? "text-state-good" : "text-ink-ghost"}
      />
      {live ? "采集中" : status === "error" ? "已断开" : "连接中"}
    </span>
  );
}

function Toolbar({
  date,
  isToday,
  granularity,
  onDate,
  onGranularity,
}: {
  date: string;
  isToday: boolean;
  granularity: number;
  onDate: (d: string) => void;
  onGranularity: (m: number) => void;
}) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <div className="flex items-center gap-1 rounded-md border border-line bg-surface-1 p-0.5">
        <IconButton label="前一天" onClick={() => onDate(shiftDate(date, -1))}>
          <ChevronLeft size={15} aria-hidden />
        </IconButton>

        <label className="relative flex items-center">
          <CalendarDays
            size={13}
            aria-hidden
            className="pointer-events-none absolute left-2 text-ink-ghost"
          />
          <span className="sr-only">选择日期</span>
          <input
            type="date"
            value={date}
            onChange={(e) => onDate(e.target.value)}
            className="tnum cursor-pointer rounded-sm bg-transparent py-1 pl-7 pr-2 text-sm text-ink hover:bg-surface-2 [color-scheme:dark]"
          />
        </label>

        <IconButton label="后一天" onClick={() => onDate(shiftDate(date, 1))}>
          <ChevronRight size={15} aria-hidden />
        </IconButton>
      </div>

      {!isToday && (
        <button
          type="button"
          onClick={() => onDate(todayString())}
          className="cursor-pointer rounded-md border border-line bg-surface-1 px-2.5 py-1.5 text-sm text-ink-muted transition-colors duration-[--duration-fast] hover:bg-surface-2 hover:text-ink"
        >
          回到今天
        </button>
      )}

      <div className="ml-auto">
        <GranularityPicker value={granularity} onChange={onGranularity} />
      </div>
    </div>
  );
}

function IconButton({
  label,
  onClick,
  children,
}: {
  label: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      className="flex size-7 cursor-pointer items-center justify-center rounded-sm text-ink-muted transition-colors duration-[--duration-fast] hover:bg-surface-2 hover:text-ink"
    >
      {children}
    </button>
  );
}
