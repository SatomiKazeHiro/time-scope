/**
 * 设置页 —— 「哪些程序还没规则覆盖」+ 分类配置。
 *
 * 这个页面回答的是「你的配置缺了什么」，与汇总面板的「今天干了什么」
 * 是两件事。同一张未覆盖列表曾在当日汇总里做过并被撤回：一张配置表
 * 混进一个描述时间去向的面板，会让两件事都说不清。
 *
 * 加完规则立刻生效，不等重启 —— 用户被要求重启，这个入口就没有意义。
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
import { PageAlert, PageHeader } from "./PageChrome";

/** 统计窗口。写进界面是因为「38%」这个数没有分母就没有意义。 */
const WINDOW_DAYS = 30;

/**
 * 能作为规则目标的类别。
 *
 * `idle` 与 `unknown` 被排除是有理由的：它们不是「程序属于哪一类」，
 * 是「没在用」和「没认出来」。写成规则只会造出永远命中不了的死规则。
 * 与后端 `append_rule` 的校验同款 —— 两边都要挡，界面挡是为了不给出点不动的选项。
 */
const RULE_TARGETS: Category[] = ["work", "study", "browsing", "entertainment", "communication", "life"];

/**
 * @param rulesOnly 仅测试用：只渲染规则区。
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
    /* 与汇总页同一个骨架：页头 + 扁平 panel 分区 + gap-3。
       不要在这里加 `overflow-y-auto` —— App 的根是 `min-h-screen`，
       整页滚是既定决定（见 App.tsx 的注释）；页面自己再滚一层就是
       「窄窗口下三层嵌套滚动条」那个已经修过的问题。

       根节点用 `<section aria-label>` 而不是 `<div>`：`<section>` 带
       可访问名时是 role=region，读屏用户能在地标列表里直接跳到这一页
       （Sidebar.test 有这条断言）。 */
    <section aria-label="设置" className="flex min-w-0 flex-1 flex-col gap-3">
      <PageHeader title="设置" />

      {error && <PageAlert>{error}</PageAlert>}

      <section className="panel p-4" aria-label="规则覆盖">
        <div className="mb-3 flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
          <span className="panel-title">规则覆盖</span>
          <span className="text-micro text-ink-faint">
            最近 {WINDOW_DAYS} 天 · 有时间但没落进任何类别
          </span>
        </div>

        {rows === null ? (
          <p className="text-sm text-ink-faint">正在统计…</p>
        ) : rows.length === 0 ? (
          <p className="text-sm text-ink-faint">
            最近 {WINDOW_DAYS} 天出现过的程序全部都有规则覆盖。
          </p>
        ) : (
          <ol className="grid gap-0.5">
            {rows.map((r) => (
              <li
                key={r.application}
                className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 rounded-sm px-1 py-1 hover:bg-surface-2"
              >
                <span className="truncate text-sm text-ink" title={r.application}>
                  {r.application}
                </span>
                <span className="flex items-center gap-2">
                  <span className="tnum text-micro text-ink-faint">
                    {formatDuration(r.totalMs)}
                  </span>
                  {picking === r.application ? (
                    <CategoryPicker
                      application={r.application}
                      disabled={busy}
                      onPick={(c) => choose(r.application, c)}
                      onCancel={() => setPicking(null)}
                    />
                  ) : (
                    <button
                      type="button"
                      // 可访问名里带上程序名：这一列有十几个同样的「归类…」，
                      // 只念按钮文字的话读屏用户不知道自己在给谁归类
                      aria-label={`给 ${r.application} 归类`}
                      className="cursor-pointer rounded-sm border border-line px-2 py-0.5 text-label text-ink-muted transition-colors duration-[--duration-fast] hover:bg-surface-1 hover:text-ink"
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
      </section>

      {!rulesOnly && (
        <section className="panel p-4" aria-label="行为参数">
          <ConfigPanel />
        </section>
      )}
    </section>
  );
}

/**
 * 六个可选类别，按层级分组。
 *
 * 写法照抄 `GranularityPicker` 的分段控件（`gap-0.5` + `p-0.5` 的容器、
 * `rounded-sm` 的按钮），它才是「从若干个里挑一个」在这个项目里的既有词汇，
 * 自己另发明一套会让两个控件看起来不是一家的。
 *
 * 类别色块是辅助，可读的名字才是标识 —— 层级内的两个类别只差明度
 * （MASTER §2.3），光靠色块分不出「工作」和「学习」。色块带 1px 内描边，
 * 与图例、详情里的类别色块同款（§2.4）。
 */
function CategoryPicker({
  application,
  disabled,
  onPick,
  onCancel,
}: {
  application: string;
  disabled: boolean;
  onPick: (c: Category) => void;
  onCancel: () => void;
}) {
  return (
    <span className="flex items-center gap-2">
      <span
        role="group"
        aria-label={`给 ${application} 选类别`}
        className="inline-flex gap-0.5 rounded-md border border-line bg-surface-1 p-0.5"
      >
        {TIER_ORDER.filter((t) => t !== "absent").map((tier) => (
          <span key={tier} className="flex items-center gap-0.5">
            {/* ink-faint 而不是 ink-ghost：后者只有 3.84 / 3.07:1，
                theme.css 标注「仅非文字」。层级名是文字，
                而分组信息对读屏也有用，所以不 aria-hidden。 */}
            <span className="px-1 text-micro text-ink-faint">
              {TIER_LABEL[tier]}
            </span>
            {RULE_TARGETS.filter((c) => CATEGORY_META[c].tier === tier).map((c) => (
              <button
                key={c}
                type="button"
                disabled={disabled}
                onClick={() => onPick(c)}
                className="flex cursor-pointer items-center gap-1 rounded-sm px-2 py-0.5 text-label text-ink-muted transition-colors duration-[--duration-fast] hover:bg-surface-2 hover:text-ink disabled:opacity-50"
              >
                <span
                  aria-hidden
                  className="size-2.5 rounded-[2px] shadow-[inset_0_0_0_1px_var(--color-line-strong)]"
                  style={{ background: `var(${CATEGORY_META[c].varName})` }}
                />
                {CATEGORY_META[c].label}
              </button>
            ))}
          </span>
        ))}
      </span>
      <button
        type="button"
        onClick={onCancel}
        className="cursor-pointer rounded-sm px-1.5 py-0.5 text-label text-ink-faint transition-colors duration-[--duration-fast] hover:text-ink"
      >
        取消
      </button>
    </span>
  );
}

