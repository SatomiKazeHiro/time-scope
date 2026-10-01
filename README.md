# Time Scope

Windows 本地个人时间账本。采集使用活动 → 聚合为可解释的 Activity Segment → 时间线展示。

设计文档：[`docs/superpowers/specs/2026-10-01-time-scope-phase1-design.md`](docs/superpowers/specs/2026-10-01-time-scope-phase1-design.md)
实施计划：[`docs/superpowers/plans/2026-10-01-phase1-scaffold.md`](docs/superpowers/plans/2026-10-01-phase1-scaffold.md)

## 当前状态

**Phase 1 骨架**——端到端垂直切片已打通：

```
Windows 事件/输入  ─▶  collector  ─▶  Event  ─▶  SQLite(WAL)  ─▶  IPC  ─▶  24h 时间线 UI
```

能切窗口、能记心跳、能按天查回原始事件并画出来。**尚缺**分类与汇总（engine crate）、
窗口标题脱敏、托盘常驻/自启/单实例——见文末"还没做什么"。

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

- **engine crate**：Activity 分类（TOML 规则）、segmenter 状态机、分桶、汇总
- **窗口标题脱敏**（spec §11 的 `[[redact]]` 规则）
- **托盘常驻 / 开机自启 / 单实例**（spec §12）
- 分类配色与证据链展示（spec §7、§10）

## 已知注意事项

- 采集时**不要**把 Time Scope 自己当作验证窗口：`WINEVENT_SKIPOWNPROCESS` 会过滤掉
  自己的窗口，验证 focus 采集要切到别的程序。
- 手工查库时请把 `time-scope.db`、`-wal`、`-shm` 三个文件一起复制出来再打开；
  应用运行中直接开原文件可能报 `SQLITE_CANTOPEN`。
- 强杀应用最多丢最后几秒 Event（内存队列，spec §8.1 明确接受）。
