interface DayPartChartProps {
  /** 24 个桶，按本地小时切。只含非 idle 时长。 */
  hourlyMs: number[];
  /** 指标卡里的窄版：去掉底部说明、刻度只留 00/12/24 */
  compact?: boolean;
}

const H = 56;              // 绘图区高度（px）
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
export default function DayPartChart({ hourlyMs, compact = false }: DayPartChartProps) {
  // 长度不是 24 时补齐，别让调用方的畸形输入越界
  const h = hourlyMs.length === 24 ? hourlyMs : new Array(24).fill(0);
  const max = Math.max(...h, 1);
  const colW = PLOT_W / 24;
  const barW = colW * BAR_RATIO;

  return (
    <div>
      <svg
        viewBox={`0 0 ${PLOT_W} ${H + 18}`}
        /* 限宽而不是 w-full：面板 1100+px 宽时全拉会让 24 根柱摊成
           一排孤立的针，刻度字也跟着放大到 14px。 */
        className={compact ? "block w-full" : "block w-full max-w-[760px]"}
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
              fill="var(--color-scale-3)"
            >
              <title>
                {`${String(hour).padStart(2, "0")}:00 · ${Math.round((ms / HOUR_MS) * 10) / 10}h`}
              </title>
            </rect>
          );
        })}
        {(compact ? [0, 12, 24] : [0, 6, 12, 18, 24]).map((t) => (
          <g key={t}>
            <line
              x1={(t / 24) * PLOT_W} y1={H}
              x2={(t / 24) * PLOT_W} y2={H + 4}
              stroke="var(--color-line)" strokeWidth="1"
            />
            {/* 首尾两个刻度靠边对齐：居中会让「24」有一半落在 viewBox 外被切掉 */}
            <text
              x={(t / 24) * PLOT_W} y={H + 15}
              fill="var(--color-ink-faint)" fontSize="9"
              textAnchor={t === 0 ? "start" : t === 24 ? "end" : "middle"}
            >
              {String(t).padStart(2, "0")}
            </text>
          </g>
        ))}
      </svg>
      {!compact && (
        <p className="text-[10px] text-ink-faint">
          24h 时段分布 · 非空闲时长。主打「什么时间在活跃」。
        </p>
      )}
    </div>
  );
}
