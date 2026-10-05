# Time Scope — 工作进展与遗留问题

## ⛔ 先读这一节：看起来像 bug、但是故意的

**下面每一条都有人栽过并花时间改回来。动之前先读完，对不上就别动。**

| # | 看起来像什么 | 实际是什么 | 该怎么做 |
|---|---|---|---|
| 1 | `bucketSegments` / `sliceSegments` 看着像同一件事 | **完全不同，而且两个都删了。** `bucketSegments` 把整段塞进它跨越的每个桶（聚合），`sliceSegments` 才按桶边界切片。后来发现**类别模式根本不该被桶切**（段是引擎判定的活动边界），于是 `sliceSegments` 也没有调用方，一并删除 | 类别模式一个段 = 一个色块；切桶只发生在指标模式，用的是 `metrics.ts` 的 `bucketMetrics` |
| 2 | 类别模式下点粒度切换器，色块会被切成更多块 | **不该发生。** 类别模式完全不碰段，粒度只动底部刻度尺的大刻度 | `SegmentTimeline` 有一条测试遍历 0/10/30/60/120 五档粒度，断言 rect 数量恒为 1。别把它改回切段 |
| 3 | 八个类别各占一个色相更直观，该改 | **改不了。** 时间线上任意两类都可能相邻，必须过 all-pairs 色觉闸。枚举 3/4/5/6 色相全部组合，通过数 15/56、2/70、0/56、0/28；六色时最差一对在红色盲下 ΔE 仅 2.7，等同同色 | 色相只能承载三个层级，层级内用明度阶。别动 `CATEGORY_META` 的层级结构 |
| 4 | `study` / `life` 没有段内直标，是漏了 | **是算出来的。** 深色底下它们的配字只有 4.47 / 4.41:1，压在 4.5 线下；调深则对底色的标记对比跌破 3:1，蓝相在这条窄区间里无解 | 别"补上"。`inlineLabel: false` 是结构决策，见 `categories.ts` |
| 5 | `theme.css` 里浅色 token 写了两遍，该消除 | **消不掉。** 生产 CSP 是 `script-src 'self'`，不许内联脚本；用 JS 解析主题则每次启动闪一下 | 保留重复。`verify-palette.mjs` 第 0 项会逐条断言两份一致，改漏会被抓 |
| 6 | `--color-state-critical` 只有 3.67:1，该调深 | **只当图标用**，非文字，3:1 就是对的。`--color-state-warning` 才当文字（已脱敏角标），门槛 4.5:1 | 别改这个颜色。改用途时同步改 `verify-palette.mjs` 里的门槛，否则检查名不副实 |
| 7 | `idle` / `unknown` 对比度不到 3:1，该提亮 | **刻意压低的。** 它们不是类别，是"无"；一天 8 小时空闲**就该看起来是空的** | 别提亮。图例色块的 1px 内描边是给它们可见性用的 |
| 8 | `preview*.test.tsx` 不做断言，是坏测试 | **是视觉预览。** 依赖 `dist/assets` 的 CSS，没 build 就自动 skip | 别删。要跑先 `pnpm build` |
| 9 | `cargo test` 不带 `--workspace` 更快 | **只跑根包，0 个测试，显示绿色** | 永远带 `--workspace` |
| 10 | 验证采集要切到 Time Scope 自己的窗口 | `WINEVENT_SKIPOWNPROCESS` 会过滤自己的窗口，切自己不产生 focus 事件 | 这是设计不是 bug，切到别的程序 |
| 11 | 指标模式的色阶最低档太暗了，再压低一点 | **卡着两条线**：ordinal 的 2:1 底线（第 1 档是"散碎 0–30%"，是真实数据不能退到底色），以及要和"没有活动的桶"（不画、露轨道色）分得开。第一版 1.23:1 **两条都不过** | 现在 2.29:1 对面板 / 2.07:1 对轨道（浅色 2.07 / 1.90）。要改先重算这两个数（MASTER §2.5） |
| 12 | 蓝（`study`）和紫色阶太近 | **实测几乎同色**：浅色 `#3d98ed` ↔ 色阶2档 protan **ΔE 0.9**；深色 `#256abf` ↔ 色阶2档 deutan ΔE 2.5 | **不构成问题**：类别模式和指标模式互斥，永不同框。代价是模式切换必须一眼可辨，**别为了"顺手"把两种模式画进同一行**（校验器每次都会把这两个参照值打出来） |
| 13 | 挂机两小时，专注度却显示很高 | **曾经是真 bug。** `idle` 是一个类别，被当成桶内主导类别就算进了专注度 → 100%「高度专注」。空闲不是"不专注"，是"没在工作" | 已修：专注度分母改为**非空闲时长**，全是空闲的桶不涂色，头部平均只对有活动的桶加权。回归测试 `focus ignores idle time entirely` |
| 14 | 热力图只有 5 档、0 档是轨道色，看着像「少了 6 档」 | **0 档是「0 活跃」**，就该看起来是空的。与 MASTER §2.5「没有活动的桶不画，露出轨道色」同一处理 | 别给 0 档也上紫阶 |
| 15 | 选中框的宽窄是随手调的 | **框宽 = 跨的格数**（日 1 / 周 1 列 / 月 N 列），框的宽窄就是档位标识 | 别改成逐格描边 |
| 18 | 热力图只有 5 天数据时就只画 2 列、1 个月份标签，像渲染坏了 | **墙铺固定 53 周**（GitHub 的形状），最后一格是今天，窗口内没数据的日子画成空的 | 别改回「按数据跨度」 |
| 17 | 组件里写 `var(--ink)` / `var(--line)` 看着像能用 | **不存在**。真名是 `--color-ink` / `--color-line`。不可解析的 var() 让属性失效、`fill` 退回黑色，深色底上直接隐形 | 两个组件的测试会读 `theme.css` 逐个 `var()` 断言存在 |
| 16 | 「按天分组」用 SQL 的 `localtime` 在 DST 地区会差一小时 | **跟随全项目既有的固定 offset 假设**（`day_range_ms` 同款）。单做 DST 正确会制造「只有汇总页对、其他页错」 | 用户不在 UTC+8 时修；见 §4.2 |

### 待重构（现在别动，但确实该做）

| 项 | 现状 | 为什么现在不做 |
|---|---|---|
| **「还没规则覆盖的程序」提示** | 内置 15 条规则只覆盖通用软件（IDE / 终端 / Office / 浏览器 / 微信 QQ / Slack / Steam…），用自己那套工具链的人会有**三成以上时间落进 unknown**。界面上只有「未分类 33%」这个数字，没法行动 | 曾实现在当日汇总面板里（`dbf4949`），**已回退**（`83d9b9e`）：那张表回答的是"你的配置缺了什么"，与汇总面板回答的"今天干了什么"是两件事，而且会把面板撑高混掉两件事。做在**设置 / 规则页**上才对 —— 那页还不存在 |
| **扩内置默认规则** | 上面那个问题的另一个解法：从源头把 Zed / DBeaver / FinalShell / HBuilder X 这类加进 `DEFAULT_RULES_TOML` | 是改产品默认口味，需要本人列清单；且换台电脑又漏 |
| 浅色 token 重复 37 个 | 见上表第 4 条 | 等有人愿意动 CSP |
| `EventDetail`（`EventDetail.tsx` 里的 deprecated 导出） | 旧的原始事件详情面板，`SegmentDetail` 上线后就退役了 | 无调用方，可删；但和上面几条一起清比较合适 |
| CI 的 Rust job 从没在 runner 上验证过 | 已绿过多次，但 `windows-latest` + pnpm 12 的组合长期没变过 | 改动依赖时顺手验证 |
| 长时间段未对真实数据验证 | 预览里造过 6 小时连段，真机上没专门看过 | 需要用户手上真有那种数据 |

---

**记录时间**：2026-10-05（新增「汇总」页：指标 + GitHub 式热力图 + 范围下钻，见
[`specs/2026-10-05-summary-page-design.md`](specs/2026-10-05-summary-page-design.md)）
上一条：2026-10-03（第三步「打磨」完成 + UI 设计系统落地，在 `phase1-polish` 分支）
**分支**：开发在 `phase1-polish`（**未合并**）；`main` 停在第三步「打磨」之前
**状态**：基本可用。第三步「打磨」已实现并通过人工验证清单
（[清单](plans/2026-10-02-phase1-polish-verification.md)）；随后又做了一轮
**UI 设计系统**（见 §1.1），**尚未合并进 `main`**——按计划等本阶段功能开发完再合。
**对应计划**：
- [`2026-10-01-phase1-scaffold.md`](plans/2026-10-01-phase1-scaffold.md)（骨架，11 task ✓）
- [`2026-10-01-phase1-engine.md`](plans/2026-10-01-phase1-engine.md)（引擎，11 task ✓）
- [`2026-10-02-phase1-polish.md`](plans/2026-10-02-phase1-polish.md)（打磨，10 task ✓）
**对应设计**：
- [`2026-10-01-time-scope-phase1-design.md`](specs/2026-10-01-time-scope-phase1-design.md)（功能）
- [`../../design-system/time-scope/MASTER.md`](../../design-system/time-scope/MASTER.md)（UI）

---

## 一、现在处于什么阶段

Phase 1 分三步。**三步都已完成**，另加一轮 UI 设计系统（§1.1）。

| 步骤 | 内容 | 状态 |
|---|---|---|
| 1. 骨架 | Tauri+React 脚手架、窗口/空闲采集 → SQLite → 前端按天查原始事件 | ✅ 完成 |
| 2. 引擎 | engine crate（segmenter 状态机 + TOML 规则分类）→ 分类色块 + 粒度切换 + 汇总 | ✅ 完成 |
| 3. 打磨 | 托盘/自启/单实例、标题脱敏、锁屏采集、参数配置化 | ✅ 完成 |
| + | UI 设计系统 | 色调板、时间线可读性、深浅主题（§1.1） |
| + | **汇总页** | 侧边栏新入口。指标横条 + 监控热力图 + 范围下钻 + 窗口标题排名（§1.2） |

**现在能做的**（基本可用了）：

- 后台常驻采集窗口切换与输入活动，本地落 SQLite
- 按 `%APPDATA%/time-scope/rules.toml` 自动分类（16 条内置默认规则，可编辑）
- 归并成 ActivitySegment：分类着色时间线 + 10/30/60/120 粒度切换 + 当日汇总
- 点开任意段看起止时间、应用、置信度、分类依据、证据条数
- 翻看任意历史日期（首次查看会按需重放，把上次崩溃留下的孤儿事件补成分段）
- 规则文件改坏不崩溃，退回内置默认并保留用户的文件
- **关窗口最小化到托盘**（首次问一次并记住），托盘菜单可开窗口 / 切自启 / 退出
- 开机自启（默认关，托盘里主动开），与 Windows「启动应用」状态保持一致
- 锁屏与合盖记成 idle，不再把 8 小时锁屏算成 8 小时活跃
- 行为参数（空闲阈值、心跳窗口、宽限、关窗行为）在 `config.toml` 里可调
- **深/浅/跟随系统三态主题**（页头切换，选择会记住）
- **时间线三态指标**：类别 / 专注度 / 切换次数（同一行轨道；粒度只在指标模式下是桶宽）

**现在做不到的**：见 [四、遗留问题清单](#四遗留问题清单)。

### 1.2 汇总页（2026-10-05）

**为什么做**：现有界面只有「日」一个尺度。它回答「我今天在干什么」，不回答
「我总共监控了多久」「作息稳不稳」「这段时间注意力在哪些窗口上」。

**架构**（详见 spec §7.3）：三个 IPC 按数据成本分层——

| 命令 | 数据源 | 何时发 | 实测 |
|---|---|---|---|
| `get_daily_calendar` | `activities` | 挂载时**一次** | 毫秒 |
| `get_summary` | `activities` | 随选中范围变 | 毫秒 |
| `get_top_titles` | `events` + `json_extract` | **懒加载**（面板进视口） | 199ms / 5 天 → 一年约 15s |

七项指标里六项走 `activities`（年 33k 行，毫秒级），只有窗口标题排名走
`events`（年 3.7M 行）。所以**只有它需要懒加载**，而且默认范围是「全部」，
最贵的路径恰好是最常走的那条。

**三个决定**：

1. **热力图固定不动**，永远渲染全部数据；点击只选出一个范围（跨格整块矩形
   框出），顶部指标随之重算。「框的宽窄就是档位标识」——日 1 格 / 周 1 列 /
   月 N 列。
2. **窗口标题在 Rust 侧归一化后再排名**。原始 Top 6 全是 Edge 折叠标签组的
   计数器变体（`和另外 31/32/33 个页面`），统计的是「计数器变过几次」而不是
   「你在看什么」。四条剥壳规则把 1021 个唯一标题收敛到 563 个。
3. **不引入图表库**。圆环是 4 个 `<circle>` + `stroke-dasharray`，
   热力图和 24h 条都是手写 SVG。产物 262 KB → 276 KB，没有 d3。

**已知代价**：未分类占 38%（真实数据），`无标题`/`New Tab` 占标题排名大头
（Edge 空白页，如实显示不是 bug）。见 spec §9。

### 1.1 UI 设计系统（2026-10-03，不在原三步计划内）

四个提交：`55c636d` 基调与色板 → `0b8d253` 深浅主题 → `77cd913` 时间线可读性
→ `21d4ed0` 粒度切换器接上时间线 + 活跃/空闲比。
规范与理由在 [`design-system/time-scope/MASTER.md`](../../design-system/time-scope/MASTER.md)。

接手前要知道的三件事：

1. **八个类别各占一个色相过不了色觉安全闸。** 时间线上任意两类都可能相邻，
   所以色板必须按 all-pairs 校验。枚举 3/4/5/6 色相的全部组合，通过数是
   15/56、2/70、0/56、0/28 —— 六色时最差一对在红色盲下 ΔE 仅 2.7，等同同色。
   现在的方案是**色相只承载三个层级（专注/消耗/社交），层级内用明度阶分具体类别**，
   idle/unknown 走中性灰且刻意压到 3:1 以下（让"无活动"读起来就是空的）。
2. **深浅两套色板是各自选定的，不是互为反色。** 深色底上更显眼 = 更亮，
   浅色底上更显眼 = **更深** —— 层级的主次方向会跟着底色翻转。
   浅色底对白底的对比度余量紧得多，次档被 3:1 的标记线卡在 3.02–3.06:1。
3. **色值真相只有一处**：`src/styles/theme.css`。组件通过 `var(--color-cat-*)` 引用。
   改色板前后必须跑 `node design-system/time-scope/verify-palette.mjs`（exit 0 才算过），
   它会校验两套色板 + 断言浅色 token 的两份副本逐条一致。

## 二、代码规模与状态

```
Rust  37 文件 / 5854 行（不含测试）  TS/TSX  19 文件 / 2404 行（不含测试）
测试  299 Rust + 242 前端 = 541 条用例，全绿，0 warning
提交  46 个（main..phase1-polish），**未合并**
```

```
time-scope/
├── design-system/time-scope/    UI 基调与组件规范（MASTER.md）
│   └── verify-palette.mjs        色板校验脚本（改色后必跑，exit 0 才算过）
├── src/                          React 前端
│   ├── styles/theme.css          设计 token + 深浅两套主题（色值唯一真相）
│   ├── design/categories.ts      类别元数据：中文名、层级、CSS 变量引用
│   ├── design/useTheme.ts        深/浅/跟随系统 三态
│   ├── components/SegmentTimeline    24h 分类着色时间线（SVG + 底部刻度尺）
│   ├── components/GranularityPicker  10/30/60/120 分钟粒度切换
│   ├── components/DaySummary         当日各类别时长与占比（同时充当图例）
│   ├── components/EventDetail        选中段的详情
│   ├── components/ThemeToggle        主题切换按钮
│   ├── components/MetricPicker        类别/专注度/切换次数 + 色阶图例
│   ├── lib/bucket.ts                 桶边界 / 汇总 / 时长格式化（纯函数）
│   ├── lib/metrics.ts                每桶的专注度 / 切换次数等指标（纯函数）
│   ├── preview.test.tsx              整页视觉预览（3 指标 × 2 主题）
│   ├── preview-summary.test.tsx      汇总页视觉预览（最坏情况数据 × 2 主题）
│   ├── preview-timeline.test.tsx     碎片化时间线预览
│   ├── lib/summary.ts                热力图布局 / 色阶 / 范围推导（纯函数）
│   ├── views/SummaryPage.tsx         汇总页：范围状态 + 三段布局
│   └── types.ts                      ActivitySegment 的 TS 镜像 + IPC
└── src-tauri/
    ├── src/lib.rs                 Tauri 壳：commands、线程编排
    ├── src/date_range.rs          本地时区按天半开区间
    ├── src/rules.rs               rules.toml 加载 / 首次拷贝 / 容错
    ├── src/engine_runtime.rs      纯逻辑、零 IO
    ├── src/engine_thread.rs       唯一做 IO 的桥
    ├── src/redact.rs              窗口标题脱敏（spec §11）
    ├── src/day_replay.rs          按需重放任意一天（补上次崩溃的孤儿事件）
    ├── src/exit_flush.rs          退出时 flush 队列
    └── crates/
        ├── core/                  Event / EventType / 序列化，零 IO
        ├── engine/                reduce() 状态机（纯库，边界有测试守着）
        ├── storage/               版本化迁移、BatchWriter、activities/evidence
        └── collector/             window hook、input idle、consumer（事件总线）
```

依赖方向单向无环：`core ← engine`、`core ← storage ← collector ← app`。

> **`engine` 是纯库**：不碰文件、数据库、网络。`crates/engine/tests/boundaries.rs`
> 会在有人破坏这条时立刻变红（扫 Cargo.toml 依赖 + 扫源码里的 `std::fs`/`println!`）。

> **`theme.css` 是色值唯一真相**：组件通过 `var(--color-cat-*)` 引用，不抄 hex。
> Tailwind 默认调色板被清空了，写错成 `bg-blue-500` 会直接构建失败。

---

## 三、已验证 / 未验证

### 自动化已覆盖（可随时重跑）

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace   # 299 passed
pnpm test                                                      # 242 passed
pnpm build                                                     # 无 tsc 报错
node design-system/time-scope/verify-palette.mjs               # 色板校验，exit 0
```

> ⚠️ `cargo test` **必须带 `--workspace`**。`src-tauri/Cargo.toml` 同时是 workspace 根和应用包，
> 不带的话只跑根包（0 个测试）并显示绿色——实现过程中真的被这个坑过。

> ⚠️ `preview*.test.tsx` 是**视觉预览不是断言**，需要先 `pnpm build`（依赖 `dist/assets` 的
> CSS），否则会自动 skip。它们的存在是为了把真实组件渲成静态页面给 headless Chrome 截图 ——
> **校验器只管颜色不管排版**，标签碰撞、几何溢出、面板比例只能用眼睛看。

重点覆盖：engine 纯库边界、短段合并、幽灵段防护、grace 抖动吸收、u32 tick 回绕、
大小写不敏感规则匹配、rules.toml 容错、重放幂等、半开区间日期运算、分桶与汇总、
主题三态循环与持久化、此刻游标只在今天出现且不吃鼠标事件、刻度尺数量与两种高度。

### 真实 Windows 上已实测

- 应用启动、生成 `rules.toml`、建库、`user_version=1`
- 采集到真实事件并落库（105 events → activities + evidence）
- 进程名大小写不敏感、路径只比文件名
- 规则文件改坏 → 退回内置默认，不崩溃、不覆盖用户文件
- 重启不丢数据、正常退出时 flush 队列
- UI 设计系统三个提交已在真机看过（2026-10-03），深浅主题与时间线观感确认可用

### 仍需人工确认

`docs/superpowers/plans/2026-10-01-phase1-engine-verification.md` 第 1–8 步未勾：

- [ ] 分类着色（VS Code 蓝 / 浏览器橙 / 微信青 / 未规则浅灰）
- [ ] 粒度切换（10/30/60/120，且不重查后端）
- [ ] 当日汇总
- [ ] 跨天与重放（段不跨天重叠）
- [ ] 规则改坏容错
- [ ] 常驻开销（CPU≈0、内存<80MB）

> 本次自动化用的是 `AppActivate` 切窗口，它每次都激活**同一个**窗口，
> 所以真实运行只覆盖到单一应用。**多类别着色必须人工确认。**

> **UI 那轮的人工确认是有边界的**：确认了整体观感，但长时间段（比如连续 6 小时的
> work）在真实数据里的表现没专门看过 —— 验证用的 mock 最长 3 小时。

---

## 四、遗留问题清单

### 4.1 已修掉的问题（存档，勿再回头看）

两轮 review 共提出 6 + 3 条，全部修复（`b4931bb` / `676b701` / `40a0d18` /
`19d9233` / `ebadd45` / `37bbbdc`）。

| 问题 | 影响 | 状态 |
|---|---|---|
| 应用启动后心跳堆出 "unknown / 无应用" 幽灵段 | 每次启动多一个噪声段 | ✅ |
| `min_segment_duration` 只延迟落库、从不合并 | spec §7.3 未实现，64ms 噪声段会落库 | ✅ `fold_short_segments` |
| **时间线永不自动刷新** | 常驻应用看着像坏了 | ✅ 5s 轮询 |
| **跨零点的段两天都看不到** | 凌晨查看昨天“缺一块” | ✅ 改为区间相交 |
| **历史日期的孤儿事件永不分段** | 上次崩溃后翻到那天是空白 | ✅ 按需重放 |
| review Minor 5（段 id 碰撞） | — | ⛔ **撤回**，见下 |
| **窗口标题明文落库**（spec §11） | 隐私承诺与实现的差距 | ✅ `[[redact]]` 正则，入队前脱敏 |

**撤回一条 review finding**：我说 `seg-{start_at}` 的 id 可能碰撞、`INSERT OR REPLACE`
会静默覆盖。仔细想下来这不成立 —— grace 吸收是“复活前一段、丢弃新开的那段”，
所以不会有两个活着的段共享起点；重放又会先删当天。能为它写的测试根本触发不了，
所以我把那个测试撤了，而不是留一个永远不会失败的测试。

### 4.2 剩余问题（按优先级）

| 优先级 | 问题 | 现状 | 代价 |
|---|---|---|---|
| ✅ | ~~窗口标题明文落库~~ | **已实现**（`[[redact]]` 正则，入队前脱敏） | 历史数据不回溯改写，但重放时也会脱敏 |
| ~~关窗口即退出~~ ✅ | 托盘 + 自启 + 单实例已接（spec §12） | 第三次启动「打磨」完成 | — |
| ~~锁屏/睡眠不采集~~ ✅ | WTS + 电源通知已接（spec §5.3） | 监听自建 message-only 窗口 | — |
| ~~只有深色主题~~ ✅ | 深/浅/跟随系统三态（§1.1） | 两套色板各自选定并校验 | 启动时可能有一次主题闪烁（生产 CSP 不许内联脚本，见 MASTER §3.1） |
| ~~八个类别各占一个色相~~ ✅ | 改成 3 层级色相 + 明度阶（§1.1） | all-pairs 校验通过 | 同层级内两个类别区分度有限，靠图例+直标+浮层三条冗余通道兜底 |
| ~~时间线碎片看不清~~ ✅ | 轨道可见 + 最小段宽 2.5 单位（§1.1） | 碎片化数据实测 | 最短那批段被画得比实际宽（宁可略失真，不可看不见） |
| ~~粒度切换器形同虚设~~ ✅ | `sliceSegments` 真按桶边界切段（§1.1） | 刻度尺大刻度也跟着粒度变 | 碎片日上四档差别不大，价值在长时段 |
| ~~当日汇总缺活跃/空闲比~~ ✅ | 已补（spec §10） | `unknown` 算活跃 | — |
| ✅ | ~~**汇总页的设置界面**~~ | 「汇总」页已实现（指标 / 热力图 / 范围下钻 / 标题排名）。**设置页仍是空壳** | spec §9 记着它的 4 个已知限制 |
| ⚪ 已知限制 | **没有设置界面** | spec §9 的 `get_config` / `set_config` 未实现，Rust 侧只有 `get_segments` 和 `get_segment_titles`；改 `config.toml` 只能手改文件 | 要做得先实现 IPC 暴露 + 配置写回（得跟 `rules.toml` 已有的一套容错对齐） |
| 🟠 中 | **行为参数写死** | 已改 `config.toml` 可调，**无热重载** | 改完要重启；`EngineConfig` 是构造注入的，热重载要重放当天 |
| 🟡 低 | `segment-updated` 推送（spec §9） | 5 秒轮询顶着 | 段多时整表重查浪费，但轮询够用 |
| 🟡 低 | 规则热重载（spec §7.2） | 改完重启生效 | spec 提了但没定义行为，实施前得定 |
| ⚪ 已知限制 | 空闲阈值 300 秒 | 写死在 collector 参数里 | 验证 idle 要停 5 分钟；可改成配置 |
| ⚪ 已知限制 | 强杀丢最后几秒 Event | 内存队列，spec §8.1 明确接受 | 正常退出已显式 flush，只有崩溃会丢 |
| ⚪ 已知限制 | CI 从没绿过 | `.github/workflows/ci.yml` 已加（`abb588e`），**但本机只在本地跑过** | Tauri/Win32 依赖在 Linux 上装不全，Rust job 必须用 windows runner |
| ⚪ 已知限制 | 固定窗口 1200×700 | 未做多分辨率适配 | 时间线用 viewBox + width:100%，缩放不会算坏 |

### 4.3 Schema 变更（未发布期，无需迁移脚本）

`activity_evidence.event_id` 已从 `REFERENCES events(id)` 改为**软引用**。
原因：采集链路是「Event 压进内存队列 5s 后落库」与「engine ingest 后可能立即产段」并行，
段会先于它引用的 Event 落库，外键必然违反。`activity_id` 的外键保留。

改动直接编辑了 SCHEMA_VERSION=1 的迁移（本机无真实用户数据）。任何在此之前建过的库
需要删除重建：`rm -rf "$APPDATA/time-scope"`。

### 4.4 计划文档的偏差（均已修并同步回计划）

- windows-rs 0.58 的 API 与计划差 5 处（`SetWinEventHook` 在 `UI::Accessibility`、
  `WINEVENTPROC` 7 参数、`EVENT_*` 裸 u32…）
- Task 5 的 grace 代理判定是错的 → 改为"复活前一段"（裁定 C）
- 上下文每事件重建 → 改为沿用 `current_context`
- 落库门槛改为 `max(min, grace)`（裁定 D）
- `min_segment_duration` 缺"合并"那一半 → 新增 `fold_short_segments`（裁定 E）
- Task 9 删旧类型导致构建在 Task 10 之前挂掉
- 一个代码块少收尾 fence，导致后续 task 的 brief 全部提取失败

### 4.5 我自己犯的错（值得记住）

- **Task 9（骨架）**：手写测试期望值连错三次，其中"本地午夜比 UTC 早还是晚 8h"是真的符号错误。
  测试抓住了它们，我改的是测试不是代码。
- **Task 6（骨架）**：差点把"0 个测试全绿"当成通过。
- **Task 9（引擎）**：`toml::from_str::<Vec<RawRule>>` 编译不过（TOML 根永远是 table）。
- **Task 4（引擎）**：测试用 `context_of` 的结果去比"已有 context"，永远不可能过。
- **Task 7（引擎）**：实现和测试写在同一文件，跳过了 RED；后来补做。
- **Task 11（引擎）**：边界测试匹配到了注释里的"刻意没有 tauri"，假阳性。
- **Task 9（引擎）**：我照计划整块替换 `types.ts`，删掉还在被引用的类型，
  导致 Task 10 之前构建挂掉。教训：**计划不该在两个 task 之间把构建搞坏**。
- **Task 5（引擎）**：合并短段时漏判"pending 是否为空"，把 1 秒的段并进了 20 秒后的段，
  跨过了中间的 19 秒。教训：**"紧邻"要真的紧邻**。
- **最终 review**：我说"启动瞬间的事件会被漏掉"——**这是错的**。采集在 bootstrap 之后才启动，
  那个窗口里根本没有事件。顺着查下去才找到真正的洞（bootstrap 只重放今天）。
  教训：**报一个 finding 之前先确认它真的存在**。

### 4.6 UI 那一轮（2026-10-03）犯的错

- **用 mock 数据验收 UI。** 14 个整段的干净数据看起来挺好，真实的一天是 125 个
  1–3 分钟的碎片。轨道不可见、碎片是发丝线、浅色下间隙隐形——**三个问题在干净数据下
  一个都不显形**。教训：**造数据要按最坏情况造，不是按理想情况造**。
  （现在有 `preview-timeline.test.tsx` 专门造碎片数据。）
- **浅色色板第一版把「消耗」层级的主次写反了。** 校验器报 CVD 掉进 6–8 保底带才暴露。
  教训：**色板这种事不要靠推导完直接抄，要让校验器过一遍**。
- **第一版搜索选出了土黄色**（色相飘 44°）。低饱和度下色相是病态的，
  搜索时必须同时设 chroma 下限。教训：**生成颜色要在 OKLCH 空间做并守住 C 的下限**。
- **给轨道加 `overflow-hidden` 做圆角，把悬停提示裁掉了一半。** 提示框是浮在轨道**上方**的。
  教训：**圆角裁剪和浮层不是同一层的事**。
- **加了刻度尺之后段内标签下移蹭到刻度。** 标签 `top-1/2` 是相对整个容器算的，
  而容器底下多了刻度尺。教训：**绝对定位的参照物要写死，别让它随兄弟元素高度漂**。
- **`state-critical` 一开始被我按 4.5:1 判，结果它其实只当图标用（3:1 就够）**，
  差点去改一个本来正确的颜色。教训：**门槛要按实际用途定，并把这个理由写进检查里**，
  否则检查名不副实还会诱导人改错东西。
- **接上粒度之后，默认视图的段内直标全没了**，而直标是 §2.3 定的无障碍兜底通道。
  30 分粒度下每块只有约 21 单位，低于 46 的门槛。教训：**改动会让某个阈值失效时，
  要重新检查依赖那个阈值的其他机制** —— 数值本身没坏，是它下面还有别的东西靠它撑着。

---

## 五、关键决策（ruling 摘要）

完整清单在 git 历史与 `.superpowers/sdd/`（gitignored）。最重要的几条：

| 决策 | 原因 | 代价 |
|---|---|---|
| engine 零 IO、零 Tauri | spec §4 硬要求；可重放的前提 | 边界靠测试守 |
| `EngineRuntime` 也不碰 DB | 状态机可脱离 DB 单测 | IO 全在 app 层 |
| collector 用第二条 channel 转发 Event | collector 不认识 engine | consumer 多一个参数 |
| `ActivityContext` 沿用而非重建 | 否则心跳冲掉 application，每心跳切一段 | 无 |
| grace 吸收要"复活前一段" | "不切段"会留下错误 context | 需要 previous_application |
| 落库门槛 `max(min, grace)` | 吸收只能复活未落库的段 | 修了 spec 默认值 30<60 的隐患 |
| 短段找不到长邻居就压着 | 宁可不并，不可跨时间并错 | 孤立方段延迟最多 3 倍门槛 |
| `SharedConn = Arc<Mutex<Connection>>` | `Connection` 是 `Send` 非 `Sync` | 每次读写多一次锁 |
| `idle_seconds` 用 u32 + `wrapping_sub` | `dwTime` 是 u32，49.7 天回绕 | 用 u64 会"永不 idle" |
| `payload` 只存 `EventType` JSON | id/timestamp/type 各有独立列 | 存全量会有两个真相来源 |
| evidence 用软引用 | 段先于 Event 落库 | 失去引用完整性 |
| CSP 生产严格 + devCsp 放宽 | Vite dev 注入内联 script | 需 tauri-utils 2.10+ |
| **色相只承载 3 个层级，层级内用明度阶**（§1.1） | 八色平铺过不了 all-pairs 色觉安全闸，六色时最差一对 ΔE 2.7 | 同层级内两类别区分度有限，靠三条冗余通道兜底 |
| **深浅色板各自选定，不互为反色** | 深底"更显眼"=更亮，浅底"更显眼"=更深，方向会翻转 | 两套都要各自跑校验 |
| **界面层不借用数据色相** | 按钮一旦是蓝色，蓝色在时间线上就不再只意味着 work | 选中/焦点/主按钮全用明度表达 |
| **`study`/`life` 两个主题下都不打段内直标** | 深底下浅字只有 4.47/4.41:1，蓝相无解 | 浅色下其实够，但让界面结构随主题漂移更糟 |
| **`inlineLabel` 在 TS、标签颜色在 CSS** | 标签该不该打是结构决策，与配色无关 | 多一个 token 要维护 |
| **游标红单开 token，不复用 `state-critical`** | 借用会让"当前时刻"读成"出错了" | 多一个 token |
| **浅色 token 写两遍 + 机器断言同步** | 生产 CSP `script-src 'self'` 不许内联脚本，消不掉重复 | 漏改一份会被 `verify-palette.mjs` 第 0 项抓住 |
| **首尾相接的同类别的段之间用 1 单位细缝** | 那不是活动边界，只是引擎把同一次活动切成了两截（标题变了） | 与「不同活动之间 1.5 单位」的视觉差很细，用户未必说得清 |
| **段内直标按整段判宽、贴整段开头、只标第一块** | 30 分粒度下每块 21 单位 < 46 门槛，按块判宽等于默认视图一个标签都没有 | 直标位置不再严格居中于色块 |
| **`unknown` 计入活跃而非空闲** | 它是"没分类出是什么"，不是"没在做事" | 分类规则写得差的用户会看到偏高的活跃率 |
| **`bucketSegments` 保留不动** | `sliceSegments` 满足 spec §8.2；聚合语义也许汇总面板以后要用 | 它目前是死代码，见顶部待重构表 |

---

## 六、未来计划

### 6.1 Phase 1 第三步「打磨」（已完成，2026-10-02）

按优先级排。**每项都要先走 `superpowers:brainstorming` → `writing-plans`，出计划再动手**——
前两步计划的 API 假设错了 7 次（windows-rs 签名、time crate、TOML 结构、wasm 目标…），
第三步又记了 16 条裁定（见该 plan 的「执行结果」），边写边改比事后补救便宜。

| 项 | spec | 为什么这个优先级 |
|---|---|---|
| ~~窗口标题脱敏~~ ✅ | §11 | 已完成（2026-10-01）：`rules.toml` 的 `[[redact]]` 正则，在 consumer 入队前替换为 `[redacted]` |
| ~~托盘常驻 + 开机自启 + 单实例~~ ✅ | §12 | 2026-10-02 完成。托盘三项菜单 + 确认框问一次 + 自启同步方向不对称；托盘建不起来时退回「关窗即退出」 |
| ~~SessionLock/Unlock + 睡眠唤醒~~ ✅ | §5.3 | 2026-10-02 完成。WTS + 电源通知，监听自建 message-only 窗口；合盖唤醒时间偏晚是已知限制 |
| **`segment-updated` 推送** | §9 | 目前 5 秒轮询顶着。数据量上去后（一天几百段）整表重查会浪费，但**优先级最低**——轮询够用 |
| **规则热重载** | §7.2 | spec 提了但没定义行为。当前是"改完重启生效" |

### 6.2 Phase 1 之后

| Phase | 内容 | 说明 |
|---|---|---|
| **Phase 2** | 浏览器扩展 + URL/domain 采集 | `Browsing` 维度才有意义。现在只能粗分整个浏览器进程 |
| **Phase 3** | 本地 LLM 语义分类、截图/OCR/Vision | 规则表穷举不了的场景。届时 Rust 侧走 HTTP 调本地模型（如 Ollama） |
| **Phase 4** | 复盘 Agent、日/周总结 | 依赖 Phase 2/3 的数据密度 |
| — | macOS / Linux | 架构已预留接口，Phase 1 不做 |

### 6.3 工程债

| 项 | 说明 |
|---|---|
| **CI 从没绿过** | `.github/workflows/ci.yml` 已加（`abb588e`），但**本机只在本地跑过全套**，没验证过 Windows runner + pnpm 12 的组合。第一次 push 上去要盯一眼 |
| **spec 再同步** | 已按"先判定谁错"的原则同步过（见 spec §16）。第三步做完后需再同步一次，**UI 那轮 spec 完全没动**（UI 规范在 `design-system/time-scope/MASTER.md` 而不是 spec 里） |
| **`vendor/` 不入库** | superpowers 那 1.9MB 本地包被 gitignore。队友若要用这套 skill 得自己装（见 §7.6） |
| **色板校验依赖外部 skill** | `verify-palette.mjs` 的校验逻辑来自 dataviz skill 分发的 `validate_palette.js`，不在本仓库。skill 没装时脚本会明确报错而不是悄悄跳过 |
| **长时间段未专门验证** | UI 验证用的 mock 最长 3 小时，真实数据里连续 6 小时的 work 没见过 |

---

## 七、给下一个接手的建议

1. **动手前先读**本文开头的 **⛔ 别动这里** 一节，再读 spec 的 §3（架构）、§5（采集层）、
   §7（引擎）、§9（IPC）、§10（前端）。**改 UI 之前另读
   [`design-system/time-scope/MASTER.md`](../../design-system/time-scope/MASTER.md)** ——
   §1 两条硬规则、§2 色板的来龙去脉、§7 组件约定都在那里。
2. **`cargo test` 永远带 `--workspace`**。`src-tauri/Cargo.toml` 同时是 workspace 根和应用包，
   不带的话只跑根包（0 个测试）并显示绿色。
3. **改 engine 前后跑 `cargo test -p activity-engine --test boundaries`**，
   那是"纯库"边界的守卫（扫 Cargo.toml 依赖 + 扫源码里的 `std::fs`/`println!`）。
4. **改任何颜色前后跑 `node design-system/time-scope/verify-palette.mjs`**。
   它从 `src/styles/theme.css` 读色值，校验两套色板的色觉安全与对比度，
   并断言浅色 token 的两份副本逐条一致。**改色不跑这个检查是最容易犯的错** ——
   UI 那轮浅色色板的主次写反了，就是它抓出来的。
5. **改 UI 前后跑 `pnpm build` 再跑 `pnpm vitest run src/preview*.test.tsx` 并截图**。
   校验器只管颜色不管排版；**造数据要按最坏情况造**（`preview-timeline.test.tsx`
   造的 125 个碎片段逼出了轨道可见性、最小段宽、间隙宽度三个决定）。
6. **验证采集时切到 Time Scope 之外的窗口** —— `WINEVENT_SKIPOWNPROCESS`
   会过滤自己的窗口，切到自己不产生 focus 事件，这是设计不是 bug。
7. **手工查库**要把 `time-scope.db`、`-wal`、`-shm` 三个文件一起复制出来再打开，
   运行中原地打开会报 `SQLITE_CANTOPEN`。
8. **本机 skill 环境**：`vendor/superpowers` 是本机安装的副本，不在版本库里。
   换机器或让队友用这套工作流时，需按 `vendor/superpowers/package.json` 里的
   `pi.skills` 声明重新安装。
9. **想复现决策过程**（60+ 条 ruling、每个 task 的测试日志），重跑
   `superpowers:executing-plans` 会重建 `.superpowers/sdd/` 目录。
   完整记录也在两份计划文档的"执行结果"一节里。
