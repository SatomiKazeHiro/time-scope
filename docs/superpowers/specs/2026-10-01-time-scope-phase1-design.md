# Time Scope — 本地个人时间账本（Phase 1）设计文档

**日期**：2026-10-01
**状态**：已获用户确认，待实施
**背景**：`.reference/ChatGPT-本地监控方案设计-20261001-0032.md`（概念设计来源，含 Phase 1–4 路线图）

## 1. 目标

构建一个 Windows 本地常驻应用，自动采集用户电脑使用活动，聚合为可解释的活动时间段（Activity Segment），以可调粒度的时间线展示，回答"我的时间去哪了"。

### Phase 1 范围内

- Windows 前台窗口采集（进程名、窗口标题、切换时间）
- 用户活跃/空闲检测（idle 检测 + 输入活动心跳）
- Event → Activity Segment 的规则化分类引擎（确定性规则，无 AI）
- SQLite 本地持久化（事件溯源：Event 不可变，Activity 可重算）
- React 时间线 UI：24h 时间条、类别着色、10/30/60/120 分钟粒度切换、当日分类汇总
- 托盘常驻、开机自启、单实例
- 隐私设计：不记录按键内容、窗口标题可脱敏、无网络请求

### 明确不做（后续 Phase）

- ❌ 截图、OCR、Vision AI（Phase 3）
- ❌ 浏览器扩展、URL 采集（Phase 2）
- ❌ 本地 LLM 语义分类（Phase 3）
- ❌ 复盘 Agent、日/周总结（Phase 4）
- ❌ macOS / Linux（架构预留接口，Phase 1 不实现）
- ❌ 全局键盘/鼠标低级钩子（杀软误报风险，见 §11 调研）

## 2. 已确认的关键决策

| 决策点 | 选择 | 理由 |
|---|---|---|
| 应用壳 | Tauri 2 + React | 常驻内存 <80MB，远低于 Electron |
| 后端语言 | 全 Rust（含 Activity Engine） | 单进程、性能最优；通过 TOML 规则外置 + classifier 版本号 + 引擎可重放保留演进空间 |
| 数据库 | rusqlite（bundled）+ WAL | 同步 API 适合写路径；无系统依赖；sqlx 的编译期校验在单机场景收益小 |
| 输入强度采集 | GetLastInputInfo 心跳近似 | 避开 WH_KEYBOARD_LL 全局钩子的杀软误报风险与高频回调开销（ActivityWatch 同款方案） |
| 窗口采集方式 | SetWinEventHook 事件驱动 | 零轮询，只在状态变化时产生 Event |
| 时间粒度 | 不落库，查询时切桶 | `floor(ts/interval)*interval`，改显示粒度不用重算历史 |
| 前端 | React 19 + Vite + visx | 时间块为自定义 SVG/div 渲染，不引入重型图表库 |

## 3. 总体架构

```
Windows OS
   │  SetWinEventHook（窗口切换 / 标题变化，事件驱动）
   │  GetLastInputInfo 轮询（idle + 活动心跳，1s）
   │  WTS 锁屏通知 / Power 睡眠唤醒通知
   ▼
┌────────────────────── Tauri 单进程 ──────────────────────┐
│ Rust 后端                                                 │
│  collector-windows ──▶ Event Bus ──▶ Activity Engine     │
│                           │  (channel)      │            │
│                           ▼                 ▼            │
│                     Batch Writer ──▶ SQLite (WAL)        │
│                                            │             │
│                          IPC: commands + events          │
└────────────────────────────┼─────────────────────────────┘
                             ▼
              React 前端（时间线 / 当日统计）
```

核心原则：

1. **Event 是不可变事实，Activity 是可重算的解释。** Collector 只回答"观察到了什么"，不产出业务结论。
2. **AI/语义判断不进 Phase 1。** 规则引擎先行，未来分类器升级后可重放历史 Event 重算 Activity。
3. **Agent/展示层永远不反向污染事实层。**
4. **宁愿 Unknown，不要伪准确。** 规则匹不上时 category = `unknown`，不强行分类。

## 4. 代码结构

```
time-scope/
├── package.json              # 前端（pnpm）
├── src/                      # React 前端（Vite + TS）
│   ├── components/
│   │   ├── Timeline.tsx      # 24h 时间条
│   │   ├── TimelineBlock.tsx # 单个 segment 色块
│   │   ├── GranularityPicker.tsx
│   │   └── DaySummary.tsx
│   ├── lib/
│   │   ├── bucket.ts         # 分桶纯函数
│   │   └── ipc.ts            # Tauri invoke/listen 封装
│   └── App.tsx
└── src-tauri/
    ├── Cargo.toml            # workspace root
    ├── tauri.conf.json
    ├── src/                  # app 壳：main.rs，托盘/自启/IPC 注册/线程编排
    └── crates/
        ├── core/             # 共享类型：Event、ActivitySegment、Category
        ├── collector/        # Windows 采集层（薄，只做 OS 交互）
        ├── engine/           # 纯库：Event → Context → Segment，无 IO 无 Tauri 依赖
        └── storage/          # rusqlite：schema 迁移、batch writer、查询
```

约束：

- `engine` 和 `core` 不依赖 `tauri`、`rusqlite` 以外的 IO（engine 完全无 IO），可独立 `cargo test`。
- `collector` 保持薄：所有判断逻辑都在 `engine`，collector 只翻译 OS 信号为 Event。

## 5. 采集层设计（collector crate）

### 5.1 线程模型

```
[hook 线程]  SetWinEventHook + 原生消息泵（GetMessage/DispatchMessage）
     │  回调只做：取 HWND → channel.send(RawSignal)
     ▼
[poll 线程]  每 1s：GetLastInputInfo → 活动心跳 / idle 判断
     │
     ▼
[consumer 线程]  收 RawSignal → 取窗口标题/进程名 → 组装 Event → Event Bus
```

**关键约束（调研确认）**：

- 调用 `SetWinEventHook` 的线程**必须有 Windows 消息循环**，否则注册成功但回调永不触发。用 `std::thread::spawn` 起专用线程跑原生消息泵，**不要放进 tokio task**。
- Hook 回调里**只做最轻操作**（HWND 塞进 channel），取标题/进程名在 consumer 线程做。
- 注册参数：`WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS`，无需 DLL 注入，不监听自己。

### 5.2 窗口事件

- `EVENT_SYSTEM_FOREGROUND` → 前台窗口切换。
- `EVENT_OBJECT_NAMECHANGE`（过滤：仅当前前台窗口）→ 同窗口标题变化（浏览器切 Tab）。
- 取进程名：`GetWindowThreadProcessId` → `OpenProcess` → `QueryFullProcessImageNameW`。
- 使用微软官方 `windows` crate（windows-rs）。

### 5.3 Idle 与活动心跳

- 每 1s 轮询 `GetLastInputInfo`（注意：必须正确设置 `LASTINPUTINFO.cbSize`，否则恒返回 0）。
- 空闲秒数 = `GetTickCount64 - lastInputTick`。
- 超过阈值（默认 **300s**，可配置）→ 产生 `SystemIdle` Event；检测到输入恢复 → `SystemResume`。
- 活动心跳：tick 每变化视为活跃；每 **10s** 聚合一次产生 `InputHeartbeat { active_seconds: 0..=10 }`。
- 补充事件驱动信号：`WTSRegisterSessionNotification`（锁屏/解锁 → `SessionLock`/`SessionUnlock`）、`PowerRegisterSuspendResumeNotification`（睡眠唤醒）。

### 5.4 状态变化才产 Event

轮询循环只做比较，状态没变不产生任何 Event、不写库。

## 6. 数据模型（core crate）

```rust
pub enum EventType {
    WindowFocus,         // payload: process_name, window_title(已脱敏), exe_path
    WindowTitleChange,   // payload: process_name, window_title(已脱敏)
    SystemIdle,
    SystemResume,
    SessionLock,
    SessionUnlock,
    InputHeartbeat,      // payload: active_seconds: u8 (0..=10)
}

pub struct Event {
    pub id: String,           // ULID
    pub timestamp: i64,       // Unix ms
    pub event_type: EventType,
}

pub struct ActivitySegment {
    pub id: String,
    pub start_at: i64,
    pub end_at: i64,
    pub category: Category,
    pub application: Option<String>,
    pub confidence: f32,      // 0.0 ~ 1.0
    pub classifier: String,   // 例 "rule"
    pub classifier_version: String,
    pub evidence_event_ids: Vec<String>,
}

pub enum Category {
    Work, Study, Entertainment, Communication,
    Browsing, Life, Idle, Unknown,
}
```

## 7. Activity Engine（engine crate，纯函数式）

核心签名：

```rust
pub fn reduce(state: &EngineState, event: &Event) -> EngineOutput;

pub struct EngineOutput {
    pub state: EngineState,
    pub closed_segments: Vec<ActivitySegment>,  // 已关闭、可落库
}

pub struct EngineState {
    pub current_segment: Option<OpenSegment>,
    pub current_context: Option<ActivityContext>,
    pub is_idle: bool,
}
```

### 7.1 Context Builder

从 Event 流维护当前上下文：`{ app, window_title, input_active, is_idle }`。Context 是临时状态，不落库。

### 7.2 Rule Classifier（TOML 外置规则）

规则文件 `rules.toml`（运行时使用 `%APPDATA%/time-scope/rules.toml`；首次启动时从应用内置的默认规则拷贝一份，之后用户可改，支持热更新）：

```toml
[[rule]]
id = "vscode-coding"
process = ["Code.exe"]
category = "work"
confidence = 0.9

[[rule]]
id = "browser"
process = ["msedge.exe", "chrome.exe"]
category = "browsing"        # 无 URL 时只能粗分（Phase 2 才有 domain 细分）
confidence = 0.5

[[rule]]
id = "wechat"
process = ["WeChat.exe"]
category = "communication"
confidence = 0.85
```

匹配失败 → `category = unknown, confidence = 0.0`。每次分类记录 `classifier = "rule"` 和规则集版本号。

### 7.3 Segmenter 状态机

```
同一 context（app + category 相同）
    → 延长当前 segment，追加 evidence event id
context 变化
    → 若距上次切换 < GRACE_PERIOD 且新 context 与再前一个相同 → 视为短暂切换，不切段
    → 否则关闭旧 segment，开新 segment
idle
    → 关闭当前 segment，生成 idle segment（idle 期间的心跳不计入活跃）
resume
    → 关闭 idle segment
```

可调参数（配置文件）：

| 参数 | 默认值 | 含义 |
|---|---|---|
| `min_segment_duration` | 30s | 短于此的 segment 并入相邻 segment |
| `grace_period` | 60s | 短暂上下文切换宽限 |
| `idle_threshold` | 300s | 判定空闲的无输入时长 |

### 7.4 可重放

`reduce` 无副作用。历史 Event 从 SQLite 按时间读出后重新喂给新版 engine 即可重算全部 Activity。`activities.classifier` + `version` 字段用于区分重算前后结果。

## 8. SQLite Schema（storage crate）

Phase 1 仅 3 张表：

```sql
CREATE TABLE events (
    id TEXT PRIMARY KEY,
    timestamp INTEGER NOT NULL,
    type TEXT NOT NULL,
    payload TEXT NOT NULL,          -- JSON
    created_at INTEGER NOT NULL
);
CREATE INDEX idx_events_timestamp ON events(timestamp);
CREATE INDEX idx_events_type_timestamp ON events(type, timestamp);

CREATE TABLE activities (
    id TEXT PRIMARY KEY,
    start_at INTEGER NOT NULL,
    end_at INTEGER NOT NULL,
    category TEXT NOT NULL,
    application TEXT,
    confidence REAL NOT NULL,
    classifier TEXT NOT NULL,
    version TEXT NOT NULL
);
CREATE INDEX idx_activities_range ON activities(start_at, end_at);

CREATE TABLE activity_evidence (
    activity_id TEXT NOT NULL REFERENCES activities(id),
    event_id TEXT NOT NULL REFERENCES events(id),
    PRIMARY KEY (activity_id, event_id)
);
```

### 8.1 写入策略

- `PRAGMA journal_mode=WAL`、`synchronous=NORMAL`。
- Event 进内存队列，**每 5s 或满 100 条**单事务批量插入（Batch Writer）。
- 程序正常退出时 flush；崩溃允许丢失最后几秒 Event（个人统计可接受）。
- 数据库文件：`%APPDATA%/time-scope/time-scope.db`。

### 8.2 分桶（前端查询时，不落库）

分桶是纯函数，放在前端 `src/lib/bucket.ts`（vitest 覆盖）。后端只返回原始 segments，分桶/聚合都在前端完成：

```ts
function bucketStart(ts: number, intervalMs: number): number {
  return Math.floor(ts / intervalMs) * intervalMs
}
```

一个跨桶的 segment 按桶边界切成多段，按落入时长分配。粒度切换（10/30/60/120min）只改前端参数，不重新请求。

## 9. IPC 契约（Rust ↔ React）

### Commands（一问一答）

| Command | 参数 | 返回 |
|---|---|---|
| `get_segments` | `date: string (YYYY-MM-DD)` | `ActivitySegment[]`（当日全部原始段，前端自行分桶/聚合/hover 详情） |
| `get_config` / `set_config` | — | idle 阈值、脱敏规则开关等 |

> 当日汇总也由前端从 `get_segments` 结果聚合，后端不提供单独的 summary 接口。

### Events（后端 → 前端推送）

| Event | Payload | 时机 |
|---|---|---|
| `segment-updated` | `{ id, category, application, start_at }` | 当前 segment 变化时（低频，无性能压力） |

前端收到 `segment-updated` 后局部更新，不整表重查。

## 10. 前端设计（React）

- **主视图**：当日 24h 时间线横条，segment 按 category 着色（visx scale 计算布局，手写 SVG rect 渲染；一天几百段，无需重型图表库）。
- **粒度切换器**：10 / 30（默认）/ 60 / 120 分钟。
- **hover/点击 segment**：显示起止时间、应用、类别、置信度、evidence 摘要（"依据：VS Code 前台 × 23 次心跳活跃"）。
- **当日汇总**：各 category 时长条形 + 活跃/空闲比。
- **日期切换**：上一日/下一日/回到今天。
- 技术：React 19 + Vite + TypeScript + `@visx/scale` + `@visx/shape`；无重型状态管理库（组件 state + 一个 config context 足够）。

## 11. 隐私设计

- 不记录按键内容，只有 active/quiet 心跳计数。
- 窗口标题入库前过**脱敏规则**（`rules.toml` 中 `[[redact]]` 正则列表，命中替换为 `[redacted]`）。
- 全部数据本地存储，应用不发起任何网络请求。

## 12. 常驻与功耗预算

- 托盘：`TrayIconBuilder`；关窗拦截 `CloseRequested` → `prevent_close()` + `hide()`；`RunEvent::ExitRequested` → `prevent_exit()`（退出仅走托盘菜单）。
- `tauri-plugin-single-instance`、`tauri-plugin-autostart`。
- 预算：窗口采集事件驱动（0 轮询）、idle 轮询每 1s 一次结构体读取、无全局钩子 → 常驻 CPU ≈ 0，内存目标 <80MB。

## 13. 测试策略

| 层 | 方式 | 覆盖重点 |
|---|---|---|
| `engine` | 表驱动单测（事件序列 → 期望 segments） | 合并/切段/grace period/idle 切开/unknown |
| `storage` | 内存 SQLite 单测 | schema 迁移、批量写、范围查询 |
| `core` | 类型/序列化单测 | Event JSON 往返 |
| `collector` | 手动验证为主 | 逻辑保持薄，不追求自动化 |
| 前端 | vitest | 分桶/聚合纯函数 + 时间线渲染快照 |

## 14. 实施阶段（Phase 1 内部三步）

1. **骨架**：Tauri + React 脚手架；collector 采集窗口事件 → SQLite → 前端按天查原始事件列表。
2. **引擎**：engine（segmenter + TOML 规则）→ 时间线分类色块 + 粒度切换 + 汇总。
3. **打磨**：托盘/自启/单实例、脱敏规则、evidence 展示、空状态/错误处理。

## 15. 风险与备注

| 风险 | 缓解 |
|---|---|
| Phase 3+ 接 LLM 时 Rust 生态不如 TS 方便 | 已通过规则外置 + 引擎可重放 + classifier 版本号保留演进空间；届时 Rust 侧走 HTTP 调本地模型（如 Ollama） |
| 全局钩子杀软误报 | Phase 1 不使用低级钩子，规避此风险 |
| `SetWinEventHook` 无消息泵不触发 | 专用线程跑原生消息泵，已在 §5.1 明确 |
| `GetLastInputInfo` 的 `cbSize` 坑 | 已列入 §5.3 实现注意事项 |
| 无 URL 时浏览器只能粗分 `browsing` | 架构预留：Phase 2 浏览器扩展接入后规则升级即可，schema 不变 |

## 16. 调研来源（2026-10-01 验证）

- windows-rs 官方绑定：https://github.com/microsoft/windows-rs
- SetWinEventHook 需消息循环：https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook
- ActivityWatch watchers（心跳近似法）：https://docs.activitywatch.net/en/latest/watchers.html
- rusqlite bundled：https://github.com/rusqlite/rusqlite
- Tauri 2 release line（2.10–2.12，2026）：https://v2.tauri.app/release/tauri/
- Tauri 托盘常驻模式：https://github.com/orgs/tauri-apps/discussions/11489
- React 图表库对比（visx 适合自定义渲染）：https://blog.logrocket.com/best-react-chart-libraries-2026/
- Tauri IPC 高频事件注意事项（低频 emit 无压力）：https://v2.tauri.app/ （event emit/listen 文档）
