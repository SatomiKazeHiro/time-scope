import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import ContributionWall from "../components/ContributionWall";
import DayPartChart from "../components/DayPartChart";
import MetricRow from "../components/MetricRow";
import RankedList, { type RankedItem } from "../components/RankedList";
import {
  fillDays, rangeFor, rangeLabel, type DateRange, type RangeKind,
} from "../lib/summary";
import { formatDuration } from "../lib/bucket";
import {
  getDailyCalendar, getSummary, getTopTitles,
  type DayCell, type MergedTitle, type Summary,
} from "../types";

/** 标题排名的条数。 */
const TITLE_LIMIT = 10;

/**
 * 汇总页。
 *
 * 范围状态只存在于这里，**不写 URL、不持久化**（spec §2.3）——
 * 这是单机桌面应用，不是可分享的网页。
 *
 * 三段布局（spec §2.2）：指标横条 / 热力图整宽（含 24h 条）/ 底部双栏。
 * 段内不各自滚，滚动交给整页。
 */
export default function SummaryPage() {
  // `all` 是热力图覆盖的整个数据区间。热力图只渲染它，**与选中无关**。
  const [all, setAll] = useState<DateRange | null>(null);
  const [kind, setKind] = useState<RangeKind>("all");
  const [picked, setPicked] = useState<string>("");   // 点中的那一天

  const [days, setDays] = useState<DayCell[]>([]);
  const [summary, setSummary] = useState<Summary | null>(null);
  const [error, setError] = useState(false);
  const [titles, setTitles] = useState<MergedTitle[]>([]);
  const [titleBusy, setTitleBusy] = useState(false);

  // ① 热力图数据：挂载时取一次。依赖数组**故意是空的**——
  //    范围变化时绝不能重取（spec §2.3）。
  useEffect(() => {
    let dead = false;
    getDailyCalendar()
      .then((cal) => {
        if (dead) return;
        if (!cal) return;                       // 库为空：保持 all=null，走空状态
        setAll({ kind: "all", from: cal.first, to: cal.last, label: "全部" });
        setDays(
          fillDays(cal.first, cal.last, new Map(cal.days.map((d) => [d.date, d.totalMs]))),
        );
      })
      .catch(() => { if (!dead) setError(true); });
    return () => { dead = true; };
  }, []);

  const range: DateRange | null = useMemo(
    () => (all ? rangeFor(kind, picked || all.to, all) : null),
    [all, kind, picked],
  );
  const from = range?.from;
  const to = range?.to;

  // ② 顶部指标：随 range 变。
  useEffect(() => {
    if (!from || !to) return;
    let dead = false;
    setError(false);
    getSummary(from, to)
      .then((s) => { if (!dead) setSummary(summaryOrEmpty(s)); })
      .catch(() => { if (!dead) { setError(true); setSummary(null); } });
    return () => { dead = true; };
  }, [from, to]);

  /**
   * ③ 标题排名：懒加载（spec §3.3）—— 它是唯一走 events 表的查询，
   *    而默认范围又是「全部」，所以最贵的这条路恰好是最常走的那条。
   *
   * 「检测可见」和「取数」刻意分成两个 effect：前者只置一个标志，
   * 后者挂在 [from, to, visible] 上，跟指标那次取数是同一种形状 ——
   * 缠在一个 IntersectionObserver 回调里会让取数不可测。
   */
  const bottomRef = useRef<HTMLDivElement>(null);
  const [titleVisible, setTitleVisible] = useState(false);

  useEffect(() => {
    const el = bottomRef.current;
    if (!el || typeof IntersectionObserver === "undefined") return;
    const io = new IntersectionObserver(
      (entries) => {
        if (!entries.some((e) => e.isIntersecting)) return;
        setTitleVisible(true);
        io.disconnect();
      },
      { rootMargin: "200px" },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [all]);

  useEffect(() => {
    if (!from || !to || !titleVisible) return;
    let dead = false;
    setTitleBusy(true);
    getTopTitles(from, to, TITLE_LIMIT)
      .then((t) => { if (!dead) setTitles(t); })
      .catch(() => { if (!dead) setTitles([]); })
      .finally(() => { if (!dead) setTitleBusy(false); });
    return () => { dead = true; };
  }, [from, to, titleVisible]);

  /**
   * 一次点击 = 选中该范围；**再点同一个目标 = 回到「全部」**。
   * 归一化的实现由 `rangeFor` 做，这里只决定「选什么」还是「取消」。
   */
  const select = useCallback(
    (k: RangeKind) => (date: string) => {
      if (k === "all" || (kind === k && picked === date)) {
        setKind("all");
        setPicked("");
        return;
      }
      setKind(k);
      setPicked(date);
    },
    [kind, picked],
  );

  // 标题次数不换算成时长——那是「出现次数」，说成时间会骗人。
  const titleItems: RankedItem[] = useMemo(() => {
    if (titles.length === 0) return [];
    const max = Math.max(...titles.map((t) => t.hits), 1);
    return titles.map((t) => ({
      label: t.title,
      value: `${t.hits} 次`,
      ratio: Math.round((t.hits / max) * 100),
      note: t.redacted ? "已脱敏" : undefined,
    }));
  }, [titles]);
  const titleEmpty = titleBusy
    ? "统计中…"
    : titles.length === 0
      ? "向下滚动加载"
      : "这个范围没有记录";

  const appItems: RankedItem[] = useMemo(() => {
    const apps = summary?.topApps ?? [];
    if (apps.length === 0) return [];
    const max = Math.max(...apps.map((a) => a.ms), 1);
    return apps.map((a) => ({
      label: a.name,
      value: formatDuration(a.ms),
      ratio: Math.round((a.ms / max) * 100),
    }));
  }, [summary]);

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-3">
      <div className="flex shrink-0 flex-wrap items-center gap-2">
        <h1 className="m-0 shrink-0 text-label font-semibold tracking-wide whitespace-nowrap text-ink-muted">
          Time Scope
        </h1>
        {/* 本轮是死控件：范围由热力图上的点击驱动（spec §1.2） */}
        <button
          type="button"
          data-testid="range-chip"
          aria-disabled="true"
          className="cursor-default rounded-md border border-line bg-surface-1 px-2.5 py-1 text-sm text-ink-faint"
        >
          范围：{range ? rangeLabel(range) : "—"}
        </button>
      </div>

      {error && (
        <div
          role="alert"
          className="flex items-start gap-2 rounded-md border border-line bg-surface-1 p-3 text-sm text-ink"
        >
          <span>
            加载失败。请确认后端已启动（<code className="tnum">pnpm tauri dev</code>）。
          </span>
        </div>
      )}

      <MetricRow summary={summary} loading={!summary && !error} />

      <section className="panel p-4" aria-label="监控时长热力图">
        <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
          <span className="panel-title tnum">
            监控时长 · {all ? `${all.from} – ${all.to}` : "暂无"}
          </span>
          <span className="text-micro text-ink-faint">少 ░▒▓█ 多</span>
        </div>
        <ContributionWall
          days={days}
          selection={kind === "all" ? null : range}
          onSelectDay={select("day")}
          onSelectWeek={select("week")}
          onSelectMonth={select("month")}
        />
        <div className="mt-3 border-t border-line pt-3">
          <DayPartChart hourlyMs={summary?.hourlyMs ?? []} />
        </div>
      </section>

      <div className="grid flex-1 grid-cols-1 gap-4 lg:grid-cols-2" ref={bottomRef}>
        <RankedList title="窗口标题 Top" items={titleItems} emptyHint={titleEmpty} />
        <RankedList
          title="应用 Top"
          items={appItems}
          emptyHint="这个范围没有记录"
        />
      </div>
    </div>
  );
}

/** 后端可能返回缺字段的形状（老库 / 半成品），这里补齐而不是让组件崩。 */
function summaryOrEmpty(s: Summary): Summary {
  return {
    ...s,
    hourlyMs: s.hourlyMs ?? [],
    donut: s.donut ?? [],
    topApps: s.topApps ?? [],
  };
}
