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

**尚缺**：托盘常驻/自启/单实例、锁屏与睡眠唤醒采集——见文末"还没做什么"。

## 环境要求

- Windows 10/11
- Node.js ≥ 20（开发用 22）
- pnpm ≥ 10（开发用 12）
- Rust stable，target `x86_64-pc-windows-msvc`
- WebView2 Runtime（Tauri 依赖，Win11 自带；Win10 需从微软官网装）

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
  components/Timeline    24h 横向时间线（SVG）
  components/EventDetail 选中事件的详情
  types.ts               StoredEvent 的 TS 镜像 + IPC 封装 + 日期工具
src-tauri/               Rust workspace
  crates/core            Event / EventType / 序列化（无 IO）
  crates/storage         SQLite schema、批量写入、按天范围查询
  crates/collector       Windows 采集：窗口 hook、输入 idle
  src/                   Tauri 应用壳：命令、线程编排
```

依赖方向：`core ← storage ← collector ← app`，`core` 不依赖任何 IO。

## 还没做什么

按优先级：

| 优先级 | 缺什么 | 影响 |
|---|---|---|
| 🟠 | **托盘常驻 / 开机自启 / 单实例**（spec §12） | 关窗口即退出，一天要重开十几次；无单实例还可能两个进程同时写同一个库 |
| 🟠 | **锁屏与睡眠唤醒采集**（spec §5.3） | 锁屏期间会继续记活跃，合盖后产生假 idle |
| 🟡 | `segment-updated` 实时推送（spec §9） | 现用 5 秒轮询顶着，够用 |
| 🟡 | 规则热重载（spec §7.2） | 改完 `rules.toml` 要重启 |

完整进度、遗留问题与未来计划：`docs/superpowers/STATUS.md`

## 人工验证清单

`docs/superpowers/plans/2026-10-01-phase1-engine-verification.md`
（第二步）与 `...scaffold-verification.md`（第一步）。

## 已知注意事项

- 采集时**不要**把 Time Scope 自己当作验证窗口：`WINEVENT_SKIPOWNPROCESS` 会过滤掉
  自己的窗口，验证 focus 采集要切到别的程序。
- 手工查库时请把 `time-scope.db`、`-wal`、`-shm` 三个文件一起复制出来再打开；
  应用运行中直接开原文件可能报 `SQLITE_CANTOPEN`。
- 强杀应用最多丢最后几秒 Event（内存队列，spec §8.1 明确接受）。
