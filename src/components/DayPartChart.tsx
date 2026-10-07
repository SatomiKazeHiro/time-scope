interface DayPartChartProps {
  /** 24 个桶，按本地小时切。只含非 idle 时长。 */
  hourlyMs: number[];
  /**
   * 撑满父容器的高度。指标卡里那张原来固定 74px，浮在 150px 的盒子顶上，
   * 下面一半是空的。`preserveAspectRatio` 默认会等比缩放并居中，
   * 所以显式设 `none` 让它按高度铺开 —— 柱子因此变高，而不是留白。
   */
  stretch?: boolean;
}

const H = 56;              // 绘图区高度（viewBox 单位）
const PLOT_W = 720;        // viewBox 宽
/** 柱宽占一格的比例。一半以下会像一排孤立的针（截图里就是这样）。 */
const BAR_RATIO = 0.56;
const HOUR_MS = 3_600_000;

/**
 * 24h 活跃时段分布。**24 根细柱，不用平滑面积** ——
 * 平滑曲线会在两个 0 之间鼓出弧，读起来像「凌晨 6 点也在用」，
 * 那是数据里不存在的值（spec §10）。
 *
 * 范围是「全部」时这条几乎是一条直线（5 天里既有熬夜的也有白天的，
 * 一平均就抵消了）。那不是 bug：平是诚实结果，点某天/某周就有波形。
 */
export default function DayPartChart({ hourlyMs, stretch = false }: DayPartChartProps) {
  // 长度不是 24 时补齐，别让调用方的畸形输入越界
  const h = hourlyMs.length === 24 ? hourlyMs : new Array(24).fill(0);
  const max = Math.max(...h, 1);
  const colW = PLOT_W / 24;
  const barW = colW * BAR_RATIO;

  return (
    /* 刻度文字**不能留在 SVG 里**。stretch 走的是
       `preserveAspectRatio="none"`，那会把 <text> 一起横向压扁 ——
       数字被挤成竖线。所以刻度标签放 HTML 覆盖层，
       和 SegmentTimeline 的时间轴标签同一个理由。 */
    <div className={stretch ? "flex h-full flex-col" : undefined}>
    <svg
      viewBox={`0 0 ${PLOT_W} ${H}`}
      /* 不等比缩放，否则铺满高度时两侧会留出空白，柱子被压窄 */
      preserveAspectRatio={stretch ? "none" : undefined}
      className={stretch ? "block min-h-0 w-full flex-1" : "block w-full max-w-[760px]"}
      role="img"
      aria-label="24 小时活跃分布"
    >
      {h.map((ms, hour) => {
        const bh = ms > 0 ? Math.max((ms / max) * H, 1.5) : 0;
        return (
          <rect
            key={hour}
            data-bar=""
            x={hour * colW + (colW - barW) / 2}
            y={H - bh}
            width={barW}
            height={bh}
            rx="2"
            /* 不等比拉伸时圆角会被拉成椭圆，柱宽大到一定程度就看得见了 */
            vectorEffect="non-scaling-stroke"
            fill="var(--color-scale-3)"
          >
            <title>
              {`${String(hour).padStart(2, "0")}:00 · ${Math.round((ms / HOUR_MS) * 10) / 10}h`}
            </title>
          </rect>
        );
      })}
      </svg>

      {/* 首尾两个刻度靠边对齐：居中会让「24」有一半跑出容器被切掉 */}
      <div className="relative mt-1 h-3.5 text-micro text-ink-faint" aria-hidden>
        {[0, 6, 12, 18, 24].map((t) => (
          <span
            key={t}
            className={`tnum absolute top-0 ${t === 0 ? "" : t === 24 ? "-translate-x-full" : "-translate-x-1/2"}`}
            style={{ left: `${(t / 24) * 100}%` }}
          >
            {String(t).padStart(2, "0")}
          </span>
        ))}
      </div>
    </div>
  );
}
