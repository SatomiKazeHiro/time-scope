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
 * **哪些字段要重启由后端说了算**（`get_config` 返回 `needs_restart`，
 * 保存时的提示读 `set_config` 的返回值 —— 同一个后端判定）。
 * 前端不复制那张表：复制一份就多一处会跟实现对不上的地方，
 * 而那种对不上表现为「界面说不用重启、实际要重启」，最难查。
 * （这个纪律曾经只是口头约定：`FIELDS` 里确实硬编码过一份 `restart`，
 *  2026-10-15 它与后端**一致地错**，界面照旧标着「已保存，立刻生效」。）
 *
 * **不做「点击重启」流程**（2026-10-08 用户定的）：常驻应用正在采集，
 * 重启意味着丢当前段、且要用户确认。提示到位就够了，流程以后再说。
 */
import { useCallback, useEffect, useState } from "react";
import { AppConfig, getConfig, setConfig } from "../types";
import { CircleAlert } from "lucide-react";

type ConfigKey = Exclude<keyof AppConfig, "needsRestart">;

type Editable = {
  [K in ConfigKey]: string;
};

/** 每一项：键名、标签、说明。
 *
 * **这里没有「要不要重启」那一栏** —— 它在 `cfg.needsRestart` 里，
 * 由后端随 `get_config` 一起给。曾经这里有一份硬编码副本，
 * 它与后端一致地错、界面照旧说谎，那次修复把副本删了。
 * 角标读不到判定时按「要重启」算（`?? true`）：那是诚实的那一侧。
 */
const FIELDS: {
  key: ConfigKey;
  /** `config.toml` 里的键名。**必须显式写出来** ——
   *  Rust 侧收的是 snake_case，从 camelCase 字段名推导多一层会错的地方。
   *  同时也是 `needsRestart` 的连接点。 */
  tomlKey: string;
  label: string;
  hint: string;
}[] = [
  {
    key: "idleThresholdS",
    tomlKey: "idle_threshold_s",
    label: "无输入多少秒算空闲",
    hint: "最小 10 秒。调小会让时间线更碎",
  },
  {
    key: "gracePeriodS",
    tomlKey: "grace_period_s",
    label: "短暂切换的宽限窗口（秒）",
    hint: "0 = 关掉抖动吸收。改它会明显改变切段的颗粒度",
  },
  {
    key: "minSegmentDurationS",
    tomlKey: "min_segment_duration_s",
    label: "短于此的段并入相邻（秒）",
    hint: "0 = 关掉合并",
  },
  {
    key: "heartbeatEveryS",
    tomlKey: "heartbeat_every_s",
    label: "心跳聚合窗口（秒）",
    hint: "调小会让段的数量明显变多",
  },
  {
    key: "eventsRetentionDays",
    tomlKey: "events_retention_days",
    label: "事件保留天数",
    hint: "只删原始窗口事件（约 5 MB/天），活动时间线不受影响。0 = 永不删",
  },
  { key: "autostart", tomlKey: "autostart", label: "开机自启", hint: "默认关" },
  {
    key: "closeBehavior",
    tomlKey: "close_behavior",
    label: "点 ✕ 关窗时",
    // 「每次」由字段名「点 ✕ 关窗时」带着，选项里不必重复；也短得多，
    // 塞得进那个 128px 的下拉框
    hint: "关窗时按这里说的做。改了不用重启",
  },
];

/** 下拉框显示中文，`value` 才是写进 `config.toml` 的值。 */
const CLOSE_BEHAVIOR_OPTIONS = [
  { value: "ask", label: "每次都问" },
  { value: "minimize", label: "最小化到托盘" },
  { value: "quit", label: "退出" },
] as const;

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
      .catch((e) => {
        // 界面文案只是给人看的；**日志才是给排查用的**。
        // setError 里的那句话经过好几个模块才到屏幕上，真要追
        // 「到底是哪个 IPC、哪次写回、哪个字段出的问题」，只能靠这里。
        console.error("[ConfigPanel] 读取 config 失败", e);
        setError(String(e));
      });
  }, []);

  // 注意不要写成 `useEffect(load, ...)`：`load` 现在返回 Promise，
// 那样 React 会把 Promise 当清理函数，返回时就炸 "destroy is not a function"
useEffect(() => {
  void load();
}, [load]);

  async function save() {
    // cfg 一起挡：没有它就不知道「用户改没改」，而按钮在这之前压根不会渲染
    if (!draft || !cfg) return;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      // 逐项写回：每项是一次独立的 patch_line，
      // 某一项失败不影响其余项已经落盘（部分成功是常态，不是异常）
      let needsRestart = false;
      for (const f of FIELDS) {
        if (isUnchanged(f.key, draft, cfg)) continue;
        const r = await setConfig(f.tomlKey, draft[f.key]);
        needsRestart = needsRestart || r;
      }
      setNotice(needsRestart ? "restart" : "saved");
      // 重读一次：clamp 可能改了用户填的值（填 0 的 idle 会被夹到 10）
      load();
    } catch (e) {
      // 两件事都要：日志给排查，界面文案给用户。
      //
      // 顺序要紧：**先重读磁盘上的真值，再把错误放回去**。
      // 反过来会被 `load()` 开头的 `setError(null)` 抹掉 ——
      // 写回失败却什么都不显示，是最难查的那种 bug。
      console.error("[ConfigPanel] 写回 config 失败", e);
      const msg = String(e);
      await load();
      setError(msg);
    } finally {
      setBusy(false);
    }
  }

  if (error && !cfg) {
    // 整块读不出来：这是「这一区没有内容」，不是一个字段的校验错误。
    // 样式跟两页共用的 PageAlert 一致，之前汇总页漏了警告图标就是这个
    return (
      <p role="alert" className="flex items-start gap-2 rounded-md border border-line bg-surface-1 p-3 text-sm text-ink">
        <CircleAlert size={15} className="mt-0.5 shrink-0 text-state-critical" aria-hidden />
        <span>{error}</span>
      </p>
    );
  }
  if (!cfg || !draft) {
    return <p className="text-sm text-ink-faint">正在读取…</p>;
  }

  const dirty = FIELDS.some((f) => !isUnchanged(f.key, draft, cfg));

  return (
    <div>
      <div className="mb-3 flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
        <span className="panel-title">行为参数</span>
        <span className="text-micro text-ink-faint">存在 %APPDATA%\time-scope\config.toml</span>
      </div>

      {error && (
        <p role="alert" className="mb-3 flex items-start gap-2 rounded-md border border-line bg-surface-1 p-3 text-sm text-ink">
          <CircleAlert size={15} className="mt-0.5 shrink-0 text-state-critical" aria-hidden />
          <span>{error}</span>
        </p>
      )}
      {notice === "restart" && (
        <p role="status" className="mb-3 text-label text-state-warning">
          已保存。上面标「重启后生效」的那几项要退出再启动才起作用。
        </p>
      )}
      {notice === "saved" && (
        <p role="status" className="mb-3 text-label text-ink-faint">
          已保存，立刻生效。
        </p>
      )}

      <dl className="grid gap-3">
        {FIELDS.map((f) => (
          <Field
            key={f.key}
            label={f.label}
            hint={f.hint}
            restart={cfg.needsRestart[f.tomlKey] ?? true}
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
        className="mt-3 cursor-pointer rounded-md border border-line px-3 py-1 text-sm text-ink transition-colors duration-[--duration-fast] hover:bg-surface-2 disabled:cursor-not-allowed disabled:opacity-40"
      >
        {busy ? "保存中…" : "保存"}
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
  const isChoice = CLOSE_BEHAVIOR_OPTIONS.some((o) => o.value === value);

  return (
    <div className="grid grid-cols-1 items-baseline gap-x-4 gap-y-0.5 sm:grid-cols-[minmax(0,14rem)_auto_minmax(0,1fr)] sm:items-center">
      <dt className="text-label text-ink">
        {label}
        {restart && (
          // 「重启后生效」是这个控件的一部分，不是一行附注 ——
          // 用户改完它发现没反应时，第一眼就要看到这句。
          // ink-faint 而非 ink-ghost：后者 theme.css 标注「仅非文字」
          // （3.84 / 3.07:1），而这句是要被读到的信息
          <span className="ml-1.5 text-micro text-ink-faint">重启后生效</span>
        )}
      </dt>
      <dd className="contents">
        {isBool ? (
          <input
            type="checkbox"
            aria-label={label}
            checked={value === "true"}
            disabled={disabled}
            onChange={(e) => onChange(String(e.target.checked))}
            className="size-4 justify-self-start accent-[var(--color-neutral-solid)]"
          />
        ) : isChoice ? (
          <select
            aria-label={label}
            value={value}
            disabled={disabled}
            onChange={(e) => onChange(e.target.value)}
            className="w-32 rounded-sm border border-line bg-surface-1 px-2 py-1 text-label text-ink"
          >
            {CLOSE_BEHAVIOR_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
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
            className="tnum w-32 rounded-sm border border-line bg-surface-1 px-2 py-1 text-label text-ink"
          />
        )}
        <span className="text-micro text-ink-faint">{hint}</span>
      </dd>
    </div>
  );
}

/** 这一项的草稿值与磁盘上的原值相同吗？
 *
 * **两边类型不一样** —— 草稿全是字符串（`toEditable` 里 `String(...)` 过），
 * 而 `cfg` 里数字是 number、`autostart` 是 boolean。直接 `===` 恒为 false，
 * 于是「用户改没改」判不出来：保存按钮永远可点、每次保存把七行全部重写
 * （`close_behavior` 是唯一躲过的，因为它两边都是字符串）。
 *
 * 两处比较都走这一个函数 —— 「有没有改动」在保存按钮与写回循环里各用一次，
 * 抄两遍就是下一次只改一处的地方。
 */
function isUnchanged(key: ConfigKey, draft: Editable, cfg: AppConfig): boolean {
  return draft[key] === String(cfg[key]);
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