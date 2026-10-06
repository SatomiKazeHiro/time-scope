import { useEffect, useMemo, useState, type ReactNode } from "react";
import { CalendarDays, ChevronLeft, ChevronRight, CircleAlert, Radio } from "lucide-react";
import SegmentTimeline, { type MetricMode } from "./components/SegmentTimeline";
import GranularityPicker from "./components/GranularityPicker";
import DaySummary from "./components/DaySummary";
import SegmentDetail from "./components/EventDetail";
import MetricPicker, { ScaleLegend } from "./components/MetricPicker";
import SettingsPage from "./components/SettingsPage";
import Sidebar, { type View } from "./components/Sidebar";
import SummaryPage from "./views/SummaryPage";
import { DEFAULT_GRANULARITY } from "./lib/bucket";
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
  const [metric, setMetric] = useState<MetricMode>("category");
  const [view, setView] = useState<View>("monitor");
  // 切模式时清掉按另一套语义选中的东西，免得留下一个圈不到任何东西的环
  const switchMetric = (m: MetricMode) => {
    setMetric(m);
    setSelectedBucket(null);
  };
  // 切粒度同理：`selectedBucket` 存的是**桶下标**，而下标是粒度的函数
  // （30 分的第 18 格是 09:00–09:30，60 分的第 18 格是 18:00–19:00）。
  // 不复位的话选中框会静默挪到另一个时间段；再点那一格还会被当成
  // 「再次点击取消」，点下去毫无反应（B7）。
  //
  // 只清桶选中，**不清 `selected`**：段是引擎判定的活动边界，与粒度无关，
  // 段详情面板不该因为用户调了一下粒度就自己清空。
  const switchGranularity = (g: number) => {
    setGranularity(g);
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
  // 类别模式只报段数（粒度在那儿只管刻度尺），指标模式报当天读数
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
    /* min-h-screen：内容超了就让**整页**滚。原先用 h-full + 面板各自滚，
       结果窄窗口下出现三层嵌套滚动条，每个面板只剩一两行 —— 不如一根
       页面滚动条来得自然。700px 高时内容本来就装得下，不会出现滚动。
       Sidebar 自己 sticky，不跟着页面滚走。 */
    <div className="flex min-h-screen bg-surface-0">
      <Sidebar view={view} onViewChange={setView} />

      <main className="flex min-w-0 flex-1 flex-col gap-3 p-4">
        {view === "settings" ? (
          <SettingsPage />
        ) : view === "summary" ? (
          <SummaryPage />
        ) : (
          <>
            {/* 身份 | 控件 —— 一行。原来分三行，700px 的窗口里 260px（37%）
                用在数据之前。托盘常驻的单窗口应用没有导航可放，一条工具栏是它的常态。

                **必须能换行。** 之前给两组都加了 shrink-0，结果窄窗口下 120分
                直接被裁掉、整条栏溢出。宽窗口下它自然排成一行，窄窗口下"指标+粒度"
                整体落到第二行，比哪个按钮被切掉强。 */}
            <div className="flex shrink-0 flex-wrap items-center gap-x-2 gap-y-2">
              {/* 应用名降级成安静标识：托盘里已经常驻，20px 的 h1 是抢戏。
                  whitespace-nowrap 必需：不加的话窄窗口下会被逐字折成竖排。 */}
              <h1 className="m-0 shrink-0 text-label font-semibold tracking-wide whitespace-nowrap text-ink-muted">
                Time Scope
              </h1>
              <LiveBadge status={status} />

              <span aria-hidden className="mx-1 hidden h-5 w-px shrink-0 bg-line sm:block" />

              <DateNav date={date} isToday={isToday} onDate={setDate} />

              {/* 指标和粒度是同一件事的两面（"怎么看"和"看多细"），当一个单元。
                  w-full + lg:w-auto：窄窗口换行时这一组独占一行且**靠左**，不然
                  ml-auto 会把它甩到右边，跟上面那行左对齐的控件看着像两组东西。 */}
              <div className="flex w-full shrink-0 items-center gap-2 lg:ml-auto lg:w-auto">
                <MetricPicker value={metric} onChange={switchMetric} />
                <GranularityPicker value={granularity} onChange={switchGranularity} />
              </div>
            </div>

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
                  {/* 标题位不写"24 小时时间线"—— 面板里就是 24h 色带，轴还标着
                      00:00–24:00，再声明一遍是零信息。改成放**读数**：它描述的正是
                      下面这块数据，放在这儿比推到页头最右（隔着一整条工具栏）更近。 */}
                  <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
                    <span className="panel-title tnum">
                      {metric === "category" && `${segments.length} 段`}
                      {metric === "focus" &&
                        (dayStats?.focusPct != null
                          ? `平均专注度 ${dayStats.focusPct}%（${granularity} 分一格）`
                          : "这一天没有活动")}
                      {metric === "switch" &&
                        (dayStats
                          ? `全天切换 ${dayStats.switches} 次（${granularity} 分一格）`
                          : "这一天没有活动")}
                    </span>
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
                      setSelectedBucket((cur) =>
                        cur === bucketIndex ? null : (bucketIndex ?? null),
                      );
                    }}
                    selectedId={selected?.id ?? null}
                    selectedBucket={selectedBucket}
                    showNow={isToday}
                    intervalMs={intervalMs}
                    metric={metric}
                  />
                </section>

                {/* 面板不再各自滚。滚动交给整页（根节点 min-h-screen），
                    免得窄窗口下三层滚动条套在一起，每层都只剩一两行。 */}
                <div className="grid flex-1 grid-cols-1 gap-4 lg:grid-cols-2">
                  <DaySummary segments={segments} dayStartMs={dayStartMs} />
                  <SegmentDetail segment={selected} />
                </div>
              </>
            )}
          </>
        )}
      </main>
    </div>
  );
}

/** 采集中指示。用状态色而不是某个类别色相 —— 状态色是保留语义。 */
function LiveBadge({ status }: { status: Status }) {
  const live = status === "ok";
  return (
    <span className="flex shrink-0 items-center gap-1.5 rounded-full border border-line bg-surface-1 px-2 py-0.5 text-micro whitespace-nowrap text-ink-muted">
      <Radio
        size={11}
        aria-hidden
        className={live ? "text-state-good" : "text-ink-ghost"}
      />
      {live ? "采集中" : status === "error" ? "已断开" : "连接中"}
    </span>
  );
}

/** 日期导航。原来和指标/粒度一起塞在 `Toolbar` 里，现在只是控制栏中间一段。 */
function DateNav({
  date,
  isToday,
  onDate,
}: {
  date: string;
  isToday: boolean;
  onDate: (d: string) => void;
}) {
  return (
    <div className="flex shrink-0 items-center gap-1.5">
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

      {/* 不是今天才出现 —— 常驻的按钮要么一直占位要么突然冒出来，
          都是让布局跳。消失本身是信息（"你在看今天"）。 */}
      {!isToday && (
        <button
          type="button"
          onClick={() => onDate(todayString())}
          className="cursor-pointer rounded-md border border-line bg-surface-1 px-2.5 py-1.5 text-sm text-ink-muted transition-colors duration-[--duration-fast] hover:bg-surface-2 hover:text-ink"
        >
          回到今天
        </button>
      )}
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
