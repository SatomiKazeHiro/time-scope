# 时迹（time-scope）

Windows 本地个人时间账本。采集使用活动 → 聚合为可解释的 Activity Segment →
时间线 / 汇总页展示。

**这个 README 只回答两件事：怎么跑起来、现在有什么。**
设计决策、陷阱、遗留问题分别住在另外两份文档里，见文末「文档分工」。

## 当前状态

采集 → 分类 → 展示整条链路已打通：

```
Windows 事件/输入 ─▶ collector ─▶ Event ─▶ SQLite(WAL)
                                          │
              activity-engine（纯库）◀───┘
              reduce(): Context → 规则分类 → Segmenter 状态机
                          │
                          ▼
                 ActivitySegment（category / confidence / evidence）
                          │
                          ▼
        get_segments ─▶ 24h 时间线（类别 / 专注度 / 切换次数三态）
                        + 粒度切换 + 当日汇总 + 详情面板
        get_summary  ─▶ 汇总页：53 周热力图 + 圆环 + 时段分布 + 标题排名
```

两个页面（**监控采集** / **汇总**）由左侧 72px 图标侧边栏切换，下层是主题与设置。
**设置页目前是空壳**，只有标题。

`rules.toml` 在 `%APPDATA%/time-scope/`，首次启动自动生成 15 条默认规则，
可自由编辑。改坏了也不会崩——退回内置默认并保留你的文件。

窗口标题在**入库前**按同一文件里的 `[[redact]]` 正则脱敏（默认不预置规则，需自己按需添加；写正则要用 TOML 单引号）：

```toml
[[redact]]
pattern = '客户\d+'
```

托盘常驻、开机自启（默认关）、单实例、锁屏/合盖采集、行为参数配置化均已完成。

行为参数在 `%APPDATA%/time-scope/config.toml`（首次启动自动生成；坏了会用默认值，且**不覆盖**你的文件）：

```toml
idle_threshold_s = 300   # 无输入多少秒算空闲（最小 10）
heartbeat_every_s = 10   # 心跳聚合窗口
close_behavior = "ask"  # ask = 问一次 / minimize = 最小化到托盘 / quit = 退出
```

**改 `config.toml` 目前只能手改文件 + 重启** —— 设置页是空壳，`get_config` / `set_config` 未实现。

点窗口的 ✕ 第一次会问一次（选什么就记住什么）；此后只隐藏到托盘，**托盘菜单的「退出」是唯一退出入口**。
锁屏与合盖记成 idle，不再把 8 小时锁屏算成 8 小时活跃。
主题按钮在**侧边栏下层**：跟随系统 → 浅色 → 深色，选择会被记住（存 localStorage 的 `time-scope.theme`）。

## 环境要求

- Windows 10/11
- Node.js ≥ 20（开发用 22）
- pnpm ≥ 10（开发用 12）
- Rust stable，target `x86_64-pc-windows-msvc`
- WebView2 Runtime（Tauri 依赖，Win11 自带；Win10 需从微软官网装）

UI 走 Tailwind v4 + lucide-react。

## 运行

```bash
pnpm install
pnpm tauri dev
```

首次运行会在 `%APPDATA%\time-scope\time-scope.db` 建库。

## 测试

```bash
# Rust：必须带 --workspace，否则只跑根包、core/engine/storage/collector 全部被跳过
cargo test --manifest-path src-tauri/Cargo.toml --workspace   # 359 passed

# 前端
pnpm test                                                       # 324 passed
```

## 代码结构

```
src/                     React 前端
  styles/theme.css       设计 token（色板 / 字阶 / 间距 / 圆角）+ 深浅两套主题
  design/categories.ts   类别元数据：中文名、层级、CSS 变量引用
  design/useTheme.ts     跟随系统 / 浅色 / 深色 三态
  components/                共 13 个（不含测试）
    Sidebar.tsx         72px 图标导航 + 主题/设置；上层两个页面，下层两个工具
    PageChrome.tsx      两页共用的页头外壳
    SettingsPage.tsx    空壳（等 get_config / set_config）
    SegmentTimeline.tsx 24h 时间线（SVG，类别/专注度/切换次数三态）
    MetricRow.tsx       顶部五个指标卡   MetricPicker.tsx 三态指标切换
    GranularityPicker.tsx 粒度切换（10/30/60/120 分钟）
    DaySummary.tsx      当日汇总        EventDetail.tsx 选中段的详情
    ContributionWall.tsx 53 周热力图
    DonutChart.tsx / DayPartChart.tsx / RankedList.tsx  汇总页的三个图表
  views/SummaryPage.tsx  汇总页
  lib/                   bucket 切片+汇总 / metrics 每桶指标 / summary 热力图布局
                       （同名 .test.ts 里有「每个 var(--x) 都必须在 theme.css 有定义」的守卫）
  types.ts               StoredEvent 的 TS 镜像 + IPC 封装 + 日期工具
src-tauri/               Rust workspace
  crates/core            Event / EventType / 序列化（无 IO）
  crates/engine          规则分类 + Segmenter（纯库，零 IO / 零 Tauri）
  crates/storage         SQLite schema、批量写入、按天范围查询
  crates/collector       Windows 采集：窗口 hook、输入 idle
  src/                   Tauri 应用壳：命令、线程编排、规则、留存期
design-system/time-scope/MASTER.md   UI 规范（色板推导 + 组件约定）
```

依赖方向单向无环：`core ← engine`、`core ← storage ← collector ← app`。

色值真相只有一处：`src/styles/theme.css`。组件通过 `var(--color-cat-*)` 引用，
不抄 hex。改色板前后跑 `node design-system/time-scope/verify-palette.mjs`。

## 还没做什么

按优先级，展开理由见 [`STATUS.md` §4](docs/superpowers/STATUS.md)：

| 优先级 | 缺什么 | 影响 |
|---|---|---|
| 🟠 | **设置界面** | `get_config` / `set_config` 未实现，改 `config.toml` 只能手改文件 |
| 🟠 | **哪些程序还没规则覆盖** | 界面只有「未分类」一个总数，没法行动。自然属于设置页 |
| 🟡 | 扩内置默认规则 | 15 条只覆盖通用软件，自带工具链（Zed / DBeaver…）会有三成以上时间落进 unknown |
| 🟡 | `segment-updated` 实时推送 | 现用 5 秒轮询整表，功能等价，段多了浪费。优先级最低 |
| 🟡 | 规则 / 配置热重载 | 改完要重启。`EngineConfig` 是构造注入的，热重载要重放当天，spec 提了但**没定义行为** |

## 启动慢（`pnpm tauri dev` 白屏）

**开发模式下**窗口会先白一段再出界面。实测数字（`node_modules/.vite` 缓存被清空 vs 已建立）：

| | 冷 | 热 |
|---|---|---|
| dev server 可访问 | 5.8s | 1.8s |
| **首个模块请求** | **81s** | **0.7s** |
| 其余 13 个模块 | 4.2s | ~2s |

全部是 **Vite 预构建依赖**，且只挂在第一个请求上。主要成本来自 `lucide-react`：
4242 个图标的 barrel，预构建后单个文件 1420 KB。

**这跟打包后的应用无关。** 生产产物是一个 283 KB 的 JS + 26 KB CSS，没有依赖爬取，
没有这段等待。

已经做的：`index.html` 里内联了一个首屏占位（`#root:empty::after`），
这段等待里窗口显示「时迹」而不是纯白。改不了耗时，但不让它看起来像坏了。

没做的，以及代价：

- **`optimizeDeps.include: ["lucide-react"]`** —— 试过，**无效**。实测第一个请求仍阻塞 38s。
  写进 `include` 并不能把爬取挪到启动阶段。
- **改成深导入**（`lucide-react/dist/esm/icons/activity.mjs`）—— 能让冷启动大幅缩短，
  但那是包内部路径，`package.json` 没有 `exports` 字段保证，lucide 升个版本就断。
  只为开发模式的速度不值得。
- **不预热 / 不清缓存** —— 平时什么都不用做，缓存热了就是 1 秒内。
  遇到长时间白屏，等它跑完即可，缓存建立后下次就不会再有。

## 文档分工

三份活文档各回答一个问题，别在 README 里堆细节：

| 文档 | 回答 | 什么时候读 |
|---|---|---|
| **本文** | 怎么跑起来、现在有什么 | 第一次接触这个项目 |
| [`STATUS.md`](docs/superpowers/STATUS.md) | **别改什么** —— 25 条「看起来像 bug 但是故意的」，加上遗留工作与关键决策 | **动手改代码之前** |
| [`MASTER.md`](design-system/time-scope/MASTER.md) | **为什么这样设计** —— 色板推导、组件约定、token 理由 | 改 UI 之前 |

`docs/superpowers/specs/` 是**历史快照**，顶部已标注不是现状说明，
只在追溯「当初为什么那么想」时才需要翻。

`docs/superpowers/audits/` 是 2026-10-06 的隐藏问题审计。
**注意它开头的「请按 §1 的顺序修」是当时的指示，不是现在的** ——
那 16 条多数已在 10-06/07 修掉，逐条现状见 [`STATUS.md`](docs/superpowers/STATUS.md) §4.3。

## 已知注意事项

- 采集时**不要**把时迹自己当作验证窗口：`WINEVENT_SKIPOWNPROCESS` 会过滤掉
  自己的窗口，验证 focus 采集要切到别的程序。
- 手工查库时请把 `time-scope.db`、`-wal`、`-shm` 三个文件一起复制出来再打开；
  应用运行中直接开原文件可能报 `SQLITE_CANTOPEN`。
- 强杀应用最多丢最后几秒 Event（内存队列，spec §8.1 明确接受）。正常退出会 flush。
- 归类按固定时区偏移（UTC+8）计算「一天」，DST 地区会差一小时。全项目一致的既有假设。
