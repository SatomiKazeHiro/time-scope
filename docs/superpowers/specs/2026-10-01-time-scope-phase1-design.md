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
| 前端 | React 19 + Vite + Tailwind v4 | 时间块为自定义 SVG/div 渲染，不引入重型图表库（原稿写的是 visx，实际没用，见 §19） |

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
├── design-system/            # UI 规范（MASTER.md）+ 色板校验脚本
├── src/                      # React 前端（Vite + TS + Tailwind v4）
│   ├── styles/theme.css      # 设计 token，色值唯一真相
│   ├── design/               # categories.ts 类别元数据 / useTheme.ts 三态主题
│   ├── components/
│   │   ├── SegmentTimeline.tsx   # 24h 时间条（类别/专注度/切换次数三态）
│   │   ├── GranularityPicker.tsx
│   │   ├── MetricPicker.tsx      # 指标切换 + 顺序色阶图例
│   │   ├── DaySummary.tsx        # 分类时长 + 活跃/空闲比
│   │   ├── EventDetail.tsx       # 选中段的详情
│   │   └── ThemeToggle.tsx
│   ├── lib/
│   │   ├── bucket.ts         # 分桶切片 / 汇总纯函数
│   │   └── metrics.ts        # 每桶的专注度、切换次数
│   └── App.tsx               # IPC 封装内联在 types.ts，原计划的 ipc.ts 没建
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

> 上面的树按 2026-10-04 的实际结构更新（原稿的 `Timeline.tsx` / `TimelineBlock.tsx`
> / `ipc.ts` 都没建；IPC 封装最终放进了 `types.ts`）。

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
    pub id: String,           // 见下方补注
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

> **`Event.id` 的实际形式**（2026-10-01 实施后补注）：Phase 1 未引入 ulid 依赖，
> 用的是 `<16 位十六进制时间戳><16 位十六进制进程内计数器>`，定长 32 字符。
> 它满足 ULID 在本项目里唯一被依赖的性质（**字典序 == 时间序**），
> 且同毫秒内靠计数器区分。将来若要与外部系统交换 id，再换成真 ULID。

> **`ActivityContext` 的一致性判定**（同日补注）：判定"是否同一 context"只看
> `application` 与 `is_idle`，**不看 `window_title`**。浏览器每秒都在改标题，
> 按标题切段会把一天碎成几百段。这与 §7.3 状态机里"app + category 相同就延长"一致。

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

> **匹配语义**（2026-10-01 实施后补注）：
> - 进程名**大小写不敏感**（\`Code.exe\` 的规则能匹配 \`code.exe\`）。
>   用户手写规则不该因为大小写失配而以为规则坏了。
> - 只取**文件名部分**比较（\`C:\x\y\Code.exe\` 匹配 \`Code.exe\`）。
> - 按声明顺序，**第一条命中即返回**。
> - 规则里的 \`category\` 写错（如 \`not_a_category\`）**必须报错**并退回内置默认，
>   不能静默落到 unknown——否则用户以为规则生效了。
> - \`confidence\` 直接用规则里写的值。
> - 规则文件被改坏（语法错 / 类别写错）时应用**不能崩**，应退回内置默认、
>   在 stderr 说明、且**不覆盖用户的文件**（留着给他自己修）。

### 7.3 Segmenter 状态机

```
同一 context（app + category 相同）
    → 延长当前 segment，追加 evidence event id
context 变化
    → 若距上次切换 < GRACE_PERIOD 且新 context 与再前一个相同
      → 视为短暂切换：**丢弃刚开的那段，从缓冲里复活前一段并延长**
    → 否则关闭旧 segment（进缓冲），开新 segment
idle
    → 关闭当前 segment，生成 idle segment（idle 期间的心跳不计入活跃）
resume
    → 关闭 idle segment
```

> **关于"视为短暂切换"**（2026-10-01 实施后补注）：这里的处理必须是**复活前一段**，
> 而不是简单地"不切段"。字面按"不切段"实现会把当前段（那个一闪而过的 context）延长下去，
> `Code → chrome(2s) → Code` 会留下一个 **chrome** 段——可"短暂切换"的语义是
> **当作它没发生过**，那么活下来的必须是前一个 context。
> 实现在 `engine/src/segmenter.rs` 的 `absorb()`。

> **关于上下文的维护方式**（同日补注）：context 是从事件流里**维护**的，即
> **沿用上一份再应用当前事件**，而不是每个事件从空 context 重建。
> 重建的话，`InputHeartbeat`（不携带 application）会把 application 冲成 `None`，
> 导致"同一应用"判定失败、**每来一个心跳就切一段**。

**已关闭段的缓冲与落库门槛**（同日补注）：

segment 一旦落库就改不了，因此引擎内部维护 `pending` 缓冲，只有满足下面条件的段才交给上层落库：

```
可以落库 = 该段已结束
         且 (距今 ≥ max(min_segment_duration, grace_period))    ← 门槛取两者较大
         或 短段已完成合并（见下）
```

门槛取 `max(...)` 的原因：grace 吸收要能**复活**前一段，而复活要求前一段**还没落库**。
若落库门槛只有 `min_segment_duration(30s)`，那么 40 秒时回来的段早已落库、无法复活，
`grace_period(60s)` 有一半是空转。门槛必须覆盖整个宽限窗口。

**短段合并**（同日补注）：短于 `min_segment_duration` 的段在**落库前**并入相邻的长段：

- 优先并入**前驱**长段；没有前驱则并入**后继**长段
- 正在生长的当前段也可作为后继目标，但仅当该短段确实紧邻（它后面没有别的段还卡在缓冲里），
  否则会跨过中间那段时间被并错
- 两侧都找不到长邻居时**先留在缓冲里等下一轮**，而不是硬并——宁可不并，不可并错
- 等满三倍门槛仍无邻居，则原样放行，避免孤立短段永远卡在缓冲里

合并会取两段 `start_at`/`end_at` 的并集边界，并拼接 `evidence_event_ids`。

> **无应用的事件不开段**（同日补注）：`InputHeartbeat` 不携带 application。
> 当还没有任何已知应用时，**不应**为它开一个 `unknown / 无应用`的段——应用一启动
> 就会被心跳堆出一个噪声段。已有段之后的心跳照常延长该段。

可调参数（配置文件）：

| 参数 | 默认值 | 含义 |
|---|---|---|
| `min_segment_duration` | 30s | 短于此的 segment 并入相邻 segment（见上） |
| `grace_period` | 60s | 短暂上下文切换宽限（见上） |
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
    event_id TEXT NOT NULL,          -- 软引用，不 REFERENCES events(id)，见下方补注
    PRIMARY KEY (activity_id, event_id)
);
```

> **`event_id` 不加外键**（2026-10-01 实施后修正）：采集链路是两条并行路径——
> 一边把 Event 压进内存队列（5s 后才落库），一边让 engine 同步 ingest；
> engine 可能**立即**产出一个需要落库的段。于是**段会先于它引用的 Event 落库**，
> 外键必然违反。实现时的 4 个测试实测报 `FOREIGN KEY constraint failed`，
> 同样的顺序在真实运行中也会发生。
>
> 代价是失去"证据一定指向真实 event"的引用完整性。接受它：
> `activity_id` 的外键保留（证据行总是随它的段一起增删，那才是真正需要的完整性），
> 而 `event_id` 只用于 UI 展示与下钻，悬空引用只会让那一行的 id 查不到东西。
> 覆盖写时先 `DELETE` 该段的旧证据再插新的，避免重算后残留。

### 8.1 写入策略

- `PRAGMA journal_mode=WAL`、`synchronous=NORMAL`。
- Event 进内存队列，**每 5s 或满 100 条**单事务批量插入（Batch Writer）。
- 程序正常退出时 flush；崩溃允许丢失最后几秒 Event（个人统计可接受）。
  正常退出的 flush 必须**显式**做：后台线程每 100ms 才醒一次，进程退出时它不保证跑得到，
  所以要挂在 `RunEvent::Exit` 上同步刷一次。
- 数据库文件：`%APPDATA%/time-scope/time-scope.db`。
- 迁移按 `PRAGMA user_version` 顺序应用（有序的 `(目标版本, SQL)` 表），
  每次启动只执行版本大于当前值的条目。不可写成"每次启动重跑全部 CREATE TABLE"。

> **重放的范围**（2026-10-01 实施后补注）：启动时重放**当天**。
> 但首次查看**其它**日期时也必须按需重放一次——否则上次崩溃留下的孤儿 Event
> （在 `events` 表里、却从未被引擎处理过）永远不会变成 segment，
> 用户翻到那一天会看到空白，以为那天没记录到自己。
> 用一个"已重放日期"集合保证同一天只重放一次。

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

> **刷新方式**（2026-10-01 实施后补注）：\`segment-updated\` 推送尚未实现，
> Phase 1 先用前端每 5 秒轮询 \`get_segments\` 顶上。
> 不轮询的话，常驻应用的时间线会一直停在启动那一刻的快照，
> 看着像程序坏了——引擎明明在后台不断产出新段。

### Events（后端 → 前端推送）

| Event | Payload | 时机 |
|---|---|---|
| `segment-updated` | `{ id, category, application, start_at }` | 当前 segment 变化时（低频，无性能压力） |

前端收到 `segment-updated` 后局部更新，不整表重查。

## 10. 前端设计（React）

> 本节在 2026-10-03/04 的 UI 轮次后按实现重写。原稿假定用 `@visx/scale`，
> 实际没有引入 —— 理由与新增能力见 §19。

- **主视图**：当日 24h 时间线横条，segment 按 category 着色。手写 SVG rect +
  `viewBox`（不用 visx：布局只是"起点→x、时长→宽"两个线性映射，
  引入 scale 库换不来任何东西，却多一个依赖和一层抽象）。
- **时间线三态指标**：`类别` / `专注度` / `切换次数`。同一行轨道，不加第二行。
  指标模式画的是**桶**（粒度即桶宽）而不是段，用一条顺序色阶（紫）着色。
  - 专注度 = 桶内主导类别的时长占比
  - 切换次数 = 桶内相邻段之间**应用变了**的次数（不是段数减一：引擎会把同一次
    活动按标题变化切成多段，那些不是用户自己在跳）
  - 两者都由 `get_segments` 的结果算出，**不需要新后端接口**
- **粒度切换器**：10 / 30（默认）/ 60 / 120 分钟。类别模式下按它切分段落；
  指标模式下它就是桶宽。
- **主题**：深 / 浅 / 跟随系统三态，选择记在 `localStorage`。两套色板各自
  独立选步进，不是互为反色（深色底上"更显眼"= 更亮，浅色底上 = 更深）。
- **hover/点击**：悬停出浮层，点击出详情面板 —— 起止时间、时长、应用、类别、
  置信度、分类依据、证据条数、窗口标题（限高自滚）。
- **当日汇总**：各 category 时长条形 + **活跃/空闲比**。
- **日期切换**：上一日/下一日/回到今天。
- **设计 token**：`src/styles/theme.css` 是色值唯一真相，组件通过
  `var(--color-cat-*)` 引用。类别色经过色觉安全校验（时间线上任意两类都可能
  相邻，必须按 all-pairs 判）。规范与理由见
  [`design-system/time-scope/MASTER.md`](../../../design-system/time-scope/MASTER.md)。
- 技术：React 19 + Vite + TypeScript + Tailwind v4 + lucide-react；
  无重型状态管理库（组件 state 足够，config context 至今没派上用场）。

## 11. 隐私设计

- 不记录按键内容，只有 active/quiet 心跳计数。
- 窗口标题入库前过**脱敏规则**（`rules.toml` 中 `[[redact]]` 正则列表，命中替换为 `[redacted]`）。
- 全部数据本地存储，应用不发起任何网络请求。

> **实现约定**（2026-10-01 实施后补注）：
>
> - 脱敏发生在**入队之前**（consumer 里），敏感标题不会以明文在内存队列里存在哪怕一秒。
>   脱敏器由 app 层构造后注入 collector——collector 因此既不认识规则、也不依赖 regex crate，
>   §4 的边界不变。
> - **写正则必须用 TOML 单引号**（literal string）。双引号会处理转义，`"\d"` 是非法转义。
> - **默认不预置任何脱敏规则**：过度脱敏会把有用的数据也毁掉，宁可让用户按需添加。
> - 一条正则编译失败只跳过该条并告警，**不影响分类规则**。
> - 脱敏只作用于新采集的数据；已落库的历史标题**不会被回溯改写**（`events` 表保持原样，
>   用户可自行核对或清库）。但“按需重放”读历史数据时会再脱敏一次，所以界面上不会露出。
> - **进程名与 `exe_path` 不脱敏**（§11 只要求标题）。

## 12. 常驻与功耗预算

- 托盘：`TrayIconBuilder`；关窗拦截 `CloseRequested` → `prevent_close()` + `hide()`；`RunEvent::ExitRequested` → `prevent_exit()`（退出仅走托盘菜单）。
- `tauri-plugin-single-instance`、`tauri-plugin-autostart`。
- 预算：窗口采集事件驱动（0 轮询）、idle 轮询每 1s 一次结构体读取、无全局钩子 → 常驻 CPU ≈ 0，内存目标 <80MB。

### 12.1 实现约定（2026-10-02 第三步「打磨」补充）

| 事项 | 约定 |
|---|---|
| 托盘菜单 | 三项：**打开时间线 / 开机自启（勾选项） / 退出**。结构与菜单事件解释抽成纯数据（`app::tray::build_menu` / `on_menu_event`），可单测 |
| 双击托盘 | 等同「打开时间线」（显示 + 置前） |
| 关窗行为 | `config.toml` 的 `close_behavior`：`ask`（默认，仅初始状态）/ `minimize` / `quit`。`ask` 弹一次确认框（原生 `MessageBoxW`），答案覆写 config 就不再问；默认选项是「最小化」 |
| 托盘不可用 | **必须退回「关窗即退出」**。否则用户会卡在一个打不开也关不掉的界面（这是常驻功能失败时唯一不可接受的降级） |
| 开机自启 | `tauri-plugin-autostart`，**默认关**。同步方向不对称：config 与系统不一致时**让系统去匹配 config**（用户意图优先）；一致时把系统状态写回 config，使托盘勾选态反映真实系统状态 |
| 锁屏/合盖 | `WTS_SESSION_LOCK/UNLOCK` 与 `PBT_APMSUSPEND/RESUME` → `EventType::SessionLock/SessionUnlock`，engine 视作 idle 边界（不是 `IdleStart`：重放时要能区分「没动鼠标」与「锁屏了」） |
| 监听挂在哪 | **自建 message-only 窗口 + 自己的消息泵**（`collector::session::spawn_listener`）。原设想是挂主窗口 HWND，但 Tauri 2 的 `WindowEvent` 不透出原始窗口消息，拿不到 `WM_WTSSESSION_CHANGE` / `WM_POWERBROADCAST`；而这两类通知按 Win32 设计必须绑 HWND，所以托盘型应用自建隐藏窗口是标准做法。副作用：`setup()` 不需要窗口结构改动 |
| 重复通知 | 锁屏与合盖可能各发一次。engine 侧对「已处于 idle 段」幂等：只延长，不重开段 |
| 行为参数 | `config.toml`（见第三步 spec §5）。**不并入 `rules.toml`**：后者会派生 `classifier_version`，塞进行为参数会让「改一个阈值就换一版规则编号」失去意义 |
| 配置容错 | 文件缺失→写默认模板；语法错→全默认 + stderr，**不覆盖文件**；单字段非法→该字段默认，其余照常生效；数值越界→夹到下限（`idle_threshold_s` 下限 10，否则一天几万段） |

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
| u32 tick 回绕（实施中新增） | `LASTINPUTINFO.dwTime` 是 **u32** 的 \`GetTickCount\`，约 49.7 天回绕一次。必须与 **32 位** \`GetTickCount\` 相减并用 \`wrapping_sub\`；拿它与 64 位 \`GetTickCount64\` 相减，回绕后会算出天文数字，表现为“永远不 idle”。已在 §7.3 落实并有单测 |
| 段 id 冲突与写入顺序（实施中新增） | 段 id 形如 \`seg-{start_at}\`，落库走 \`INSERT OR REPLACE\`。当前实现下两个活着的段不会共享起点（grace 吸收是复活旧段、丢弃新段），重放又会先删当天，因此暂不冲突；若将来改变这一点，需改用包含计数器的 id |

## 16. 实施后修订记录（2026-10-01）

Phase 1 前两步（骨架、引擎）实施完成后，回头审了一遍本文与代码的每一处分歧。
**不是所有分歧都是实现跑偏**——逐条判定如下：

| spec 原文 | 判定 | 依据 | 已改动的位置 |
|---|---|---|---|
| §7.3 "视为短暂切换，**不切段**" | **spec 措辞不准** | "视为没发生"要求活下来的是**前一个** context；字面"不切段"会延长那个一闪而过的 context，`Code→chrome(2s)→Code` 留下 chrome 段，与语义自相矛盾 | §7.3 伪码 + 补注 |
| §7.3 `min=30s` / `grace=60s` | **spec 参数自相矛盾** | 复活前一段要求它还没落库；30s 门槛下 40s 回来的段已落库，60s 宽限有一半空转。落库门槛必须 ≥ 宽限窗口 | §7.3 缓冲与落库门槛 |
| §7.3 "短于此的 segment 并入相邻 segment" | **spec 正确，是实施漏了** | 方向没错，但"什么时候并、并给谁"未定义 | §7.3 短段合并 |
| §7.1 "从 Event 流**维护**上下文" | **spec 正确，是计划写错了** | "维护"就是沿用再更新；每事件重建会让心跳把 application 冲成 None | §7.3 补注（原文不改） |
| §8 `event_id REFERENCES events(id)` | **spec 错，已实测验证** | 段先于 Event 落库，外键必然违反 | §8 schema + 补注 |
| §6 `Event.id // ULID` | **spec 略宽松** | 实现是时间戳+计数器的 32 字符定长值，字典序仍等于时间序 | §6 补注 |
| §7.2 规则匹配语义 | **spec 未定义** | 大小写/路径/顺序/错误处理都没写，实施时不得不定 | §7.2 补注 |
| §9 `segment-updated` 推迟 | **spec 正确，但推迟范围要收窄** | 推迟的是**推送**，不是**刷新**；完全不刷新会让常驻应用看着是坏的 | §9 补注 |

未被改动的 spec 章节（已逐条核对与实现一致）：§3 架构、§4 crate 边界、§6 数据模型字段、
§7.4 可重放、§8 三张表与索引、§10 前端、§11 脱敏、§12 常驻、§13 测试策略。

## 17. 调研来源（2026-10-01 验证）

- windows-rs 官方绑定：https://github.com/microsoft/windows-rs
- SetWinEventHook 需消息循环：https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook
- ActivityWatch watchers（心跳近似法）：https://docs.activitywatch.net/en/latest/watchers.html
- rusqlite bundled：https://github.com/rusqlite/rusqlite
- Tauri 2 release line（2.10–2.12，2026）：https://v2.tauri.app/release/tauri/
- Tauri 托盘常驻模式：https://github.com/orgs/tauri-apps/discussions/11489
- React 图表库对比（visx 适合自定义渲染）：https://blog.logrocket.com/best-react-chart-libraries-2026/
- Tauri IPC 高频事件注意事项（低频 emit 无压力）：https://v2.tauri.app/ （event emit/listen 文档）

---

## 18. 实施后修订记录（2026-10-02，第三步「打磨」）

| spec 原文 | 判定 | 依据 | 处理 |
|---|---|---|---|
| §3.2 "监听注册在主窗口上，`setup()` 拿到 hwnd 后启动" | **不可行，实现换法** | Tauri 2 的 `WindowEvent` 没有 `ReceivedMessage`，不透出原始窗口消息；WTS/Power 通知必须绑 HWND。改为监听自建 message-only 窗口 + 自己的消息泵 | §12.1「监听挂在哪」 |
| §5.2 "任何字段缺失或类型不对 → 用默认值" | **spec 措辞不够强，实现收紧** | 整份 deserialize 时一个字段类型错会让整份文件失效，与"其余照常生效"矛盾。实现改为逐字段读 `toml::Table` | §12.1「配置容错」 |
| §4.2 `close_behavior` 三态 | spec 正确，**plan 的实现写错了** | plan 的 `close_interception_when(tray_ok)` 硬编码 `Ask`，`minimize`/`quit` 形同虚设 | 实现按 spec，plan 的写法未采纳 |
| §4.2 "退出仅走托盘菜单" | spec **过严**，实现放宽 | 托盘创建失败时若仍禁止其他退出路径，用户无法退出应用。降级为「关窗即退出」 | §12.1「托盘不可用」 |
| §5.2 数值下限 `idle_threshold_s ≥ 10` | spec 正确，实现补了**上界** | spec 只给下限；上界（一天）防止 u32 极值把时间线切成几十万段 | `config.rs` 的 `MAX_SECONDS` |

---

## 19. 实施后修订记录（2026-10-03/04，UI 设计系统轮次）

这一轮不在原三步计划内。规范主体移到了
[`design-system/time-scope/MASTER.md`](../../../design-system/time-scope/MASTER.md)，
本节只记录**与本 spec 的分歧和缺口**。

| spec 原文 | 判定 | 依据 | 处理 |
|---|---|---|---|
| §10 "visx scale 计算布局" + 技术栈列 `@visx/scale` `@visx/shape` | **spec 假设过重，实现没用** | 时间线布局只是"起点→x、时长→宽"两个线性映射；一天几百段的场景下手写 SVG 与用 scale 库等价 | 改为手写 SVG + `viewBox`。`visx` 未进 `package.json` |
| §10 "粒度切换器" | spec **没说清它干什么**，实现最初也没干 | 早期实现只改了一个桶计数，视图纹丝不动 —— 宽度已经表示时长、颜色已经表示类别、段间缝已经表示活动边界，再切一刀不增加任何信息 | 重定义为桶宽；指标模式下它决定格子大小 |
| §10 "evidence 摘要（'依据：VS Code 前台 × 23 次心跳活跃'）" | spec 描述的文案**没实现** | 实际是"N 条事件支撑" | 保持现状，spec 这句暂不成立 |
| §10 "各 category 时长条形 + 活跃/空闲比" | spec 正确，实现**长期缺了后半句** | 活跃/空闲比直到 2026-10-04 才补上 | 已实现。`unknown` 计入活跃（"没分类出是什么"≠"没在做事"） |
| §9 `get_config` / `set_config` | spec 列了，**至今未实现** | Rust 侧只有 `get_segments` 和 `get_segment_titles` | 已知缺口。改 `config.toml` 只能手改文件，见 §4.2 |
| §9 `segment-updated` 推送 | 未实现，5 秒轮询顶着 | 功能等价，差别只在数据量大了之后浪费 | 已知欠账，优先级最低 |
| （spec 未提） | **新增** | 类别色必须过 all-pairs 色觉闸：八色平铺在时间线上不成立（六色时最差一对 deutan ΔE 2.7，等同同色） | 色相只承载三个层级 + 明度阶；深浅两套各自选步进。理由见 MASTER §2 |
| （spec 未提） | **新增** | 真实数据是一天上百个 1–3 分钟的短段，干净数据下设计的界面在真实场景里不可读 | 轨道可见 + 最小段宽 2.5 单位 + 底部刻度尺。验证用 mock 必须按最坏情况造 |
| （spec 未提） | **新增** | 窗口标题多到会撑开整个页面 | 标题列表限高自滚 + App 根节点 `h-full` 让底部面板各自滚 |
