import { GRANULARITIES } from "../lib/bucket";

interface Props {
  value: number;
  onChange: (minutes: number) => void;
}

/** 粒度切换器（spec §10：10 / 30（默认）/ 60 / 120 分钟）。切换只改前端参数，不重查后端。 */
export default function GranularityPicker({ value, onChange }: Props) {
  return (
    <div role="group" aria-label="时间粒度" style={{ display: "flex", gap: 4 }}>
      {GRANULARITIES.map((m) => {
        const active = value === m;
        return (
          <button
            key={m}
            type="button"
            onClick={() => onChange(m)}
            aria-pressed={active}
            style={{
              fontWeight: active ? 700 : 400,
              background: active ? "#4c8dff" : undefined,
              color: active ? "#fff" : undefined,
              border: "1px solid #ccc",
              borderRadius: 4,
              padding: "2px 8px",
              cursor: "pointer",
            }}
          >
            {m}分
          </button>
        );
      })}
    </div>
  );
}
