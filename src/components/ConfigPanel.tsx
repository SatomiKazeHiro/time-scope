/**
 * 行为参数设置。
 *
 * **这一页的核心约束是诚实**：有些参数改完立刻生效，有些必须重启。
 * 后者必须在界面上写明 —— 用户拖了滑块看到时间线纹丝不动，
 * 会以为界面坏了，而「界面坏了」是个比「要重启」坏得多的结论。
 *
 * 这是 STATUS「别动这里」第 16 条「读数就该长得像读数」的延伸：
 * 那条讲的是读数（范围 chip 曾经长得像能点的按钮），
 * 这里讲的是**控件**——一个当下不起作用的滑块就该说自己不起作用。
 *
 * **哪些字段要重启由后端说了算**（`set_config` 返回 `needs_restart`）。
 * 前端不复制那张表：复制一份就多一处会跟实现对不上的地方，
 * 而那种对不上表现为「界面说不用重启、实际要重启」，最难查。
 *
 * **不做「点击重启」流程**（2026-10-08 用户定的）：常驻应用正在采集，
 * 重启意味着丢当前段、且要用户确认。提示到位就够了，流程以后再说。
 */
import { useCallback, useEffect, useState } from "react";
import { AppConfig, getConfig, setConfig } from "../types";

type Editable = {
  [K in keyof AppConfig]: string;
};

/** 每一项：键名、标签、说明、以及它属于「立刻生效」还是「要重启」。 */
const FIELDS: {
  key: keyof AppConfig;
  /** `config.toml` 里的键名。**必须显式写出来** ——
   *  Rust 侧收的是 snake_case，从 camelCase 字段名推导多一层会错的地方。 */
  tomlKey: string;
  label: string;
  hint: string;
  /** 界面层面的预期；后端返回 needs_restart 才是准的，这里只用于标注 */
  restart: boolean;
}[] = [
  {
    key: "idleThresholdS",
    tomlKey: "idle_threshold_s",
    label: "无输入多少秒算空闲",
    hint: "最小 10 秒。调小会让时间线更碎",
    restart: true,
  },
  {
    key: "gracePeriodS",
    tomlKey: "grace_period_s",
    label: "短暂切换的宽限窗口（秒）",
    hint: "0 = 关掉抖动吸收。改它会明显改变切段的颗粒度",
    restart: true,
  },
  {
    key: "minSegmentDurationS",
    tomlKey: "min_segment_duration_s",
    label: "短于此的段并入相邻（秒）",
    hint: "0 = 关掉合并",
    restart: true,
  },
  {
    key: "heartbeatEveryS",
    tomlKey: "heartbeat_every_s",
    label: "心跳聚合窗口（秒）",
    hint: "调小会让段的数量明显变多",
    restart: true,
  },
  {
    key: "eventsRetentionDays",
    tomlKey: "events_retention_days",
    label: "事件保留天数",
    hint: "只删原始窗口事件（约 5 MB/天），活动时间线不受影响。0 = 永不删",
    restart: false,
  },
  { key: "autostart", tomlKey: "autostart", label: "开机自启", hint: "默认关", restart: false },
  {
    key: "closeBehavior",
    tomlKey: "close_behavior",
    label: "点 ✕ 关窗时",
    hint: "ask = 问一次 / minimize = 最小化到托盘 / quit = 退出",
    restart: false,
  },
];

const CLOSE_BEHAVIORS = ["ask", "minimize", "quit"] as const;

export default function ConfigPanel() {
  const [cfg, setCfg] = useState<AppConfig | null>(null);
  const [draft, setDraft] = useState<Editable | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<"restart" | "saved" | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    setError(null);
    return getConfig()
      .then((c) => {
        setCfg(c);
        setDraft(toEditable(c));
      })
      .catch((e) => setError(String(e)));
  }, []);

  // 注意不要写成 `useEffect(load, ...)`：`load` 现在返回 Promise，
// 那样 React 会把 Promise 当清理函数，返回时就炸 "destroy is not a function"
useEffect(() => {
  void load();
}, [load]);

  async function save() {
    if (!draft) return;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      // 逐项写回：每项是一次独立的 patch_line，
      // 某一项失败不影响其余项已经落盘（部分成功是常态，不是异常）
      let needsRestart = false;
      for (const f of FIELDS) {
        if (draft[f.key] === cfg?.[f.key]) continue;
        const r = await setConfig(f.tomlKey, draft[f.key]);
        needsRestart = needsRestart || r;
      }
      setNotice(needsRestart ? "restart" : "saved");
      // 重读一次：clamp 可能改了用户填的值（填 0 的 idle 会被夹到 10）
      load();
    } catch (e) {
      // 顺序要紧：先重读磁盘上的真值，再把错误放回去。
      // 反过来会被 `load()` 开头的 `setError(null)` 抹掉 ——
      // 写回失败却什么都不显示，是最难查的那种 bug。
      const msg = String(e);
      await load();
      setError(msg);
    } finally {
      setBusy(false);
    }
  }

  if (error && !cfg) {
    return (
      <p role="alert" className="mt-2 text-xs text-state-critical">
        {error}
      </p>
    );
  }
  if (!cfg || !draft) {
    return <p className="mt-2 text-sm text-ink-faint">正在读取…</p>;
  }

  const dirty = FIELDS.some((f) => draft[f.key] !== cfg[f.key]);

  return (
    <div>
      <h2 className="text-sm font-medium text-ink">行为参数</h2>
      <p className="mt-1 text-xs text-ink-faint">
        存在 %APPDATA%\time-scope\config.toml 里。带「重启后生效」的那几项绑在
        采集线程和引擎构造参数上，进程内改不了。
      </p>

      {error && (
        <p role="alert" className="mt-2 text-xs text-state-critical">
          {error}
        </p>
      )}
      {notice === "restart" && (
        <p role="status" className="mt-2 text-xs text-state-warning">
          已保存。上面标「重启后生效」的那几项要退出再启动才起作用。
        </p>
      )}
      {notice === "saved" && (
        <p role="status" className="mt-2 text-xs text-ink-faint">
          已保存，立刻生效。
        </p>
      )}

      <dl className="mt-3 grid gap-3">
        {FIELDS.map((f) => (
          <Field
            key={f.key}
            label={f.label}
            hint={f.hint}
            restart={f.restart}
            value={draft[f.key]}
            disabled={busy}
            onChange={(v) => setDraft({ ...draft, [f.key]: v })}
          />
        ))}
      </dl>

      <button
        type="button"
        disabled={!dirty || busy}
        onClick={save}
        className="mt-3 rounded-md border border-line px-3 py-1 text-xs text-ink hover:bg-surface-2 disabled:opacity-40"
      >
        保存
      </button>
    </div>
  );
}

function Field({
  label,
  hint,
  restart,
  value,
  disabled,
  onChange,
}: {
  label: string;
  hint: string;
  restart: boolean;
  value: string;
  disabled: boolean;
  onChange: (v: string) => void;
}) {
  const isBool = value === "true" || value === "false";
  const isChoice = CLOSE_BEHAVIORS.includes(value as (typeof CLOSE_BEHAVIORS)[number]);

  return (
    <div className="grid grid-cols-1 gap-1">
      <dt className="text-xs text-ink">
        {label}
        {restart && (
          <span className="ml-1.5 text-[10px] text-ink-ghost">重启后生效</span>
        )}
      </dt>
      <dd className="grid grid-cols-[minmax(0,9rem)_minmax(0,1fr)] items-center gap-2">
        {isBool ? (
          <input
            type="checkbox"
            aria-label={label}
            checked={value === "true"}
            disabled={disabled}
            onChange={(e) => onChange(String(e.target.checked))}
            className="size-4 accent-[var(--color-neutral-solid)]"
          />
        ) : isChoice ? (
          <select
            aria-label={label}
            value={value}
            disabled={disabled}
            onChange={(e) => onChange(e.target.value)}
            className="rounded-sm border border-line bg-surface-1 px-2 py-1 text-xs text-ink"
          >
            {CLOSE_BEHAVIORS.map((b) => (
              <option key={b} value={b}>
                {b}
              </option>
            ))}
          </select>
        ) : (
          <input
            type="number"
            aria-label={label}
            value={value}
            disabled={disabled}
            onChange={(e) => onChange(e.target.value)}
            className="rounded-sm border border-line bg-surface-1 px-2 py-1 text-xs tabular-nums text-ink"
          />
        )}
        <span className="text-[11px] text-ink-faint">{hint}</span>
      </dd>
    </div>
  );
}

function toEditable(c: AppConfig): Editable {
  return {
    idleThresholdS: String(c.idleThresholdS),
    gracePeriodS: String(c.gracePeriodS),
    minSegmentDurationS: String(c.minSegmentDurationS),
    heartbeatEveryS: String(c.heartbeatEveryS),
    eventsRetentionDays: String(c.eventsRetentionDays),
    autostart: String(c.autostart),
    closeBehavior: c.closeBehavior,
  };
}