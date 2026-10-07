export interface RankedItem {
  label: string;
  /** 右侧读数，由调用方格式化（时长或次数，两者语义不同） */
  value: string;
  /** 0..100，决定条的长度 */
  ratio: number;
  /** 可选后缀，例如「已脱敏」 */
  note?: string;
}

interface RankedListProps {
  title: string;
  items: RankedItem[];
  emptyHint: string;
}

/** 排名列表。应用 Top 与窗口标题 Top 共用 —— 两者是同一形状。 */
export default function RankedList({ title, items, emptyHint }: RankedListProps) {
  return (
    <div className="panel p-3">
      <h2 className="panel-title mb-2">{title}</h2>
      {items.length === 0 ? (
        <p className="text-sm text-ink-faint">{emptyHint}</p>
      ) : (
        <ol className="grid gap-2">
          {items.map((it, i) => (
            <li
              key={`${it.label}-${i}`}
              className="grid grid-cols-[minmax(0,1fr)_auto] gap-x-2 gap-y-1"
            >
              <span className="truncate text-xs text-ink" title={it.label}>
                {it.label}
                {it.note && (
                  <em className="ml-1.5 text-[10px] text-state-warning not-italic">
                    {it.note}
                  </em>
                )}
              </span>
              <span className="tnum text-xs text-ink-faint">{it.value}</span>
              <span className="col-span-2 block h-[3px] overflow-hidden rounded-full bg-surface-2">
                <i data-bar="" className="block h-full bg-mark-fill"
                   style={{ width: `${it.ratio}%` }} />
              </span>
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}
