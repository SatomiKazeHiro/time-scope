/**
 * 设置页 —— 「哪些程序还没规则覆盖」+ 分类配置。
 *
 * 这个页面回答的是**「你的配置缺了什么」**，与汇总面板的「今天干了什么」
 * 是两件事。同一张未覆盖列表曾在当日汇总里做过并被撤回：一张配置表
 * 混进一个描述时间去向的面板，会让两件事都说不清。
 *
 * **加完规则立刻生效**，不等重启 —— 用户被要求重启，这个入口就没有意义。
 * 后端 `add_rule` 写回 rules.toml 后热替换内存里的规则集。
 *
 * 另一页（配置）见 ConfigPanel；两者共用本页的外壳与分区标题样式。
 */
import { useCallback, useEffect, useState } from "react";
import { CATEGORY_META, TIER_LABEL, TIER_ORDER } from "../design/categories";
import { formatDuration } from "../lib/bucket";
import {
  addRule,
  Category,
  getUncoveredPrograms,
  UncoveredProgram,
} from "../types";
import ConfigPanel from "./ConfigPanel";

/** 统计窗口。写进界面是因为「38%」这个数没有分母就没有意义。 */
const WINDOW_DAYS = 30;

/**
 * 能作为规则目标的类别。
 *
 * **`idle` 与 `unknown` 被排除是有理由的**：它们不是「程序属于哪一类」，
 * 是「没在用」和「没认出来」。写成规则只会造出永远命中不了的死规则。
 * 与后端 `append_rule` 的校验同款 —— 两边都要挡，界面挡是为了不给出点不动的选项。
 */
const RULE_TARGETS: Category[] = ["work", "study", "browsing", "entertainment", "communication", "life"];

/**
 * @param rulesOnly **仅测试用**：只渲染规则区。
 *
 * 存在的唯一原因是 vitest 的 `vi.mock` 对同时发起的重复动态 import 处理不对
 * （第二个 import 拿到 undefined）。真实使用里 App 一次只挂一个视图，
 * 不存在这个时序。产品代码不该知道这件事，所以它是个 prop 而不是分支逻辑。
 */
export default function SettingsPage({ rulesOnly = false }: { rulesOnly?: boolean } = {}) {
  const [rows, setRows] = useState<UncoveredProgram[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [picking, setPicking] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    setError(null);
    getUncoveredPrograms(WINDOW_DAYS)
      .then((r) => setRows(r))
      .catch((e) => {
        // 界面文案给人看，日志给排查用 —— 错误要流过
        // IPC → types.ts → 这里才变成一行字，只有 console 里那条
        // 带着原始的异常对象和调用点
        console.error("[SettingsPage] 读取未覆盖程序失败", e);
        setError(String(e));
      });
  }, []);

  // 不写成 `useEffect(load, ...)`：React 会把返回值当清理函数。
  // `load` 一旦改成返回 Promise（像 ConfigPanel 那样为了 await 重读），
  // 那样写就会在卸载时炸 `destroy is not a function` —— 而且只在
  // 两个组件同时挂载时才暴露，极难联想到真正的原因。
  useEffect(() => {
    void load();
  }, [load]);

  async function choose(app: string, cat: Category) {
    setBusy(true);
    setError(null);
    try {
      await addRule(app, cat);
      // 加成功的那个从列表里消失，是这个功能唯一的反馈回路。
      // 重新拉而不是本地剔除：后端可能还记着别的同名未覆盖段。
      setPicking(null);
      load();
    } catch (e) {
      // 失败必须说清并留在原地让人重试 —— 静默失败会让人以为配好了。
      // 程序名和类别一起打进日志：报障时用户只会说「加规则失败了」，
      // 日志里这两个值才认得出是哪一条
      console.error(`[SettingsPage] 给 ${app} 加规则失败（category=${cat}）`, e);
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section aria-label="设置" className="panel min-h-0 flex-1 overflow-y-auto p-4">
      <h1 className="panel-title">设置</h1>

      <div className="mt-3 rounded-md border border-line bg-surface-2 p-3">
        <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
          <h2 className="text-sm font-medium text-ink">规则覆盖</h2>
          <span className="text-xs text-ink-faint">
            只统计最近 {WINDOW_DAYS} 天 · 这些程序有时间但没落进任何类别
          </span>
        </div>

        {error && (
          <p role="alert" className="mt-2 text-xs text-state-critical">
            {error}
          </p>
        )}

        {rows === null ? (
          <p className="mt-2 text-sm text-ink-faint">正在统计…</p>
        ) : rows.length === 0 ? (
          <p className="mt-2 text-sm text-ink-faint">
            最近 {WINDOW_DAYS} 天出现过的程序全部都有规则覆盖。
          </p>
        ) : (
          <ol className="mt-2 grid gap-1">
            {rows.map((r) => (
              <li
                key={r.application}
                className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 gap-y-1 rounded-sm px-1 py-1 hover:bg-surface-3"
              >
                <span className="truncate text-xs text-ink" title={r.application}>
                  {r.application}
                </span>
                <span className="flex items-center gap-2">
                  <span className="text-xs tabular-nums text-ink-faint">
                    {formatDuration(r.totalMs)}
                  </span>
                  {picking === r.application ? (
                    <CategoryPicker
                      disabled={busy}
                      onPick={(c) => choose(r.application, c)}
                      onCancel={() => setPicking(null)}
                    />
                  ) : (
                    <button
                      type="button"
                      // 可访问名里带上程序名：这一行有十几个同样的「归类…」，
                      // 只念按钮文字的话读屏用户不知道自己在给谁归类
                      aria-label={`给 ${r.application} 归类`}
                      className="rounded-sm px-1.5 py-0.5 text-xs text-ink-muted hover:bg-surface-1 hover:text-ink"
                      onClick={() => setPicking(r.application)}
                    >
                      归类…
                    </button>
                  )}
                </span>
              </li>
            ))}
          </ol>
        )}
      </div>

      {!rulesOnly && (
        <div className="mt-3 rounded-md border border-line bg-surface-2 p-3">
          <ConfigPanel />
        </div>
      )}
    </section>
  );
}

/** 六个可选类别，按层级分组 —— 分组标题用的是 `TIER_LABEL`，与时间线一致。 */
function CategoryPicker({
  disabled,
  onPick,
  onCancel,
}: {
  disabled: boolean;
  onPick: (c: Category) => void;
  onCancel: () => void;
}) {
  return (
    <span className="flex flex-wrap items-center justify-end gap-1">
      {TIER_ORDER.filter((t) => t !== "absent").map((tier) => (
        <span key={tier} className="flex items-center gap-1">
          <span className="text-[10px] text-ink-ghost">{TIER_LABEL[tier]}</span>
          {RULE_TARGETS.filter((c) => CATEGORY_META[c].tier === tier).map((c) => (
            <button
              key={c}
              type="button"
              disabled={disabled}
              // 类别名在这里是**唯一**可读的标识 —— 行内只有程序名，
              // 光靠色块分不出「工作」和「学习」（§2.3 层级内靠明度区分）
              onClick={() => onPick(c)}
              className="rounded-sm px-1.5 py-0.5 text-xs text-ink hover:bg-surface-1 disabled:opacity-50"
            >
              {CATEGORY_META[c].label}
            </button>
          ))}
        </span>
      ))}
      <button
        type="button"
        onClick={onCancel}
        className="rounded-sm px-1.5 py-0.5 text-xs text-ink-faint hover:text-ink"
      >
        取消
      </button>
    </span>
  );
}

