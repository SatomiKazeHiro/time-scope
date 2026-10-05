/**
 * 汇总页。范围状态 + 三段布局都在这里。
 *
 * 顶栏那个「范围」chip 本轮是**死控件**（spec §1.2）—— 真实的范围切换
 * 全部由热力图上的点击驱动。
 */
export default function SummaryPage() {
  return (
    <section aria-label="汇总" className="panel flex-1 p-4">
      <h1 className="panel-title">汇总</h1>
      <p className="mt-2 text-sm text-ink-faint">还没有内容。</p>
    </section>
  );
}
