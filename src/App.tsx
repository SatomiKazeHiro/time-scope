import { useEffect, useMemo, useState, type ReactNode } from "react";
import { CalendarDays, ChevronLeft, ChevronRight, CircleAlert, Radio } from "lucide-react";
import SegmentTimeline from "./components/SegmentTimeline";
import GranularityPicker from "./components/GranularityPicker";
import DaySummary from "./components/DaySummary";
import SegmentDetail from "./components/EventDetail";
import ThemeToggle from "./components/ThemeToggle";
import MetricPicker, { ScaleLegend } from "./components/MetricPicker";
import { type MetricMode } from "./components/SegmentTimeline";
import { useTheme } from "./design/useTheme";
import { sliceSegments, DEFAULT_GRANULARITY } from "./lib/bucket";
import { bucketMetrics } from "./lib/metrics";
import { getSegments, shiftDate, todayString, type Segment } from "./types";

type Status = "loading" | "ok" | "error";

/** 自动刷新间隔。spec §9 的 `segment-updated` 推送尚未实现，先用轮询顶上。 */
const REFRESH_MS = 5_000;

export default function App() {
  const [date, setDate] = useState(todayString());
  const [segments, setSegments] = useState<Segment[]>([]);
  const [selected, setSelected] = useState<Segment | null>(null);
  /** 指标模式下被点中的那一格 */
  const [selectedBucket, setSelectedBucket] = useState<number | null>(null);
  const [status, setStatus] = useState<Status>("loading");
  const [granularity, setGranularity] = useState<number>(DEFAULT_GRANULARITY);
  const { theme, cycle } = useTheme();
  const [metric, setMetric] = useState<MetricMode>("category");
  // 切模式时清掉按另一套语义选中的东西，免得留下一个圈不到任何东西的环
  const switchMetric = (m: MetricMode) => {
    setMetric(m);
    setSelectedBucket(null);
  };

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
  const intervalMs = granularity * 60_000;
  const isMetric = metric !== "category";
  // 类别模式说"切了几块"，指标模式说"全天平均专注度 / 共切换多少次"
  const pieceCount = useMemo(
    () => (isMetric ? 0 : sliceSegments(segments, intervalMs).length),
    [segments, intervalMs, isMetric],
  );
  const dayStats = useMemo(() => {
    if (!isMetric || segments.length === 0) return null;
    const bs = bucketMetrics(segments, dayStartMs, intervalMs);
    // 专注度只对**有活动**的桶求加权平均。挂机两小时不该把当天的专注度拉高或拉低
    // ——那既不是"专注"也不是"分心"，是"没在工作"，它由活跃/空闲比去说。
    const active = bs.filter((b) => b.activeMs > 0);
    const switches = bs.reduce((s, b) => s + b.switches, 0);
    if (active.length === 0) {
      return { focusPct: null, switches };
    }
    const weight = active.reduce((s, b) => s + b.activeMs, 0);
    return {
      // 按非空闲时长加权；别忘了 ×100 —— 少了这一步 1.0 会被 round 成「1%」
      focusPct: Math.round(
        (active.reduce((s, b) => s + b.focus * b.activeMs, 0) / weight) * 100,
      ),
      switches,
    };
  }, [isMetric, segments, dayStartMs, intervalMs]);

  const isToday = date === todayString();

  return (
    /* h-full 而不是 min-h-full：给 flex 链一个确定的高度，底部那行才能真正
       收缩并在内部滚动。用 min-h-full 时内容只会把整页顶高。 */
    <div className="flex h-full flex-col gap-4 overflow-hidden bg-surface-0 p-4">
      <header className="flex items-center gap-3">
        <h1 className="m-0 text-lg font-semibold tracking-tight text-ink">Time Scope</h1>
        <LiveBadge status={status} />
        <ThemeToggle theme={theme} onCycle={cycle} />
        <span className="ml-auto tnum text-sm text-ink-faint">
          {status === "ok" && metric === "category" && (
            <>
              {pieceCount > segments.length
                ? `${segments.length} 段 → ${pieceCount} 块（${granularity} 分）`
                : `${segments.length} 段（${granularity} 分）`}
            </>
          )}
          {status === "ok" && metric === "focus" && dayStats?.focusPct !== null && (
            <>平均专注度 {dayStats?.focusPct}%（{granularity} 分一格）</>
          )}
          {status === "ok" && metric === "switch" && dayStats && (
            <>全天切换 {dayStats.switches} 次（{granularity} 分一格）</>
          )}
        </span>
      </header>

      <Toolbar
        date={date}
        isToday={isToday}
        granularity={granularity}
        metric={metric}
        onDate={setDate}
        onGranularity={setGranularity}
        onMetric={switchMetric}
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
              {/* 图例常驻：连续量没有图例就读不出数值。不能因为选中就让它消失 */}
              <ScaleLegend metric={metric} />
              {selected && (
                <span className="text-micro text-ink-faint">已选中 · 再次点击取消</span>
              )}
            </div>
            <SegmentTimeline
              segments={segments}
              dayStartMs={dayStartMs}
              onSelect={(s, bucketIndex) => {
                setSelected((cur) => (cur?.id === s.id ? null : s));
                setSelectedBucket((cur) => (cur === bucketIndex ? null : (bucketIndex ?? null)));
              }}
              selectedId={selected?.id ?? null}
              selectedBucket={selectedBucket}
              showNow={isToday}
              intervalMs={intervalMs}
              metric={metric}
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
  metric,
  onDate,
  onGranularity,
  onMetric,
}: {
  date: string;
  isToday: boolean;
  granularity: number;
  metric: MetricMode;
  onDate: (d: string) => void;
  onGranularity: (m: number) => void;
  onMetric: (m: MetricMode) => void;
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

      {/* 指标和粒度挨着放：指标模式下粒度就是桶宽，两个控件是同一件事的两面 */}
      <div className="ml-auto flex items-center gap-2">
        <MetricPicker value={metric} onChange={onMetric} />
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
