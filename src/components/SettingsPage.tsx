/**
 * 设置页 —— 目前是空壳，只有标题。
 *
 * 预留给：读写 `config.toml`（空闲阈值 / 心跳窗口 / 关窗行为）、
 * 编辑 `rules.toml`（分类规则 + 脱敏规则）、以及「哪些程序还没规则覆盖」
 * 那个列表 —— 它回答的是"你的配置缺了什么"，不该塞进当日汇总面板。
 *
 * 那些都要先有 `get_config` / `set_config` 这两个 IPC 才有落脚点
 * （spec §9 列了，一直没实现），写回时得跟 `rules.toml` 已有的
 * 「文件写坏不覆盖、逐字段容错」那套对齐。
 */
export default function SettingsPage() {
  return (
    <section aria-label="设置" className="panel flex-1 p-4">
      <h1 className="panel-title">设置</h1>
      <p className="mt-2 text-sm text-ink-faint">还没有内容。</p>
    </section>
  );
}
