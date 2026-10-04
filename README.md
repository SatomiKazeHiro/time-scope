# Time Scope

Windows 本地个人时间账本。采集使用活动 → 聚合为可解释的 Activity Segment → 时间线展示。

设计文档：[`docs/superpowers/specs/2026-10-01-time-scope-phase1-design.md`](docs/superpowers/specs/2026-10-01-time-scope-phase1-design.md)
实施计划：[`docs/superpowers/plans/2026-10-01-phase1-scaffold.md`](docs/superpowers/plans/2026-10-01-phase1-scaffold.md)

## 当前状态

**Phase 1 第二步完成，基本可用**——采集 → 分类 → 时间线整条链路已打通：

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
              get_segments ─▶ 24h 分类着色时间线 + 粒度切换 + 当日汇总
```

`rules.toml` 在 `%APPDATA%/time-scope/`，首次启动自动生成 16 条默认规则，
可自由编辑。改坏了也不会崩——退回内置默认并保留你的文件。

窗口标题在**入库前**按同一文件里的 `[[redact]]` 正则脱敏（默认不预置规则，需自己按需添加；写正则要用 TOML 单引号）：

```toml
[[redact]]
pattern = '客户\d+'
```

托盘常驻、开机自启（默认关）、单实例、锁屏/合盖采集、行为参数配置化均已完成（spec §12）。

行为参数在 `%APPDATA%/time-scope/config.toml`（首次启动自动生成；坏了会用默认值，且**不覆盖**你的文件）：

```toml
idle_threshold_s = 300   # 无输入多少秒算空闲（最小 10）
heartbeat_every_s = 10   # 心跳聚合窗口
close_behavior = "ask"  # ask = 问一次 / minimize = 最小化到托盘 / quit = 退出
```

点窗口的 ✕ 第一次会问一次（选什么就记住什么）；此后只隐藏到托盘，**托盘菜单的「退出」是唯一退出入口**。
锁屏与合盖记成 idle，不再把 8 小时锁屏算成 8 小时活跃。
页头「采集中」右边的按钮切换主题：跟随系统 → 浅色 → 深色，选择会被记住。

**尚缺**：`segment-updated` 实时推送、规则热重载——见文末"还没做什么"。

## 环境要求

- Windows 10/11
- Node.js ≥ 20（开发用 22）
- pnpm ≥ 10（开发用 12）
- Rust stable，target `x86_64-pc-windows-msvc`
- WebView2 Runtime（Tauri 依赖，Win11 自带；Win10 需从微软官网装）

UI 走 Tailwind v4 + lucide-react。色板见 [`design-system/time-scope/MASTER.md`](design-system/time-scope/MASTER.md)。

## 运行

```bash
pnpm install
pnpm tauri dev
```

首次运行会在 `%APPDATA%\time-scope\time-scope.db` 建库。

## 测试

```bash
# Rust：必须带 --workspace，否则只跑根包、core/storage/collector 全部被跳过
cargo test --manifest-path src-tauri/Cargo.toml --workspace

# 前端
pnpm test
```

## 代码结构

```
src/                     React 前端
  styles/theme.css       设计 token（色板 / 字阶 / 间距 / 圆角）+ 深浅两套主题
  design/categories.ts   类别元数据：中文名、层级、CSS 变量引用
  design/useTheme.ts     跟随系统 / 浅色 / 深色 三态
  components/SegmentTimeline  24h 横向时间线（SVG）
  components/EventDetail 选中段的详情
  types.ts               StoredEvent 的 TS 镜像 + IPC 封装 + 日期工具
src-tauri/               Rust workspace
  crates/core            Event / EventType / 序列化（无 IO）
  crates/storage         SQLite schema、批量写入、按天范围查询
  crates/collector       Windows 采集：窗口 hook、输入 idle
  src/                   Tauri 应用壳：命令、线程编排
design-system/           UI 基调与组件规范
```

色值真相只有一处：`src/styles/theme.css`。组件通过 `var(--color-cat-*)` 引用，
不抄 hex。改色板前后跑 `node design-system/time-scope/verify-palette.mjs`。

依赖方向：`core ← storage ← collector ← app`，`core` 不依赖任何 IO。

## 还没做什么

按优先级：

| 优先级 | 缺什么 | 影响 |
|---|---|---|
| 🟠 | **设置界面** | spec §9 的 `get_config` / `set_config` 未实现，改 `config.toml` 只能手改文件 |
| 🟡 | `segment-updated` 实时推送（spec §9） | 现用 5 秒轮询顶着，够用 |
| 🟡 | 规则热重载（spec §7.2） | 改完 `rules.toml` 要重启 |
| 🟡 | `config.toml` 热重载 | 改完要重启（`EngineConfig` 是构造注入的，热重载要重放当天，不值当） |

> **接手前先读 [`docs/superpowers/STATUS.md`](docs/superpowers/STATUS.md) 开头的
> 「⛔ 别动这里」一节** —— 那里列了十条看起来像 bug 但是故意的地方，
> 以及待重构清单。改 UI 另读
> [`design-system/time-scope/MASTER.md`](design-system/time-scope/MASTER.md)。

完整进度、遗留问题与未来计划：`docs/superpowers/STATUS.md`

本轮（Phase 1 第三步「打磨」）的人工验证清单：
`docs/superpowers/plans/2026-10-02-phase1-polish-verification.md`

## 启动慢（`pnpm tauri dev` 白屏）

**开发模式下**窗口会先白一段再出界面。实测数字（`node_modules/.vite` 缓存被清空 vs 已建立）：

| | 冷 | 热 |
|---|---|---|
| dev server 可访问 | 5.8s | 1.8s |
| **首个模块请求** | **81s** | **0.7s** |
| 其余 13 个模块 | 4.2s | ~2s |

全部是 **Vite 预构建依赖**，且只挂在第一个请求上。主要成本来自 `lucide-react`：
4242 个图标的 barrel，预构建后单个文件 1420 KB。

**这跟打包后的应用无关。** 生产产物是一个 262 KB 的 JS + 21 KB CSS，没有依赖爬取，
没有这段等待。

已经做的：`index.html` 里内联了一个首屏占位（`#root:empty::after`），
这段等待里窗口显示 "Time Scope" 而不是纯白。改不了耗时，但不让它看起来像坏了。

没做的，以及代价：

- **`optimizeDeps.include: ["lucide-react"]`** —— 试过，**无效**。实测第一个请求仍阻塞 38s。
  写进 `include` 并不能把爬取挪到启动阶段。
- **改成深导入**（`lucide-react/dist/esm/icons/activity.mjs`）—— 能让冷启动大幅缩短，
  但那是包内部路径，`package.json` 没有 `exports` 字段保证，lucide 升个版本就断。
  只为开发模式的速度不值得。
- **不预热 / 不清缓存** —— 平时什么都不用做，缓存热了就是 1 秒内。
  遇到长时间白屏，等它跑完即可，缓存建立后下次就不会再有。

## 人工验证清单

`docs/superpowers/plans/2026-10-01-phase1-engine-verification.md`
（第二步）与 `...scaffold-verification.md`（第一步）。

## 已知注意事项

- 采集时**不要**把 Time Scope 自己当作验证窗口：`WINEVENT_SKIPOWNPROCESS` 会过滤掉
  自己的窗口，验证 focus 采集要切到别的程序。
- 手工查库时请把 `time-scope.db`、`-wal`、`-shm` 三个文件一起复制出来再打开；
  应用运行中直接开原文件可能报 `SQLITE_CANTOPEN`。
- 强杀应用最多丢最后几秒 Event（内存队列，spec §8.1 明确接受）。
