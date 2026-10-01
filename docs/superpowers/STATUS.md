# Time Scope — 工作进展与遗留问题

**记录时间**：2026-10-01
**分支**：`feature/phase1-scaffold`（16 commits，**未合入 `master`**）
**对应计划**：[`2026-10-01-phase1-scaffold.md`](superpowers/plans/2026-10-01-phase1-scaffold.md)（11 task，全部完成）
**对应设计**：[`2026-10-01-time-scope-phase1-design.md`](../specs/2026-10-01-time-scope-phase1-design.md)

---

## 一、现在处于什么阶段

Phase 1 分为三步（设计文档 §14）。**第一步「骨架」已完成并全绿**；第二、三步尚未开始。

| 步骤 | 内容 | 状态 |
|---|---|---|
| 1. 骨架 | Tauri+React 脚手架、窗口/空闲采集 → SQLite → 前端按天查原始事件 | ✅ **完成** |
| 2. 引擎 | engine crate（segmenter 状态机 + TOML 规则分类）→ 分类色块 + 粒度切换 + 汇总 | ⬜ 未开始 |
| 3. 打磨 | 托盘/自启/单实例、标题脱敏、evidence 展示、空状态 | ⬜ 未开始 |

当前能做的：**采集真实使用活动、存进本地 SQLite、在 24h 时间线上看到色块、点开看进程和窗口标题**。
当前做不到的：**任何形式的"分类"**——所有窗口事件都是同一个蓝色系，没有 work/browsing/idle 的语义区分。

---

## 二、代码规模与状态

```
Rust  22 文件 / 1937 行      TS  10 文件 / 564 行
测试  59 Rust + 24 前端 = 83 条用例，全绿，0 warning
提交  16 个（master..HEAD），工作区干净
```

依赖方向单向、无环：`core ← storage ← collector ← app`。
`core` 零 IO；`storage` 不认识 collector；`collector` 不认识 tauri。

```
time-scope/
├── src/                          React 前端
│   ├── components/Timeline.tsx   24h 横向时间线（SVG，按事件类型着色）
│   ├── components/EventDetail.tsx 选中事件详情
│   └── types.ts                  StoredEvent 的 TS 镜像 + IPC + 日期工具
└── src-tauri/
    ├── src/lib.rs                Tauri 壳：commands、线程编排、退出刷盘
    ├── src/date_range.rs         本地时区按天半开区间（8 个单测）
    ├── src/exit_flush.rs         正常退出时 flush 队列（3 个单测）
    └── crates/
        ├── core/                 Event / EventType / 序列化，零 IO
        ├── storage/              版本化迁移、BatchWriter、按天范围查询
        └── collector/            window hook、input idle、consumer（事件总线）
```

---

## 三、已验证 / 未验证

### 自动化已覆盖（可随时重跑）

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace   # 59 passed
pnpm test                                                      # 24 passed
pnpm build                                                     # 无 tsc 报错
```

> ⚠️ `cargo test` **必须带 `--workspace`**。`src-tauri/Cargo.toml` 同时是 workspace 根和应用包，
> 不带的话只跑根包（0 个测试）并显示绿色——实现过程中真的被这个坑过一次。

### 真实 Windows 上已实测

- 应用能启动、在 `%APPDATA%\time-scope\time-scope.db` 建库、`journal_mode=wal`
- 采到真实事件：15 条（12 `window_title_change` + 3 `input_heartbeat`），
  含 `WindowsTerminal.exe` / 窗口标题 / 完整 exe 路径
- **重启不丢数据**（Review Focus #1）：杀进程后重启，2 条 → 9 条，旧数据全保留
- 正常退出时队列里的残留会写入（`RunEvent::Exit` → `flush_for_exit`）

### 仍需人工确认

`docs/superpowers/plans/2026-10-01-phase1-scaffold-verification.md` 里第 2–6、9 步未勾选：

- [ ] 界面真实交互（点色块看详情、日期切换、错误态、空态）
- [ ] 空闲检测（需停止键鼠 **5 分钟以上**才触发，阈值 300 秒）
- [ ] 常驻开销（CPU ≈ 0、内存 < 80 MB，spec §12 的预算）

---

## 四、遗留问题清单

### 4.1 代码问题（6 条 Minor，均未修）

| # | 问题 | 位置 | 影响 | 修法 |
|---|---|---|---|---|
| 1 | 标题过滤用"当前"前台窗口，不是"事件发生时"的前台窗口 | `collector/src/window.rs` `make_title_event` | 切换快于 consumer 排空时误删部分 `window_title_change`。**不影响 focus 事件，时间线仍正确反映用了哪些程序** | 把 `GetForegroundWindow()` 比较搬进 hook 回调（只多一次极轻的 user32 调用） |
| 2 | `sleep.rs` 残留空文件 | `collector/src/sleep.rs` | 死文件，Task 8 改用 `input.rs` 后再没被引用 | 删 |
| 3 | `flush_and_stop()` / `BatchWriter::written()` / `get_db_path` 无调用方 | `storage/src/writer.rs`、`src-tauri/src/lib.rs` | 死代码 | 删 |
| 4 | `migrate()` 没有版本化迁移路径 | `storage/src/schema.rs` | **首次改表结构前必须先改**，否则新列不会落到已有数据库上 | 改成按 `user_version` 分支的迁移器 |
| 5 | `csp: null` 禁用了 CSP | `tauri.conf.json` | 只加载本地资源、不发网络请求（spec §11），风险低；但一道 CSP 能把这条约束变成机器可验证的 | 配 `default-src 'self'` |
| 6 | `window_info` 栈上开 `[u16; 32768]`（64 KB） | `collector/src/window.rs` | `MAX_PATH` 只要 260；在 consumer 线程（2 MB 栈）所以不致命 | 缩小缓冲 |

> 这 6 条按 executing-plans 的规则归为 Minor、不进 fix pass，留给 human partner 决定。
> 真正值得优先处理的是 **#4**——它会在 Phase 2 第一次改表时咬人。

### 4.2 spec 声明本轮不覆盖（属后续计划，不是遗漏）

| spec 章节 | 内容 | 现状 |
|---|---|---|
| §5.3 | SessionLock / SessionUnlock、睡眠唤醒采集 | `EventType` 变体已预留，采集未实现 |
| §7 | engine crate：segmenter 状态机、TOML 规则分类器、grace period | 整个 crate 不存在 |
| §8 | `activities` 表的写入路径 | 表和索引已建，无写入者（写入者就是 engine） |
| §8.2 | 前端分桶（10/30/60/120 分钟粒度切换） | 未实现 |
| §9 | `get_segments` command、`segment-updated` 事件 | 未实现（只有 `get_events`） |
| §11 | 窗口标题脱敏（`[[redact]]` 规则） | 未实现，当前**原样存储标题** |
| §12 | 托盘常驻 / 开机自启 / 单实例 | 未实现，关闭窗口即退出 |

> ⚠️ **§11 脱敏是隐私相关的**：spec §1 承诺"窗口标题可脱敏"，但当前标题以明文落库。
> 骨架阶段可接受（本地存储、无网络请求），但**任何时候把数据导出/分享出去之前**，
> 必须先做脱敏。

### 4.3 计划文档本身的偏差（已修，记录备查）

执行过程中发现计划有多处与实际不符，均已在 ledger 记录并修掉：

- **windows-rs 0.58 的 API 与计划差 5 处**（`SetWinEventHook` 在 `UI::Accessibility`、
  `WINEVENTPROC` 是 7 参数、`EVENT_*` 是裸 u32 等）
- **Task 5 少一个代码块收尾 fence**，导致后续所有 task 的 brief 都提取不出来
- **Task 8 的 u32 tick 回绕 bug**（计划让 u32 的 `dwTime` 和 u64 的 `GetTickCount64` 相减）
- **Task 7 漏实现 spec §5.2 的"仅过滤前台窗口"**（`EVENT_OBJECT_NAMECHANGE` 会对所有对象派发）
- **Task 6 的 workspace 根配置无法编译**（virtual manifest 不能有 `[dependencies]`）
- **Task 5 的 `Arc<Connection>` 无法跨线程**（`Connection` 是 `Send` 但非 `Sync`）
- **Task 9 的 consumer 缺 `activity-storage` 依赖**；`time` 0.3.55 的 `well_known::Date` 不存在

### 4.4 我自己犯的错（值得记住）

- **Task 9 手写测试期望值连错三次**（epoch 差半年、减法方向反了、把一天的结束和另一天的开始相比），
  其中"本地午夜比 UTC 早还是晚 8h"是真的符号错误。测试抓住了它们，我改的是测试不是代码。
- **Task 6 差点把"0 个测试全绿"当成通过**。`cargo test` 在根包模式下静默跳过 core/storage 的 27 个测试。

---

## 五、实现过程中的关键决策（43 条 ruling 的浓缩）

完整清单在 `.superpowers/sdd/2026-10-01-phase1-scaffold/progress.md`（已 gitignore，需时可从 git 历史重建）。
最重要的几条：

| 决策 | 原因 | 代价 |
|---|---|---|
| collector → storage 单向依赖 | consumer（事件总线）在 collector 里要往 `BatchWriter` 推 | 无环，边界仍守住 |
| `SharedConn = Arc<Mutex<Connection>>` | `rusqlite::Connection` 是 `Send` 非 `Sync` | 每次刷盘/查询多一次锁 |
| `unsafe impl Send for RawSignal` | `HWND` 是裸指针，没有 `Send` | 传递不透明句柄，非共享可变状态 |
| `idle_seconds` 用 u32 + `wrapping_sub` | `dwTime` 是 u32，49.7 天回绕 | 用 u64 会导致"永远 idle" |
| `payload` 只存 `EventType` JSON | id/timestamp/type 各有独立列，存两遍有两个真相来源 | 若 engine 要单字段拿全量 Event 需迁移 |
| 标题过滤放 consumer 侧 | 保持 hook 回调"只塞 HWND" | 产生 4.1 #1 |
| 砍掉 android/ios 图标 | spec §1 明确 Phase 1 只做 Windows | 将来跨平台要重跑 `pnpm tauri icon` |
| 用 `time` crate 而非手写 civil-days | 本地时区 + DST 正确 | 多一个依赖 |

---

## 六、下一步的选择

### A. 先合入骨架（推荐）

```bash
# 1. 跑一遍人工验证清单的 2–6、9 步
#    docs/superpowers/plans/2026-10-01-phase1-scaffold-verification.md
# 2. 处理 4.1 里想处理的（建议至少 #4）
# 3. 合并
git checkout master && git merge --no-ff feature/phase1-scaffold
```

### B. 直接进 Phase 1 第二步「引擎」

需要先写一份新的实施计划（走 `superpowers:brainstorming` → `writing-plans`）。
核心内容按 spec §7：

1. 建 `activity-engine` crate（**纯库、零 IO、可独立 `cargo test`**）
2. `Context Builder`：从 Event 流维护 `{app, window_title, input_active, is_idle}`
3. `Rule Classifier`：TOML 外置规则，`%APPDATA%/time-scope/rules.toml`，首次启动从内置默认拷贝
4. `Segmenter` 状态机：同 context 延长 / 变化则切段（grace_period 60s）/ idle 切开 / resume 关闭
5. `reduce(state, event) -> EngineOutput` 保持无副作用，历史 Event 可重放重算
6. `activities` + `activity_evidence` 写入
7. IPC 从 `get_events` 换成 `get_segments`
8. 前端：分类配色 + 10/30/60/120 分桶 + 当日汇总

### C. 先补骨架的小缺口

从 4.2 里挑：标题脱敏（§11，隐私相关，建议优先）、托盘常驻（§12）、
SessionLock/Unlock 采集（§5.3）。

---

## 七、给下一个接手的建议

1. **动手前先读** `docs/superpowers/specs/2026-10-01-time-scope-phase1-design.md` 的
   §3（架构）、§5（采集层）、§7（引擎）、§8（Schema）、§9（IPC）。
2. **`cargo test` 永远带 `--workspace`**。
3. **想复现本次执行的完整决策过程**（含 43 条 ruling、每个 task 的测试日志），
   重新跑一次 `superpowers:executing-plans` 会重建 `.superpowers/sdd/` 目录。
4. **验证采集时务必切到 Time Scope 之外的窗口**——`WINEVENT_SKIPOWNPROCESS`
   会过滤自己的窗口，切到自己不产生 focus 事件，这是设计不是 bug。
5. **想手工查库**就把 `time-scope.db`、`-wal`、`-shm` 三个文件一起复制出来再打开，
   运行中原地打开会报 `SQLITE_CANTOPEN`。
