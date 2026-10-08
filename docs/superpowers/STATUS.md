# Time Scope — 工作进展与遗留问题

**一句话**：Windows 本地常驻时间账本。采集窗口与输入 → 规则引擎分成 Activity Segment
→ 时间线 / 汇总页展示。采集 → 分类 → 展示这条链已经通了，剩下的都是缺口和限制，
在 §4。

**跑起来**：`pnpm install && pnpm tauri dev`（详见 [README](../../README.md)）

**代码状态**（2026-10-07）

```
Rust  38 文件 / 7238 行    TS/TSX  23 文件 / 3649 行     （均不含测试）
测试  359 Rust + 327 前端 = 686 条，全绿
提交  main..phase1-polish 94 个，未合并
```

---

## 1. ⛔ 先读这一节：看起来像 bug、但是故意的

**下面每一条都有人栽过并花时间改回来。动手前先读完，对不上就别动。**

| # | 看起来像什么 | 实际是什么 | 该怎么做 |
|---|---|---|---|
| 1 | `bucketSegments` / `sliceSegments` 是同一件事的两个版本 | **完全不同，而且两个都删了。** 前者把整段塞进它跨越的每个桶（聚合），后者才按桶边界切片。后来发现**类别模式根本不该被桶切** —— 段是引擎判定的活动边界，切碎它既不增加信息又让「这段多长」要靠心算，于是切片失去了调用方 | 类别模式**一个段 = 一个色块**。切桶只发生在指标模式，用 `metrics.ts` 的 `bucketMetrics`。`SegmentTimeline` 有条测试遍历五档粒度断言 rect 数量恒为 1 |
| 2 | 类别模式下点粒度切换器，色块会被切成更多块 | **不该发生。** 类别模式完全不碰段，粒度只动底部刻度尺的大刻度 | 同上那条测试。别把它改回切段 |
| 3 | 八个类别各占一个色相更直观 | **过不了色觉安全闸。** 时间线上任意两类都可能相邻，必须按 all-pairs 校验。枚举 3/4/5/6 色相全部组合，通过数 15/56、2/70、0/56、0/28；六色时最差一对在红色盲下 ΔE 仅 2.7，等同同色 | 色相只承载三个层级，层级内用明度阶。别动 `CATEGORY_META` 的层级结构。改色前后跑 `verify-palette.mjs` |
| 4 | `study` / `life` 没有段内直标，是漏了 | **算出来的。** 深色底下配字只有 4.47 / 4.41:1，压在 4.5 线下；调深则对底色的标记对比跌破 3:1，蓝相在窄区间里无解 | `inlineLabel: false` 是结构决策。浅色下其实够，但让界面结构随主题漂移更糟 |
| 5 | 主题 token 写两遍，该消除 | **消不掉。** 生产 CSP 是 `script-src 'self'`，不许内联脚本；用 JS 解析主题则每次启动闪一下 | 保留重复。`verify-palette.mjs` 第 0 项逐条断言两份一致，改漏会被抓 |
| 6 | `--color-state-critical` 只有 3.67:1，该调深 | **只当图标用**，非文字，3:1 就是对的。`--color-state-warning` 才当文字（已脱敏角标），门槛 4.5:1 | 别改这个颜色。改用途时同步改 `verify-palette.mjs` 的门槛，否则检查名不副实 |
| 7 | 占比条用 `--color-ink` 才最重最清楚 | 反了。ink 对卡片 18.29:1（浅）/ 14.59:1（深），那是正文文字的重量 —— 画成实心块读起来是「一根黑线」不是「一段填充」，直接成为整页最重的元素 | 填充用 `--color-mark-fill`（ink-muted，7.23:1 / 6.63:1 对轨道）。**三个界面曾各用不同墨色画同一种填充** —— 遇到「同一个东西长得不一样」先怀疑缺 token |
| 8 | `idle` / `unknown` 对比度不到 3:1，该提亮 | **刻意压低的。** 它们不是类别是「无」；一天 8 小时空闲**就该看起来是空的** | 别提亮。图例色块的 1px 内描边是给它们可见性用的 |
| 9 | 侧边栏选中态用 `surface-3` 就行 | 浅色下 `surface-3` 与 `surface-1` 是**同一个 `#ffffff`**（1.000:1），选中态完全消失。`surface-2` 两边都只有 ~1.10:1 | 用 `--color-nav-active`，校验器第 8 项守着 ≥1.2:1 |
| 10 | 滚动条滑块用最浅的墨色 | `--color-ink-ghost` 浅色下只有 2.69:1，低于滚动条作为非文字 UI 元素的 3:1 下限 | 用 `--color-ink-faint`（5.29 / 4.02） |
| 11 | 色阶最低档太暗，再压低 | **卡两条线**：ordinal 的 2:1 底线（第 1 档代表「散碎的 0–30%」是真实数据不能退到底色），以及要和「没有活动的桶」分得开（那是不画的，露出轨道色）。第一版 1.23:1 **两条都不过** | 改色阶前先算这两个数（MASTER §2.5） |
| 12 | 蓝（`study`）和紫色阶太近 | **实测几乎同色**：浅色 `#3d98ed` ↔ 色阶 2 档 protan **ΔE 0.9** | 不构成问题 —— 类别模式和指标模式互斥，永不同框。代价是模式切换必须一眼可辨，别把两种模式画进同一行 |
| 13 | 挂机两小时，专注度却显示很高 | **曾经是真 bug。** `idle` 是一个类别，被当成桶内主导类别就算进了专注度 → 100%「高度专注」 | 已修：分母改为非空闲时长，全空闲的桶不涂色。回归测试 `focus ignores idle time entirely` |
| 14 | 图表拉高后刻度数字被压扁 | `preserveAspectRatio="none"` 会把 SVG 里的 `<text>` 一起横向压扁 | **文字一律不进会被拉伸的 SVG**，走 HTML 覆盖层。时间线与 24h 条都是这么做的，有测试钉住 |
| 15 | 面板标题写「24 小时时间线」 | 零信息 —— 面板里就是 24h 色带，轴还标着 00:00–24:00 | 那个位置放**读数**（段数 / 平均专注度），它描述的正是下面这块数据 |
| 16 | 范围 chip 长得像能点 | **它不是控件。** 范围由热力图点击驱动，这里只是念出当前选的是什么。曾经是 `<button aria-disabled>` 且带边框底色，和旁边真能点的按钮同款 | 读数就该长得像读数。测试断言它不是 `BUTTON` |
| 17 | 热力图图例里有「还没装」和「当天没活动」两格 | 前者是**关于应用安装日期的事实，不是活动数据**，画成色块会被当成另一种活动去读 | 「还没装」在热力图下方的提示行里，不在图例里。**两种「空」的视觉区分照旧保留** —— 合并成一种就是谎报装之前那些天也没活动 |
| 18 | 热力图只有 5 天数据时只画 2 列，像坏了 | **墙铺固定 53 周**（GitHub 的形状），最后一格是今天，窗口内没数据的日子画成空的 | 别改回「按数据跨度」 |
| 19 | 热力图 53 列写死 11px，右边空一大片 | 列宽用 `cqw`（容器查询单位）跟着容器走，显式给 7 行。不写行高时 `grid-auto-flow:column` 只排一行、整面墙高度塌 0 | 别把行高改回 auto |
| 20 | 组件里写 `var(--ink)` / `var(--line)` 看着像能用 | **不存在。** 真名是 `--color-ink` / `--color-line`。不可解析的 var() 让属性失效、`fill` 退回黑色，深色底上直接隐形 | 有测试读 `theme.css` 逐个 `var()` 断言存在 |
| 21 | 「按天分组」用 SQL 的 `localtime` 在 DST 地区会差一小时 | 跟随全项目既有的固定 offset 假设（`day_range_ms` 同款）。单做 DST 正确会制造「只有汇总页对、其他页错」 | 用户不在 UTC+8 时修 |
| 22 | 时间线 / 汇总 / 热力图对同一天算出不同的时长 | **三处必须同源**：命中按**区间相交**（`start_at < :end AND end_at > :start`）而不是 `start_at` 落在区间内；返回的起止**裁剪到查询区间** | 两条测试钉住。**改任何一处查询记得同时改另外两处** |
| 23 | `absorb` 里 `pending.last()` 看着就是「刚离开的那一段」 | **不是。** `keep` 段会 append 到 pending 末尾打破时间序，末位是十几秒前的老段 —— 复活它就**产出互相重叠的段** | 现在按**时间相邻**定位。`reduce` 尾部那行 `sort_by_key` 别删 |
| 24 | 保留期测试偶尔挂一条，像真 bug | **测试隔离问题。** `LAST_PRUNE_MS` 是**进程级**静态，测试并行时另一条测试写进去的时间戳会落进断言。实测约 8 次里挂 1 次 | 碰它的测试先拿 `retention::tests::exclusive()` 那把锁。**新测试只要碰它就必须拿** |
| 25 | 提交里写「UI 调好了」之类的话 | `classifiers`/分类等已改项在别处 | 用数据说话：改了什么、为什么、代价是什么 |

---

## 2. 已验证 / 未验证

### 自动化（可随时重跑，全绿）

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace    # 359 passed
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets   # 0 warning
pnpm test                                                       # 327 passed
pnpm build                                                      # 无 tsc 报错
node design-system/time-scope/verify-palette.mjs                # 色板校验，exit 0
```

> ⚠️ `cargo test` **必须带 `--workspace`**。`src-tauri/Cargo.toml` 同时是 workspace 根和应用包，
> 不带就只跑根包（0 个测试）并显示绿色 —— 实现过程中真被这个坑过。

> ⚠️ `preview*.test.tsx` 是**视觉预览不是断言**，要先 `pnpm build`（依赖 `dist/assets` 的 CSS），
> 否则自动 skip。**校验器只管颜色不管排版** —— 标签碰撞、几何溢出、面板比例只能用眼睛看。
> 改 UI 之后跑一遍，截图比读 diff 更快发现问题。

> ⚠️ `verify-palette.mjs` 的校验逻辑来自 dataviz skill 分发的 `validate_palette.js`，
> **不在本仓库**。找不到时脚本以 exit 2 明确报错，不悄悄放行；重调一次 `/dataviz` 即可恢复。

### 真实 Windows 上已实测（2026-10-06/07）

- 应用启动、生成 `rules.toml` 与 `config.toml`、建库
- 采集到真实事件并落库；进程名大小写不敏感、路径只比文件名
- 规则文件改坏 → 退回内置默认，不崩溃、不覆盖用户文件
- 重启不丢数据、正常退出时 flush 队列
- 托盘常驻 / 开机自启 / 锁屏 idle / 三态主题 / 时间线三态指标 / 汇总页 —— 均已在真机跑过

### 未验证

- `docs/superpowers/plans/` 下三份**人工验证清单共 92 项一项未勾**（骨架 27 / 引擎 32 / 打磨 33）。
  走的是真机但没按清单形式核对。清单本身也部分过时（写着旧 UI 的样子）。
- 长时间段（连续 6 小时的工作）在真实数据里的表现 —— 预览造过，真机没见过

---

## 3. 架构速览

```
Windows OS
  ├─ SetWinEventHook（窗口切换/标题变化）    事件驱动，零轮询
  ├─ GetLastInputInfo 轮询（idle + 心跳）    每 1s
  └─ WTS 锁屏 / Power 睡眠通知
        ↓ RawSignal channel
     consumer 线程 ─ 组装 Event（含脱敏）─┬→ BatchWriter → events 表
                                          └→ engine 线程 → activities 表
                                                          ↓
                                              React（时间线 / 汇总）
```

依赖方向单向无环：`core ← engine`、`core ← storage ← collector ← app`。

**三条不变量**（破坏时立刻有测试变红）：

1. **`engine` 是纯库** —— 不碰文件、数据库、网络。
   `crates/engine/tests/boundaries.rs` 扫 Cargo.toml 依赖 + 源码里的 `std::fs`/`println!`。
2. **`theme.css` 是色值唯一真相** —— 组件通过 `var(--color-cat-*)` 引用，不抄 hex。
   Tailwind 默认调色板被清空了，写错成 `bg-blue-500` 直接构建失败。
3. **IPC 读写的同源口径** —— 见「别动这里」第 22 条。

### 目录

```
design-system/time-scope/    UI 规范（MASTER.md）+ 色板校验脚本
src/
  styles/theme.css           设计 token + 深浅两套主题
  design/                    categories.ts 类别元数据 / useTheme.ts 三态主题
  components/                SegmentTimeline（时间线·三态指标）/ MetricPicker
                             ContributionWall（热力图）/ DonutChart / DayPartChart
                             RankedList / DaySummary / EventDetail / PageChrome
  lib/                       bucket.ts 切片+汇总 / metrics.ts 每桶指标 / summary.ts 热力图布局
  views/SummaryPage.tsx      汇总页
  preview*.test.tsx          视觉预览（整页 / 碎片时间线 / 汇总页）
src-tauri/
  src/                       app 壳：config / tray / rules / redact / 留存期 / 标题归一化 …
  crates/{core,engine,storage,collector}
docs/superpowers/
  specs/    三份设计快照（已实施，顶部有历史说明）
  audits/   2026-10-06 隐藏问题审计（16 条 finding 的完整证据）
  plans/    三份人工验证清单（未勾）
```

---

## 4. 剩余工作

### 4.1 功能缺口

| 事项 | 现状 | 要做得先解决 |
|---|---|---|
| **设置界面** | `get_config` / `set_config` 未实现，Rust 侧只有 `get_segments`、`get_segment_titles`。改 `config.toml` 只能手改文件 + 重启 | IPC 暴露 + 配置写回，写回要跟 `rules.toml` 已有的「坏文件不覆盖、逐字段容错」对齐。**用户 2026-10-07 决定暂不做** |
| **「哪些程序还没规则覆盖」** | 界面只有「未分类 38%」这个总数，没法行动 | 天然属于设置页。**曾在当日汇总里做过并被撤回**（用户：那张表答的是「你的配置缺了什么」，与汇总面板的「今天干了什么」是两件事） |
| **扩内置默认规则** | 15 条只覆盖通用软件。装了自己那套工具链的人（Zed / DBeaver / FinalShell / HBuilder X…）会有三成以上时间落进 unknown | 需要本人列清单；且换台电脑又漏 |
| `segment-updated` 推送 | 5 秒轮询整表 | 功能等价，段多了浪费。**优先级最低** |
| 规则 / 配置热重载 | 改完重启生效 | `EngineConfig` 是构造注入的，热重载要重放当天。spec 提了但**没定义行为**，动手前得先定 |

### 4.2 已知限制（决定不做）

| 事项 | 性质 |
|---|---|
| 同层级内两个类别区分度有限 | 色觉安全的算术后果（§1 第 3 条）。放宽只能砍类别数，不是调色 |
| 细短段（1–3 分钟）鼠标命中区只有 2–3px | 靠 8px 透明描边扩；已能用方向键精确走到。**用户 2026-10-06 决定先放一放** |
| 详情卡片的前后段导航不跨天 | 前端只持有当天段，跨天要么加缓存要么按需发 IPC，还要跟轮询对齐失效 |
| 强杀丢最后几秒 Event | spec §8.1 明确接受。正常退出已 flush，只有崩溃会丢 |
| 固定窗口 1200×700 | 无多分辨率适配。时间线用 viewBox，缩放不会算坏 |
| 启动时可能闪一下主题 | 系统浅色而上次选深色时。生产 CSP 不许内联脚本（§1 第 5 条） |
| 热力图 371 个 Tab 停靠点 | 每格约 13px，鼠标点得准，收益几乎全在无障碍。**用户 2026-10-06 决定先放一放** |
| 归类用固定 offset，DST 日差一小时 | 跟随全项目既有假设（§1 第 21 条） |

### 4.3 工程债

| 事项 | 现状 |
|---|---|
| **读操作仍与写路径共用一条连接** | 审计 A3 修法第 2 条未做：`get_top_titles` 跑那 20 秒时 `BatchWriter` 与 engine 线程照样写不进去。加 async 后界面不再冻结（触发丢数据的那条链断了），但内存队列在长查询期间仍会堆积 |
| **标题统计没有预聚合/索引** | `json_extract(payload,…)` 必须回表。5 天 199ms，一年约 15s。**约 3 个月（≈90 天）时重新评估** |
| 92 项验证清单未勾 | §2 |
| CI 只跑过基线配置 | 长期没变过依赖，改依赖时要盯一眼 |

---

## 5. 关键决策（只留不在「别动这里」里的）

| 决策 | 原因 | 代价 |
|---|---|---|
| engine 零 IO、零 Tauri | 可重放的前提 | 边界靠测试守 |
| collector 用第二条 channel 转发 Event | collector 不认识 engine | consumer 多一个参数 |
| `ActivityContext` 沿用而非重建 | 否则心跳冲掉 application，每心跳切一段 | 无 |
| grace 吸收要「复活前一段」 | 「不切段」会留下错误 context | 需要 `previous_application` |
| 落库门槛 `max(min, grace)` | 吸收只能复活未落库的段 | 修了 spec 默认值 30<60 的隐患 |
| 短段找不到长邻居就压着 | 宁可不并，不可跨时间并错 | 孤立方段延迟最多 3 倍门槛 |
| `SharedConn = Arc<Mutex<Connection>>` | `Connection` 是 `Send` 非 `Sync` | 每次读写多一次锁 |
| `idle_seconds` 用 u32 + `wrapping_sub` | `dwTime` 是 u32，49.7 天回绕 | 用 u64 会「永不 idle」 |
| `payload` 只存 `EventType` JSON | id/timestamp/type 各有独立列 | 存全量会有两个真相来源 |
| evidence 用软引用 | 段先于 Event 落库 | 失去引用完整性 |
| 重放「删当天 + 写当天」合成单事务 | 否则失败后那天永久空白 | `claim` 必须在成功之后 |
| `classifier_version` 由**内容**派生 | 只由条数派生时改规则版本号不变 | 不能用 SipHash，版本号会落库 |
| 汇总页三个 IPC 按数据成本分层 | 七个指标里六个走 `activities`（毫秒级），只有标题排名走 `events` | 最贵的那条恰好是懒加载的 |
| 热力图固定 53 周不随选中变 | 它是导航图，不是被导航的对象 | 见 §1 第 18 条 |
| 窗口标题在 Rust 侧归一化 | 放前端会让同一归一化值散成多行，合并不了 | — |
| 不引入图表库 | 圆环是 4 个 `<circle>` + `stroke-dasharray` | 产物 262 KB → 276 KB，没有 d3 的 540 KB |

---

## 6. 文档地图

| 文档 | 是什么 | 状态 |
|---|---|---|
| [README](../../README.md) | 入口：怎么跑、配置、目录 | 活 |
| 本文 | 交接：陷阱、已验证、剩余工作、关键决策 | 活 |
| [`design-system/time-scope/MASTER.md`](../../design-system/time-scope/MASTER.md) | UI 规范：色板推导、组件约定、token 理由 | 活 |
| [`audits/2026-10-06-hidden-issues.md`](audits/2026-10-06-hidden-issues.md) | 16 条 finding 的完整证据（真实库数据 + 触发序列） | 历史 |
| [`specs/`](specs/) 三份 | 设计快照，顶部已标注与现状的出入 | 历史 |
| [`plans/`](plans/) 三份 | 人工验证清单，92 项未勾 | 待用 |
| `.reference/` | 概念设计来源（Phase 1–4 路线图） | 外部 |
