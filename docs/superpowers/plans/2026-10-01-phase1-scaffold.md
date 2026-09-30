# Time Scope Phase 1 — 骨架（数据管线垂直切片）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 搭起 Tauri 2 + React 19 + Rust workspace 骨架，打通一条端到端垂直切片：Windows 窗口/空闲采集 → Event → SQLite 落库 → Rust 侧按天查原始事件 → IPC → React 时间线占位渲染。

**Architecture:** Tauri 单进程。Rust workspace 分 4 个 crate：`core`（共享类型）、`storage`（rusqlite 持久化）、`collector`（Windows 采集，薄）、`engine` 占位（本计划不实现分类，只放一个 `noop` 以便后续 task 引入）。collector 产生不可变 Event 落库；本计划 UI 只渲染原始 Event 的时间条，作为骨架验收物。

**Tech Stack:** Tauri 2.x、React 19 + Vite 6 + TypeScript、pnpm、Rust 1.96 (stable, x86_64-pc-windows-msvc)、rusqlite (bundled) + WAL、serde/serde_json、windows-rs 0.6x、vitest。

**Spec:** `docs/superpowers/specs/2026-10-01-time-scope-phase1-design.md` —— 实施者必须先读该 spec（尤其 §3 架构、§4 代码结构、§5 采集层、§6 数据模型、§8 Schema、§9 IPC、§13 测试策略），本 plan 与之冲突时以 spec 为准。

---

## Global Constraints

（下列为 spec 逐字提取的全局约束，每个 task 的要求都隐含包含本节）

- **平台**：仅 Windows。前台窗口采集、idle 检测、常驻托盘均针对 Win32。
- **包管理器**：前端用 **pnpm**（`pnpm install` / `pnpm tauri dev` / `pnpm tauri build`）。禁止 npm/yarn 生成 lockfile。
- **Rust 版**：stable 工具链 `x86_64-pc-windows-msvc`（本机已验证 1.96.1 可编译链接）。
- **数据库**：`rusqlite`（`bundled` feature）+ `PRAGMA journal_mode=WAL`、`synchronous=NORMAL`。**禁止**换成 sqlx/diesel 或外部 Postgres。
- **Event 是不可变事实**：collector 只产出 Event，**不产出任何业务结论/分类**。Activity 分类属 engine，是后续 task。
- **engine / core 不得依赖 tauri**；`core` 不得依赖 rusqlite（本计划 engine 仅占位，但保持此边界以便后续加纯库测试）。
- **idempotency / 隐私**：本计划不记录按键内容；窗口标题暂不做脱敏（脱敏规则属 spec §11，留到后续 task），但 Event payload 字段结构已预留脱敏位置。
- **写库策略**：Event 进内存队列，**每 5s 或满 100 条**单事务批量插入；正常退出 flush。
- **DB 路径**：`%APPDATA%/time-scope/time-scope.db`。
- **IPC 命令**命名与返回结构必须与 spec §9 一致（本计划实现 `get_events` 用于骨架验收；spec 的 `get_segments` 属 engine task）。
- **TDD**：每个 crate 的逻辑先写表驱动单测再实现；`cargo test` 独立于 tauri 可跑。
- **注释/文档语言**：代码注释、commit message 用英文；本 plan 与面向用户的 UI 文案用中文。
- **命名**：crate 用 kebab-case（`activity-core` 等，见 File Structure）；类型用 PascalCase；函数/变量用 snake_case。

## Review Focus

（spec 隐含、但没有哪个 task 的测试显式覆盖、最可能坑到真实用户的五类输入/情况，按可能性排序）

1. **程序被强杀 / 崩溃**：内存队列里最后几秒的 Event 丢失。spec §8.1 明确"崩溃允许丢失最后几秒 Event（个人统计可接受）"——可接受，但必须在 Task 5 验证 DB 落盘后关闭窗口、relaunch 后当天 Event 不为空（不为空即可，不保证最后几秒）。
2. **应用 24h 常驻但当天跨零点**：查询"某一天"必须按**本地时区** `[00:00, 次日00:00)` 半开区间过滤，跨零点后"今天"应切到新日期且旧日期数据仍可查。落到 Task 7（storage 范围查询测试）。
3. **Windows 缩放 / 高 DPI**：骨架时间条用固定 px 即可，spec 未要求响应式；本计划不测，但 Task 9 用 min-width 容器避免 0 宽渲染崩溃。
4. **窗口标题为 null / 空**（部分系统窗口、UWP 应用）：Event payload 的 `window_title` 可选，UI 不能因 `null` 崩溃。落到 Task 6（core serde 往返）+ Task 9（渲染 null 安全）。
5. **同一进程多窗口 / 进程名大小写差异**（`Code.exe` vs `code.exe`）：本计划不做分类，进程名仅原样存 Event、UI 原样显示；但 core 存的是进程名原值，Task 7 查询时按 process 原样返回，不做大写归一（避免埋下与 engine 规则大小写语义冲突的隐式行为）。

---

## File Structure

```
time-scope/
├── package.json                  # pnpm scripts（dev/build/tauri/test）
├── pnpm-lock.yaml                # pnpm 生成，入库
├── tsconfig.json                 # 前端 TS 配置
├── vite.config.ts                # Vite + @vitejs/plugin-react + vitest 配置
├── index.html                    # Vite 入口
├── .gitignore                    # 已有（忽略 node_modules/dist/target/vendor/.pi）
├── src/                          # React 前端
│   ├── main.tsx                  # 挂载 React
│   ├── App.tsx                   # 顶层：日期选择 + 时间线 + 状态
│   ├── types.ts                  # 镜像 Rust Event 的 TS 类型 + tauri invoke 封装
│   ├── components/
│   │   ├── Timeline.tsx          # 24h 事件条（SVG 横向时间轴）
│   │   └── EventDetail.tsx       # 选中事件详情（进程/标题/时间）
│   └── test/setup.ts             # vitest 环境（jsdom）
└── src-tauri/
    ├── Cargo.toml                # workspace root + tauri 依赖
    ├── tauri.conf.json           # Tauri 2 配置（窗口、bundle）
    ├── build.rs                  # tauri-build
    ├── capabilities/default.json # Tauri 2 权限（core:default）
    ├── icons/                    # Tauri 必需图标（用 tauri icon 生成或占位）
    └── crates/
        ├── core/                 # activity-core：Event/EventType 类型 + serde
        │   ├── Cargo.toml
        │   └── src/
        │       ├── lib.rs
        │       └── tests.rs      # serde 往返单测
        ├── storage/              # activity-storage：schema/批写/范围查询
        │   ├── Cargo.toml
        │   └── src/
        │       ├── lib.rs        # Storage 门面 + migrate
        │       ├── schema.rs     # 建表/索引 + 迁移
        │       ├── writer.rs     # BatchWriter（5s/100 条批量插入）
        │       └── query.rs      # get_events_in_range
        │   └── tests/
        │       ├── migration.rs  # 内存 SQLite 迁移/建表
        │       └── query.rs      # 范围查询半开区间
        └── collector/            # activity-collector：Windows 采集
            ├── Cargo.toml
            └── src/
                ├── lib.rs        # 线程编排：hook/poll/consumer
                ├── window.rs     # SetWinEventHook + 取进程名/标题
                ├── input.rs      # GetLastInputInfo idle/心跳
                └── signals.rs    # RawSignal channel 类型
```

（engine crate 本计划**不建**——骨架不涉及分类，避免空 crate。后续 engine task 再引入。）

---

## Task 1: 前端脚手架（pnpm + Vite + React + vitest 可跑）

**Files:**
- Create: `package.json`
- Create: `pnpm-lock.yaml`（由 pnpm 生成）
- Create: `tsconfig.json`
- Create: `vite.config.ts`
- Create: `index.html`
- Create: `src/main.tsx`
- Create: `src/test/setup.ts`
- Create: `src/App.test.tsx`
- Test: `src/App.test.tsx`

**Interfaces:**
- Consumes: 无（起点）
- Produces: 可运行的 `pnpm dev` / `pnpm test` / `pnpm build`；`src/main.tsx` 导出挂载逻辑；`package.json` 提供 `tauri` script（后续 task 依赖）。

- [ ] **Step 1: 创建 `package.json`**

```json
{
  "name": "time-scope",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc --noEmit && vite build",
    "preview": "vite preview",
    "test": "vitest run",
    "test:watch": "vitest",
    "tauri": "tauri"
  },
  "dependencies": {
    "@tauri-apps/api": "^2.1.1",
    "react": "^19.0.0",
    "react-dom": "^19.0.0"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.1.0",
    "@testing-library/react": "^16.1.0",
    "@types/react": "^19.0.0",
    "@types/react-dom": "^19.0.0",
    "@vitejs/plugin-react": "^4.3.4",
    "jsdom": "^25.0.1",
    "typescript": "^5.7.2",
    "vite": "^6.0.5",
    "vitest": "^2.1.8"
  }
}
```

- [ ] **Step 2: 创建 `tsconfig.json`**

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true,
    "skipLibCheck": true,
    "esModuleInterop": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "types": ["vitest/globals"]
  },
  "include": ["src"]
}
```

- [ ] **Step 3: 创建 `vite.config.ts`**

```ts
/// <reference types="vitest" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  test: {
    globals: true,
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
  },
});
```

- [ ] **Step 4: 创建 `index.html`**

```html
<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Time Scope</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

- [ ] **Step 5: 创建 `src/test/setup.ts`**

```ts
// 目前无需全局 polyfill；保留文件以满足 vite.config 的 setupFiles 引用，后续 task 按需扩展。
export {};
```

- [ ] **Step 6: 写失败的冒烟测试 `src/App.test.tsx`**

```tsx
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import App from "./App";

describe("App smoke", () => {
  it("renders the app title", () => {
    render(<App />);
    expect(screen.getByText(/Time Scope/i)).toBeTruthy();
  });
});
```

该测试依赖 `@testing-library/react`，它已在 Step 1 的 `devDependencies` 中声明（`^16.1.0`），Step 7 安装。断言用 `getByText` 而非 `toBeInTheDocument()`，因此**不需要** `@testing-library/jest-dom`（无需在 `setup.ts` 里 `import "@testing-library/jest-dom"`）。

- [ ] **Step 7: 安装依赖**

Run: `pnpm install`
Expected: 创建 `pnpm-lock.yaml`，无 error。

- [ ] **Step 8: 创建 `src/App.tsx`（最小可渲染）**

```tsx
export default function App() {
  return (
    <main>
      <h1>Time Scope</h1>
      <p>骨架阶段：数据管线垂直切片。</p>
    </main>
  );
}
```

- [ ] **Step 9: 创建 `src/main.tsx`**

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

const root = document.getElementById("root");
if (!root) throw new Error("root element not found");

ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
```

- [ ] **Step 10: 运行测试确认通过**

Run: `pnpm test`
Expected: PASS（1 test）。

- [ ] **Step 11: 验证 build 通过**

Run: `pnpm build`
Expected: `tsc --noEmit` 无错，`vite build` 输出 `dist/`。

- [ ] **Step 12: 提交**

```bash
git add package.json pnpm-lock.yaml tsconfig.json vite.config.ts index.html src/
git commit -m "chore: scaffold frontend with Vite + React 19 + vitest"
```

---

## Task 2: Rust workspace + core crate（Event 类型 + serde）

**Files:**
- Create: `src-tauri/Cargo.toml`
- Create: `src-tauri/crates/core/Cargo.toml`
- Create: `src-tauri/crates/core/src/lib.rs`
- Create: `src-tauri/crates/core/src/tests.rs`
- Test: `src-tauri/crates/core/src/tests.rs`（`#[cfg(test)]` 模块，由 `cargo test -p activity-core` 运行）

**Interfaces:**
- Consumes: 无
- Produces:
  - `activity_core::Event { id: String, timestamp: i64, event_type: EventType }`
  - `activity_core::EventType` 枚举，serde 序列化为**小写 tag + snake_case payload 字段**（见下）
  - `activity_core::{WindowFocusPayload, WindowTitleChangePayload, InputHeartbeatPayload}`
  - `Event::new(event_type, timestamp) -> Event`（生成 ULID 式 id：时间有序）

**ULID 生成**：不引入 ulid crate（本计划 YAGNI），用 `format!("{:016x}{:016x}", timestamp_u64, counter)` 保证单调、可排序、进程内唯一（配合原子计数器）。后续 task 若需真 ULID 再替换。

- [ ] **Step 1: 创建 workspace `src-tauri/Cargo.toml`**

```toml
[workspace]
# 分步补齐：Task 2 先只含 core，Task 3 补 storage，Task 6 补 collector。
# cargo 会在 members 指向不存在目录时直接报错，这是刻意的防呆。
members = ["crates/core", "."]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"

[workspace.dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

[dependencies]
tauri = { version = "2.1", features = [] }
serde.workspace = true
serde_json.workspace = true
activity-core = { path = "crates/core" }
time = { version = "0.3", features = ["local-offset", "parsing", "formatting", "macros"] }

[build-dependencies]
tauri-build = { version = "2.0", features = [] }
```

- [ ] **Step 2: 创建 `src-tauri/crates/core/Cargo.toml`**

```toml
[package]
name = "activity-core"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
```

- [ ] **Step 3: 写失败测试 `src-tauri/crates/core/src/tests.rs`**

```rust
use crate::{Event, EventType, InputHeartbeatPayload, WindowFocusPayload, WindowTitleChangePayload};

#[test]
fn window_focus_event_roundtrips_json() {
    let e = Event::new(
        EventType::WindowFocus(WindowFocusPayload {
            process_name: "Code.exe".into(),
            window_title: Some("main.rs - time-scope".into()),
            exe_path: Some("C:\dev\Code.exe".into()),
        }),
        1_700_000_000_000,
    );
    let s = serde_json::to_string(&e).unwrap();
    let back: Event = serde_json::from_str(&s).unwrap();
    assert_eq!(back.id, e.id);
    assert_eq!(back.timestamp, 1_700_000_000_000);
    match back.event_type {
        EventType::WindowFocus(ref p) => {
            assert_eq!(p.process_name, "Code.exe");
            assert_eq!(p.window_title.as_deref(), Some("main.rs - time-scope"));
            assert_eq!(p.exe_path.as_deref(), Some("C:\dev\Code.exe"));
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn event_type_serializes_with_snake_case_tag() {
    let e = Event::new(
        EventType::SystemIdle,
        1_700_000_002_000,
    );
    let s = serde_json::to_string(&e).unwrap();
    assert!(s.contains("\"type\":\"system_idle\""), "unexpected json: {s}");
    assert_eq!(e.event_type.type_tag(), "system_idle");
}

#[test]
fn window_title_change_with_null_title_roundtrips() {
    // Review Focus #4: 部分系统窗口/UWP 应用没有标题，null 不得导致反序列化失败
    let e = Event::new(
        EventType::WindowTitleChange(WindowTitleChangePayload {
            process_name: "explorer.exe".into(),
            window_title: None,
        }),
        1_700_000_001_000,
    );
    let s = serde_json::to_string(&e).unwrap();
    let back: Event = serde_json::from_str(&s).unwrap();
    match back.event_type {
        EventType::WindowTitleChange(ref p) => {
            assert!(p.window_title.is_none());
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn unit_events_roundtrip() {
    for et in [
        EventType::SystemIdle,
        EventType::SystemResume,
        EventType::SessionLock,
        EventType::SessionUnlock,
    ] {
        let tag = et.type_tag();
        let e = Event::new(et, 1_700_000_002_000);
        let s = serde_json::to_string(&e).unwrap();
        let back: Event = serde_json::from_str(&s).unwrap();
        assert_eq!(back.timestamp, 1_700_000_002_000);
        assert_eq!(back.event_type.type_tag(), tag);
    }
}

#[test]
fn input_heartbeat_roundtrips() {
    let e = Event::new(
        EventType::InputHeartbeat(InputHeartbeatPayload { active_seconds: 7 }),
        1_700_000_003_000,
    );
    let s = serde_json::to_string(&e).unwrap();
    let back: Event = serde_json::from_str(&s).unwrap();
    match back.event_type {
        EventType::InputHeartbeat(ref p) => assert_eq!(p.active_seconds, 7),
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn ids_are_unique_even_within_same_millisecond() {
    let a = Event::new(EventType::SystemIdle, 1_700_000_004_000);
    let b = Event::new(EventType::SystemIdle, 1_700_000_004_000);
    assert_ne!(a.id, b.id);
    assert_eq!(a.id.len(), 32);
}
```

- [ ] **Step 4: 运行测试确认失败**

Run: `cd src-tauri && cargo test -p activity-core`
Expected: FAIL / ERROR（`Event`/`EventType` 未定义）。

> Workspace 成员问题：Task 2 结束时 `crates/storage` 与 `crates/collector` 还不存在，`cargo` 会因
> `members` 指向缺失目录而报错。因此 **Task 2 的 Step 1 先把 workspace `members` 写成
> `["crates/core", "."]`**，Task 3 Step 1 补回 `crates/storage`，Task 6 Step 1 补回
> `crates/collector`。若忘记，`cargo` 的错误信息会直接指出缺失路径。

- [ ] **Step 5: 实现 `src-tauri/crates/core/src/lib.rs`**

```rust
pub mod tests;

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventType {
    WindowFocus(WindowFocusPayload),
    WindowTitleChange(WindowTitleChangePayload),
    SystemIdle,
    SystemResume,
    SessionLock,
    SessionUnlock,
    InputHeartbeat(InputHeartbeatPayload),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowFocusPayload {
    pub process_name: String,
    pub window_title: Option<String>,
    pub exe_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowTitleChangePayload {
    pub process_name: String,
    pub window_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InputHeartbeatPayload {
    pub active_seconds: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub id: String,
    pub timestamp: i64,
    pub event_type: EventType,
}

impl Event {
    pub fn new(event_type: EventType, timestamp: i64) -> Self {
        let c = COUNTER.fetch_add(1, Ordering::SeqCst);
        let id = format!("{:016x}{:016x}", timestamp as u64, c);
        Self { id, timestamp, event_type }
    }
}

impl EventType {
    /// 与 serde 的 `rename_all = "snake_case"` 保持一致；storage 落库时用它填 `events.type` 列，
    /// 避免查询侧再解析一遍 JSON。
    pub fn type_tag(&self) -> &'static str {
        match self {
            EventType::WindowFocus(_) => "window_focus",
            EventType::WindowTitleChange(_) => "window_title_change",
            EventType::SystemIdle => "system_idle",
            EventType::SystemResume => "system_resume",
            EventType::SessionLock => "session_lock",
            EventType::SessionUnlock => "session_unlock",
            EventType::InputHeartbeat(_) => "input_heartbeat",
        }
    }
}
```

- [ ] **Step 6: 运行测试确认通过**

Run: `cd src-tauri && cargo test -p activity-core`
Expected: PASS（6 tests）。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/Cargo.toml src-tauri/crates/core/
git commit -m "feat(core): add Event and EventType with serde roundtrip tests"
```

---

## Task 3: storage crate — schema 与迁移

**Files:**
- Create: `src-tauri/crates/storage/Cargo.toml`
- Create: `src-tauri/crates/storage/src/lib.rs`
- Create: `src-tauri/crates/storage/src/schema.rs`
- Create: `src-tauri/crates/storage/tests/migration.rs`
- Modify: `src-tauri/Cargo.toml`（workspace members 已含 storage）

**Interfaces:**
- Consumes: 无（不依赖 core，本 task 只建表）
- Produces: `activity_storage::open_in_memory() -> rusqlite::Connection`（已应用 PRAGMA + 迁移），供测试与后续 query/writer 复用。

- [ ] **Step 1: 创建 `src-tauri/crates/storage/Cargo.toml`，并把 storage 加回 workspace members**

先在 `src-tauri/Cargo.toml` 把 `members` 改为：

```toml
members = ["crates/core", "crates/storage", "."]
```

再创建 `src-tauri/crates/storage/Cargo.toml`：

```toml
[package]
name = "activity-storage"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
rusqlite = { version = "0.32", features = ["bundled"] }
```

- [ ] **Step 2: 写失败测试 `src-tauri/crates/storage/tests/migration.rs`**

```rust
use activity_storage::open_in_memory;

#[test]
fn creates_expected_tables_and_indexes() {
    let conn = open_in_memory();
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type IN ('table','index')")
        .unwrap();
    let names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    for expected in [
        "events", "activities", "activity_evidence",
        "idx_events_timestamp", "idx_events_type_timestamp", "idx_activities_range",
    ] {
        assert!(names.iter().any(|n| n == expected), "missing {expected} in {names:?}");
    }
}

#[test]
fn journal_mode_is_wal() {
    let conn = open_in_memory();
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    // 内存库无法用 WAL，回退到 memory；此断言仅确认 PRAGMA 可执行不报错
    assert!(mode == "memory" || mode == "wal", "unexpected journal_mode {mode}");
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cd src-tauri && cargo test -p activity-storage`
Expected: FAIL（`open_in_memory` 未定义）。

- [ ] **Step 4: 实现 `src-tauri/crates/storage/src/schema.rs`**

```rust
use rusqlite::Connection;

pub const SCHEMA_VERSION: i64 = 1;

pub fn apply_pragmas(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS events (
            id TEXT PRIMARY KEY,
            timestamp INTEGER NOT NULL,
            type TEXT NOT NULL,
            payload TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
        CREATE INDEX IF NOT EXISTS idx_events_type_timestamp ON events(type, timestamp);

        CREATE TABLE IF NOT EXISTS activities (
            id TEXT PRIMARY KEY,
            start_at INTEGER NOT NULL,
            end_at INTEGER NOT NULL,
            category TEXT NOT NULL,
            application TEXT,
            confidence REAL NOT NULL,
            classifier TEXT NOT NULL,
            version TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_activities_range ON activities(start_at, end_at);

        CREATE TABLE IF NOT EXISTS activity_evidence (
            activity_id TEXT NOT NULL REFERENCES activities(id),
            event_id TEXT NOT NULL REFERENCES events(id),
            PRIMARY KEY (activity_id, event_id)
        );
        "#,
    )?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}
```

- [ ] **Step 5: 实现 `src-tauri/crates/storage/src/lib.rs`**

```rust
pub mod schema;
pub mod query;
pub mod writer;

pub use schema::{migrate, apply_pragmas, SCHEMA_VERSION};

use rusqlite::Connection;

pub fn open_in_memory() -> Connection {
    let conn = Connection::open_in_memory().expect("open in-memory db");
    apply_pragmas(&conn).expect("apply pragmas");
    migrate(&conn).expect("migrate");
    conn
}

pub fn open_file(path: &std::path::Path) -> rusqlite::Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(path)?;
    apply_pragmas(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}
```

（`query`、`writer` 模块在 Task 4/5 实现；此步先建空模块文件 `query.rs`/`writer.rs` 含一行注释占位以让 `mod` 声明成立。）

- [ ] **Step 6: 创建空 `query.rs` / `writer.rs` 占位**

`src-tauri/crates/storage/src/query.rs`:
```rust
// Query implementation lands in Task 4.
```

`src-tauri/crates/storage/src/writer.rs`:
```rust
// Batch writer lands in Task 5.
```

- [ ] **Step 7: 运行测试确认通过**

Run: `cd src-tauri && cargo test -p activity-storage`
Expected: PASS（2 tests）。

- [ ] **Step 8: 提交**

```bash
git add src-tauri/crates/storage/ src-tauri/Cargo.toml
git commit -m "feat(storage): add schema migration and pragmas with tests"
```

---

## Task 4: storage crate — 事件写入与按天范围查询

**Files:**
- Create: `src-tauri/crates/storage/src/query.rs`（替换占位）
- Create: `src-tauri/crates/storage/tests/query.rs`
- Modify: `src-tauri/crates/storage/Cargo.toml`（加 `activity-core`、`serde` 依赖）
- Modify: `src-tauri/crates/core/src/lib.rs`（已含 `type_tag`，本 task 只需调用）
- Modify: `src-tauri/crates/storage/src/lib.rs`（导出 `StoredEvent`）

**Interfaces:**
- Consumes: `activity_core::Event`、`EventType`
- Produces:
  - `activity_storage::StoredEvent { id: String, timestamp: i64, type: String, payload: String }`
  - `activity_storage::insert_events(conn: &Connection, events: &[Event]) -> rusqlite::Result<()>`（单事务批量插入）
  - `activity_storage::get_events_in_range(conn: &Connection, start_ms: i64, end_ms: i64) -> rusqlite::Result<Vec<StoredEvent>>`（半开区间 `[start_ms, end_ms)`，按 timestamp 升序）

- [ ] **Step 1: 更新 `src-tauri/crates/storage/Cargo.toml` 依赖**

在 `[dependencies]` 增加：
```toml
activity-core = { path = "../core" }
serde = { version = "1.0", features = ["derive"] }
serde_json.workspace = true
```
（`StoredEvent` 需 `serde::Serialize` 供 Tauri command 直接返回，所以要显式加 `serde`。）

- [ ] **Step 2: 写失败测试 `src-tauri/crates/storage/tests/query.rs`**

```rust
use activity_core::{Event, EventType, WindowFocusPayload};
use activity_storage::{insert_events, get_events_in_range, open_in_memory};

fn ev(ts: i64, proc: &str) -> Event {
    Event::new(
        EventType::WindowFocus(WindowFocusPayload {
            process_name: proc.into(),
            window_title: None,
            exe_path: None,
        }),
        ts,
    )
}

#[test]
fn insert_and_query_roundtrip() {
    let conn = open_in_memory();
    let a = ev(1000, "a.exe");
    let b = ev(2000, "b.exe");
    insert_events(&conn, &[a.clone(), b.clone()]).unwrap();
    let rows = get_events_in_range(&conn, 0, 3000).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, a.id);
    assert_eq!(rows[1].id, b.id);
    assert_eq!(rows[0].type, "window_focus");
}

#[test]
fn range_is_half_open_and_sorted() {
    let conn = open_in_memory();
    let events: Vec<Event> = (0..5).map(|i| ev(1000 + i * 1000, "x.exe")).collect();
    insert_events(&conn, &events).unwrap();
    // 半开区间 [1500, 3500) => ts 2000,3000
    let rows = get_events_in_range(&conn, 1500, 3500).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].timestamp, 2000);
    assert_eq!(rows[1].timestamp, 3000);
    // 升序校验
    let all = get_events_in_range(&conn, 0, i64::MAX).unwrap();
    assert!(all.windows(2).all(|w| w[0].timestamp <= w[1].timestamp));
}

#[test]
fn empty_range_returns_empty() {
    let conn = open_in_memory();
    insert_events(&conn, &[ev(1000, "a.exe")]).unwrap();
    let rows = get_events_in_range(&conn, 5000, 6000).unwrap();
    assert!(rows.is_empty());
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cd src-tauri && cargo test -p activity-storage --test query`
Expected: FAIL（`insert_events`/`get_events_in_range` 未定义）。

- [ ] **Step 4: 实现 `query.rs`**

```rust
use activity_core::Event;
use rusqlite::Connection;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredEvent {
    pub id: String,
    pub timestamp: i64,
    #[serde(rename = "type")]
    pub type_: String,
    pub payload: String,
}

pub fn insert_events(conn: &Connection, events: &[Event]) -> rusqlite::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR REPLACE INTO events (id, timestamp, type, payload, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        let now = now_ms();
        for e in events {
            let type_name = e.event_type.type_tag();
            let payload = serde_json::to_string(&e.event_type).map_err(|err| {
                rusqlite::Error::ToSqlConversionFailure(Box::new(err))
            })?;
            stmt.execute(rusqlite::params![&e.id, e.timestamp, type_name, payload, now])?;
        }
    }
    tx.commit()
}

pub fn get_events_in_range(
    conn: &Connection,
    start_ms: i64,
    end_ms: i64,
) -> rusqlite::Result<Vec<StoredEvent>> {
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, type, payload FROM events
         WHERE timestamp >= ?1 AND timestamp < ?2
         ORDER BY timestamp ASC",
    )?;
    let rows = stmt.query_map(rusqlite::params![start_ms, end_ms], |row| {
        Ok(StoredEvent {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            type_: row.get(2)?,
            payload: row.get(3)?,
        })
    })?;
    rows.collect()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
```

`events.type` 列填的是 `EventType::type_tag()`（Task 2 已在 core 定义），与 serde 的 snake_case tag 一致，
所以查询侧不必解析 JSON 就能按类型过滤/聚合。

- [ ] **Step 5: 导出 `StoredEvent` / 函数：`lib.rs` 加 `pub use query::{insert_events, get_events_in_range, StoredEvent};`**

- [ ] **Step 6: 运行测试确认通过**

Run: `cd src-tauri && cargo test -p activity-storage`
Expected: PASS（migration 2 + query 3 = 5 tests）。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/crates/core/src/lib.rs src-tauri/crates/storage/
git commit -m "feat(storage): add event batch insert and half-open range query with tests"
```

---

## Task 5: storage crate — BatchWriter（内存队列 + 5s/100 条批量刷盘）

**Files:**
- Create: `src-tauri/crates/storage/src/writer.rs`（替换占位）
- Create: `src-tauri/crates/storage/tests/writer.rs`
- Modify: `src-tauri/crates/storage/src/lib.rs`（导出 `BatchWriter`）

**Interfaces:**
- Consumes: `activity_core::Event`、`insert_events`
- Produces: `activity_storage::BatchWriter::new(conn, flush_interval_ms, batch_size) -> Self`；`push(&self, e: Event)`；`flush(&self) -> rusqlite::Result<usize>`（返回写入条数，0 表示无数据）；后台线程按 `flush_interval_ms` 定时或满 `batch_size` 触发。`Drop` 时尽力 flush。

设计为持有 `Arc<Connection>` + `Arc<Mutex<Vec<Event>>>` + 后台 `std::thread`（间隔 flush），线程在 `Drop` 时通过 `Arc` 计数自然退出（`BatchWriter` drop 后 queue 的 Arc 计数减少，worker 检测到 sender 断开退出）。为简化且可测，本实现用 **手动 flush + 后台定时线程**；测试直接调 `flush()` 验证。

- [ ] **Step 1: 写失败测试 `src-tauri/crates/storage/tests/writer.rs`**

```rust
use activity_core::{Event, EventType};
use activity_storage::{get_events_in_range, open_in_memory, BatchWriter};
use std::sync::Arc;

fn ev(ts: i64) -> Event {
    Event::new(EventType::SystemIdle, ts)
}

#[test]
fn push_then_flush_writes_all() {
    let conn = Arc::new(open_in_memory());
    let w = BatchWriter::new(Arc::clone(&conn), 60_000, 100);
    for i in 0..10 {
        w.push(ev(1000 + i));
    }
    let n = w.flush().unwrap();
    assert_eq!(n, 10);
    let rows = get_events_in_range(&conn, 0, i64::MAX).unwrap();
    assert_eq!(rows.len(), 10);
}

#[test]
fn flush_on_empty_is_noop() {
    let conn = Arc::new(open_in_memory());
    let w = BatchWriter::new(conn, 60_000, 100);
    assert_eq!(w.flush().unwrap(), 0);
}

#[test]
fn repeated_flush_does_not_duplicate_rows() {
    let conn = Arc::new(open_in_memory());
    let w = BatchWriter::new(Arc::clone(&conn), 60_000, 100);
    w.push(ev(1000));
    assert_eq!(w.flush().unwrap(), 1);
    assert_eq!(w.flush().unwrap(), 0);
    let rows = get_events_in_range(&conn, 0, i64::MAX).unwrap();
    assert_eq!(rows.len(), 1);
}

#[test]
fn background_thread_auto_flushes_without_manual_call() {
    let conn = Arc::new(open_in_memory());
    // flush_interval=0ms：验证后台线程确实会自行刷盘，不依赖手动 flush()
    let w = BatchWriter::new(Arc::clone(&conn), 0, 100);
    w.push(ev(1000));
    w.push(ev(2000));
    std::thread::sleep(std::time::Duration::from_millis(500));
    let rows = get_events_in_range(&conn, 0, i64::MAX).unwrap();
    assert_eq!(rows.len(), 2, "background thread should flush on its own");
}
- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test -p activity-storage --test writer`
Expected: FAIL（`BatchWriter` 未定义）。

- [ ] **Step 3: 实现 `writer.rs`**

```rust
use activity_core::Event;
use rusqlite::Connection;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::query::insert_events;

pub struct BatchWriter {
    conn: Arc<Connection>,
    queue: Arc<Mutex<Vec<Event>>>,
    flush_interval_ms: u64,
    batch_size: usize,
    stop: Arc<AtomicBool>,
    written: Arc<AtomicUsize>,
}

impl BatchWriter {
    pub fn new(conn: Arc<Connection>, flush_interval_ms: u64, batch_size: usize) -> Self {
        let queue = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let written = Arc::new(AtomicUsize::new(0));

        let q = Arc::clone(&queue);
        let s = Arc::clone(&stop);
        let w = Arc::clone(&written);
        let c = Arc::clone(&conn);
        std::thread::spawn(move || {
            let mut last = std::time::Instant::now();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if s.load(Ordering::Relaxed) {
                    // 退出前冲一次
                    let batch: Vec<Event> = {
                        let mut g = q.lock().unwrap();
                        std::mem::take(&mut *g)
                    };
                    if !batch.is_empty() {
                        let _ = insert_events(&c, &batch);
                        w.fetch_add(batch.len(), Ordering::Relaxed);
                    }
                    break;
                }
                let should = last.elapsed().as_millis() as u64 >= flush_interval_ms
                    || q.lock().unwrap().len() >= batch_size;
                if should {
                    let batch: Vec<Event> = {
                        let mut g = q.lock().unwrap();
                        std::mem::take(&mut *g)
                    };
                    if !batch.is_empty() {
                        let _ = insert_events(&c, &batch);
                        w.fetch_add(batch.len(), Ordering::Relaxed);
                        last = std::time::Instant::now();
                    }
                }
            }
        });

        Self { conn, queue, flush_interval_ms, batch_size, stop, written }
    }

    pub fn push(&self, e: Event) {
        let mut g = self.queue.lock().unwrap();
        g.push(e);
    }

    pub fn flush(&self) -> rusqlite::Result<usize> {
        let batch: Vec<Event> = {
            let mut g = self.queue.lock().unwrap();
            std::mem::take(&mut *g)
        };
        if batch.is_empty() {
            return Ok(0);
        }
        let n = batch.len();
        insert_events(&self.conn, &batch)?;
        self.written.fetch_add(n, Ordering::Relaxed);
        Ok(n)
    }

    pub fn conn(&self) -> &Arc<Connection> {
        &self.conn
    }
}

impl Drop for BatchWriter {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
```

- [ ] **Step 4: 导出 `BatchWriter`：`lib.rs` 加 `pub use writer::BatchWriter;`**

- [ ] **Step 5: 运行测试确认通过**

Run: `cd src-tauri && cargo test -p activity-storage`
Expected: PASS（migration 2 + query 3 + writer 4 = 9 tests）。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/crates/storage/
git commit -m "feat(storage): add BatchWriter with 5s/100-queue flush and tests"
```

---

## Task 6: Tauri 应用壳 + 依赖 collector/storage/core 打通（能 build）

**Files:**
- Create: `src-tauri/build.rs`
- Create: `src-tauri/tauri.conf.json`
- Create: `src-tauri/capabilities/default.json`
- Create: `src-tauri/icons/icon.ico`（`pnpm tauri icon` 生成，需先有占位）
- Create: `src-tauri/src/main.rs`
- Create: `src-tauri/crates/collector/Cargo.toml`（最小占位 lib）
- Create: `src-tauri/crates/collector/src/lib.rs`（占位）
- Create: `src-tauri/crates/collector/src/sleep.rs`（占位）

**Interfaces:**
- Consumes: `activity_core`、`activity_storage`（本 task 只链接，不接采集逻辑，采集在 Task 7/8）
- Produces: `pnpm tauri build`（或至少 `cargo build -p time-scope`）成功；Tauri 窗口能开。

- [ ] **Step 1: 补齐 workspace members，并创建 `src-tauri/build.rs`**

先把 `src-tauri/Cargo.toml` 的 `members` 改为：

```toml
[workspace]
members = ["crates/core", "crates/storage", "crates/collector", "."]
resolver = "2"
```

再把 `[dependencies]` 补上两个 crate，并把 `[package]` / `[lib]` 段加在 `[workspace]` 之后
（`time` 已在 Task 2 Step 1 加好）：

```toml
[package]
name = "time-scope"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
name = "time_scope_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[dependencies]
activity-storage = { path = "crates/storage" }
activity-collector = { path = "crates/collector" }
```

`src-tauri/Cargo.toml` 至此的完整形状：

```toml
[workspace]
members = ["crates/core", "crates/storage", "crates/collector", "."]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"

[workspace.dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

[package]
name = "time-scope"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
name = "time_scope_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[dependencies]
tauri = { version = "2.1", features = [] }
serde.workspace = true
serde_json.workspace = true
time = { version = "0.3", features = ["local-offset", "parsing", "formatting", "macros"] }
activity-core = { path = "crates/core" }
activity-storage = { path = "crates/storage" }
activity-collector = { path = "crates/collector" }

[build-dependencies]
tauri-build = { version = "2.0", features = [] }
```

创建 `src-tauri/build.rs`：

```rust
fn main() {
    tauri_build::build()
}
```

- [ ] **Step 2: 创建 `src-tauri/tauri.conf.json`**

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Time Scope",
  "version": "0.1.0",
  "identifier": "com.timescope.app",
  "build": {
    "beforeDevCommand": "pnpm dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "pnpm build",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      {
        "title": "Time Scope",
        "width": 1200,
        "height": 700,
        "minWidth": 800,
        "minHeight": 500
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": ["icons/icon.ico"]
  }
}
```

- [ ] **Step 3: 创建 `src-tauri/capabilities/default.json`**

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Default capability for the main window",
  "windows": ["main"],
  "permissions": ["core:default"]
}
```

- [ ] **Step 4: 创建 `src-tauri/src/main.rs` + `src-tauri/src/lib.rs`**

`main.rs` 保持薄壳，实际入口在 lib（Task 9 会重写 lib.rs）：

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    time_scope_lib::run()
}
```

`src-tauri/src/lib.rs`：

```rust
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 5: 创建 `src-tauri/crates/collector/Cargo.toml`**

```toml
[package]
name = "activity-collector"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
activity-core = { path = "../core" }
serde.workspace = true
```

- [ ] **Step 6: 创建 collector 占位 `src/lib.rs` / `src/sleep.rs`**

`src/lib.rs`:
```rust
pub mod sleep;
// Window/input collection lands in Tasks 7-8.
```

`src/sleep.rs`:
```rust
// Sleep placeholder to keep the crate valid; real impl in Task 8.
```

- [ ] **Step 7: 生成图标**

Run: `pnpm tauri icon <path-to-any-256px-png>`（若无 png，先用任意 256x256 图片）
Expected: 生成 `src-tauri/icons/icon.ico` 等。若无源图，可先 `pnpm tauri dev` 前跳过、build 时报缺图标——必须补。可用最小脚本生成纯色 png（见下方命令）。

替代（无外部图片时用 PowerShell 生成 256x256 png）：
```powershell
Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap 256,256
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.Clear([System.Drawing.Color]::FromArgb(40,120,200))
$bmp.Save("$PWD\src-tauri\icon-src.png")
```
然后：`pnpm tauri icon src-tauri/icon-src.png`

- [ ] **Step 8: 验证 Rust 侧能编译**

Run: `cd src-tauri && cargo build`
Expected: 成功（生成 time-scope 可执行；此时前端 dist 未建，不影响 Rust 编译）。

- [ ] **Step 9: 提交**

```bash
git add src-tauri/
git commit -m "feat(tauri): scaffold Tauri 2 app shell and link workspace crates"
```

---

## Task 7: collector — 窗口事件采集（SetWinEventHook + 专用消息泵线程）

**Files:**
- Modify: `src-tauri/crates/collector/Cargo.toml`（加 `windows` 依赖）
- Create: `src-tauri/crates/collector/src/signals.rs`
- Create: `src-tauri/crates/collector/src/window.rs`
- Modify: `src-tauri/crates/collector/src/lib.rs`
- Create: `src-tauri/crates/collector/tests/smoke.rs`（`#[ignore]` 手动跑）

**Interfaces:**
- Consumes: `activity_core::Event`、`EventType`、`WindowFocusPayload`
- Produces:
  - `activity_collector::signals::RawSignal::WindowFocus(HWND)`、`RawSignal::WindowTitleChange(HWND)`
  - `activity_collector::window::spawn_window_watcher(sender: std::sync::mpsc::Sender<RawSignal>) -> windows::Win32::Foundation::HWND`（返回的 HWND 用于停止，实际用 `AtomicBool`）
  - `activity_collector::window::stop_watcher()`

设计（spec §5.1）：hook 注册在**专用 OS 线程**（非 tokio），该线程跑原生 `GetMessage/DispatchMessage` 消息泵。hook 回调只把 HWND 塞进 channel，不做任何重活。取标题/进程名在 consumer 线程做。

- [ ] **Step 1: 更新 `src-tauri/crates/collector/Cargo.toml`**

```toml
[package]
name = "activity-collector"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
activity-core = { path = "../core" }
serde.workspace = true

[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = [
  "Win32_Foundation",
  "Win32_UI_WindowsAndMessaging",
  "Win32_System_Threading",
  "Win32_System_ProcessStatus",
  "Win32_System_StationsAndDesktops",
] }
```

- [ ] **Step 2: 创建 `src-tauri/crates/collector/src/signals.rs`**

```rust
//! collector 内部信号：从 OS 采集线程传给 consumer 线程的最小载荷。
//!
//! 只传 HWND（窗口事件）或一个数字（输入事件），**不在这一层取标题/进程名**——
//! 那是重活，留给 consumer 线程（spec §5.1）。

#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::HWND;

#[derive(Debug, Clone, Copy)]
pub enum RawSignal {
    /// 前台窗口切换
    #[cfg(windows)]
    WindowFocus(HWND),
    /// 同一窗口标题变化（浏览器切 Tab）
    #[cfg(windows)]
    WindowTitleChange(HWND),
    /// 无输入超过 idle_threshold
    IdleStart,
    /// 从 idle 恢复到有输入
    InputActive,
    /// 一个 heartbeat 窗口内的活跃秒数（0..=heartbeat_every_s）
    Heartbeat(u8),
    /// 非 Windows 平台的占位，使枚举在所有平台可编译
    #[cfg(not(windows))]
    _Noop,
}
```

> Task 8 只需用到 `IdleStart` / `InputActive` / `Heartbeat`，它们已在此定义好，
> 不需要回头改本文件。

- [ ] **Step 3: 实现 `src-tauri/crates/collector/src/window.rs`**

```rust
//! Windows 前台窗口采集。
//!
//! 关键约束（spec §5.1）：调用 `SetWinEventHook` 的线程**必须有 Windows 消息循环**，
//! 否则注册成功但回调永不触发。所以这里用 `std::thread::spawn` 起专用线程跑原生
//! `GetMessageW` / `DispatchMessageW` 消息泵，**不要**放进 tokio task。
//!
//! 回调内只做最轻的事：把 HWND 塞进 channel。取标题/进程名由 consumer 线程完成。

use crate::signals::RawSignal;
use activity_core::{Event, EventType, WindowFocusPayload, WindowTitleChangePayload};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::OnceLock;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{
    GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::*;

static RUNNING: AtomicBool = AtomicBool::new(false);

/// watcher 线程的 id；`stop_window_watcher` 用它把 `WM_QUIT` 投递到**正确**的线程。
static WATCHER_TID: AtomicU32 = AtomicU32::new(0);

/// 回调里要用的 sender。回调签名是裸 `extern "system" fn`，不能捕获环境，
/// 所以用全局 `OnceLock`（进程内只 spawn 一次，够用）。
static SENDER: OnceLock<Sender<RawSignal>> = OnceLock::new();

extern "system" fn event_hook_callback(
    hwnd: HWND,
    event: WINEVENT,
    _wparam: WPARAM,
    _lparam: LPARAM,
    _hook: *mut c_void,
) {
    // 回调内只做最轻操作：塞 HWND。不取标题、不分配字符串、不碰 DB。
    let sender = match SENDER.get() {
        Some(s) => s,
        None => return,
    };
    let signal = match event.0 {
        x if x == EVENT_SYSTEM_FOREGROUND.0 => RawSignal::WindowFocus(hwnd),
        x if x == EVENT_OBJECT_NAMECHANGE.0 => RawSignal::WindowTitleChange(hwnd),
        _ => return,
    };
    let _ = sender.send(signal);
}

/// 启动窗口 watcher 线程。进程内只能调用一次（`SENDER` 是一次性的）。
pub fn spawn_window_watcher(sender: Sender<RawSignal>) {
    RUNNING.store(true, Ordering::SeqCst);
    let _ = SENDER.set(sender);
    std::thread::spawn(|| {
        let tid = unsafe { GetCurrentThreadId() };
        WATCHER_TID.store(tid, Ordering::SeqCst);

        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_OBJECT_NAMECHANGE,
                None,
                Some(event_hook_callback),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if hook.is_invalid() {
            eprintln!("SetWinEventHook failed: {:?}", windows::Win32::Foundation::GetLastError());
            return;
        }

        // 原生消息泵：没有它回调永不触发（spec §5.1）
        let mut msg: MSG = std::mem::zeroed();
        loop {
            let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            if r.0 <= 0 {
                break;
            }
            if !RUNNING.load(Ordering::SeqCst) {
                break;
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        unsafe {
            let _ = UnhookWinEvent(hook);
        }
        WATCHER_TID.store(0, Ordering::SeqCst);
    });
}

/// 停止 watcher。必须把 `WM_QUIT` 投给 **watcher 线程**——
/// `GetMessageW` 阻塞时只有收��消息才会返回，单纯置 flag 不会唤醒它。
pub fn stop_window_watcher() {
    RUNNING.store(false, Ordering::SeqCst);
    let tid = WATCHER_TID.load(Ordering::SeqCst);
    if tid != 0 {
        unsafe {
            let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

/// 取窗口的 (process_name, window_title, exe_path)。取不到进程名时返回 None。
pub fn window_info(hwnd: HWND) -> Option<(String, Option<String>, Option<String>)> {
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    if pid == 0 {
        return None;
    }

    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }?;
    let mut buf = [0u16; 32768];
    let mut len = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(h, 0, &mut buf, &mut len) };
    unsafe {
        let _ = windows::Win32::Foundation::CloseHandle(h);
    }
    let exe_path = if ok.as_bool() {
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    } else {
        None
    };
    let process_name = exe_path
        .as_deref()
        .and_then(|p| p.rsplit(['\\', '/']).next())
        .map(|s| s.to_string());

    // 窗口标题：GetWindowTextLengthW <= 0 表示无标题（部分系统窗口 / UWP，Review Focus #4）
    let title = unsafe {
        let n = GetWindowTextLengthW(hwnd);
        if n <= 0 {
            None
        } else {
            let mut t = vec![0u16; (n + 1) as usize];
            let got = GetWindowTextW(hwnd, &mut t);
            Some(String::from_utf16_lossy(&t[..got as usize]))
        }
    };

    let process_name = process_name?;
    Some((process_name, title, exe_path))
}

pub fn make_focus_event(hwnd: HWND, ts: i64) -> Option<Event> {
    let (process_name, window_title, exe_path) = window_info(hwnd)?;
    Some(Event::new(
        EventType::WindowFocus(WindowFocusPayload {
            process_name,
            window_title,
            exe_path,
        }),
        ts,
    ))
}

pub fn make_title_event(hwnd: HWND, ts: i64) -> Option<Event> {
    let (process_name, window_title, _exe) = window_info(hwnd)?;
    Some(Event::new(
        EventType::WindowTitleChange(WindowTitleChangePayload {
            process_name,
            window_title,
        }),
        ts,
    ))
}
```

两点 Win32 细节（已在上面代码里落实，实现时不要改回）：

- `GetCurrentThreadId` 来自 `Win32_System_Threading` feature（Step 1 已加），用它拿到 watcher 线程 id。
- `PostThreadMessageW(0, ...)` 的 `0` 指**当前**线程，投递不到 watcher。必须传 `WATCHER_TID`。
  只置 `RUNNING=false` 也不行——`GetMessageW` 阻塞时不会被唤醒，线程会泄漏。


- [ ] **Step 4: 更新 `src-tauri/crates/collector/src/lib.rs`**

```rust
pub mod signals;
pub mod sleep;
pub mod window;
```

- [ ] **Step 5: 创建手动验证测试 `src-tauri/crates/collector/tests/smoke.rs`**

```rust
#![cfg(windows)]
// 手动运行：cargo test -p activity-collector --test smoke -- --ignored --nocapture
use activity_collector::signals::RawSignal;
use activity_collector::window::{spawn_window_watcher, stop_window_watcher, make_focus_event};
use std::sync::mpsc::channel;
use windows::Win32::UI::WindowsAndMessaging::HWND;

#[test]
#[ignore]
fn manual_window_switch_capture() {
    let (tx, rx) = channel();
    spawn_window_watcher(tx);
    eprintln!("请在 20s 内切换几个窗口/程序，然后回车。");
    std::thread::sleep(std::time::Duration::from_secs(20));
    stop_window_watcher();
    let mut got = 0;
    while let Ok(sig) = rx.try_recv() {
        if let RawSignal::WindowFocus(hwnd) = sig {
            let hwnd: HWND = hwnd;
            if let Some(e) = make_focus_event(hwnd, 0) {
                eprintln!("{:?}", e);
                got += 1;
            }
        }
    }
    assert!(got > 0, "should capture at least one focus change");
}
```

- [ ] **Step 6: 编译检查**

Run: `cd src-tauri && cargo build -p activity-collector`
Expected: 成功。若 windows crate API 有出入，按编译器错误调整签名。

- [ ] **Step 7: 手动跑 smoke 测试验证**

Run: `cd src-tauri && cargo test -p activity-collector --test smoke -- --ignored --nocapture`
Expected: 切换窗口后 PASS，输出若干 focus 事件。（需手动切窗口。）

- [ ] **Step 8: 提交**

```bash
git add src-tauri/crates/collector/
git commit -m "feat(collector): capture window focus via SetWinEventHook with native message pump"
```

---

## Task 8: collector — 输入 idle 检测与活动心跳（GetLastInputInfo）

**Files:**
- Create: `src-tauri/crates/collector/src/input.rs`
- Create: `src-tauri/crates/collector/tests/input_logic.rs`
- Modify: `src-tauri/crates/collector/src/lib.rs`（加 `pub mod input;`）
- Modify: `src-tauri/crates/collector/Cargo.toml`（加 `Win32_UI_Input_KeyboardAndMouse` / `Win32_System_SystemInformation` feature）

**Interfaces:**
- Consumes: `activity_core::{Event, EventType, InputHeartbeatPayload}`、`RawSignal::{IdleStart, InputActive, Heartbeat}`
- Produces:
  - `activity_collector::input::idle_seconds(now_tick: u64, last_input_tick: u64) -> u32`（纯函数，可测）
  - `activity_collector::input::spawn_input_poller(sender: Sender<RawSignal>, idle_threshold_s: u64, heartbeat_every_s: u64)`
  - `activity_collector::input::stop_input_poller()`
  - `make_idle_event(ts)` / `make_resume_event(ts)` / `make_heartbeat_event(active_seconds, ts)`

设计（spec §5.3）：每 1s 轮询 `GetLastInputInfo`（**必须设 `cbSize`**，否则恒 0）。空闲秒 = `GetTickCount64 - lastInputTick`。超阈值产生 `SystemIdle`；恢复输入产生 `SystemResume`。每 10s 聚合一个 `InputHeartbeat { active_seconds }`。

- [ ] **Step 1: 补全 collector 的 `windows` features**

Task 7 Step 1 的 `features` 列表缺两个模块，本步补上：

```toml
[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = [
  "Win32_Foundation",
  "Win32_UI_WindowsAndMessaging",
  "Win32_UI_Input_KeyboardAndMouse",   # 新增：GetLastInputInfo / LASTINPUTINFO
  "Win32_System_Threading",
  "Win32_System_ProcessStatus",
  "Win32_System_SystemInformation",     # 新增：GetTickCount64
  "Win32_System_StationsAndDesktops",
] }
```

- [ ] **Step 2: 写纯逻辑单测 `src-tauri/crates/collector/tests/input_logic.rs`**

```rust
use activity_collector::input::idle_seconds;

#[test]
fn idle_seconds_computes_gap_in_whole_seconds() {
    assert_eq!(idle_seconds(10_000, 4_000), 6);
    assert_eq!(idle_seconds(5_000, 5_000), 0);
    assert_eq!(idle_seconds(6_500, 4_000), 2, "sub-second remainder truncates");
}

#[test]
fn idle_seconds_saturates_when_last_input_is_newer() {
    // 理论上不会发生（last_input_tick <= now_tick），但不能 panic/回绕
    assert_eq!(idle_seconds(1_000, 5_000), 0);
}

#[test]
fn idle_threshold_boundary() {
    // 阈值判定是 `idle >= threshold`，恰好等于阈值时应判定为 idle
    assert!(idle_seconds(300_000, 0) >= 300);
    assert!(idle_seconds(299_000, 0) < 300);
}
```

- [ ] **Step 3: 运行单测确认失败**

Run: `cd src-tauri && cargo test -p activity-collector --test input_logic`
Expected: FAIL（`input` 模块 / `idle_seconds` 未定义）。

- [ ] **Step 4: 实现 `src-tauri/crates/collector/src/input.rs`**

```rust
//! 输入活动与空闲检测（spec §5.3）。
//!
//! 每 1s 轮询一次 `GetLastInputInfo`，**状态变化才发信号**——不轮询写库。
//!
//! Win32 坑：`LASTINPUTINFO.cbSize` 必须显式设置，否则 API 直接失败、`dwTime` 恒为 0，
//! 表现为 idle_seconds 永远是 0、永远不会触发 SystemIdle。

use crate::signals::RawSignal;
use activity_core::{Event, EventType, InputHeartbeatPayload};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

static RUNNING: AtomicBool = AtomicBool::new(false);

/// 距上次输入的秒数。纯函数，抽出以便单测。
pub fn idle_seconds(now_tick: u64, last_input_tick: u64) -> u32 {
    (now_tick.saturating_sub(last_input_tick) / 1000) as u32
}

#[cfg(windows)]
fn last_input_tick() -> u64 {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, // 不设则恒返回 0
        dwTime: 0,
    };
    if unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        info.dwTime as u64
    } else {
        0
    }
}

#[cfg(windows)]
fn now_tick() -> u64 {
    unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }
}

/// 启动输入轮询线程。
/// * `idle_threshold_s` — 无输入多少秒后判定空闲（spec 默认 300）
/// * `heartbeat_every_s` — 心跳聚合窗口（spec 默认 10）
pub fn spawn_input_poller(
    sender: Sender<RawSignal>,
    idle_threshold_s: u64,
    heartbeat_every_s: u64,
) {
    RUNNING.store(true, Ordering::SeqCst);
    std::thread::spawn(move || {
        #[cfg(not(windows))]
        {
            let _ = (sender, idle_threshold_s, heartbeat_every_s);
            return;
        }
        #[cfg(windows)]
        {
            let mut was_idle = false;
            let mut active_in_window: u8 = 0;
            let mut last_tick = last_input_tick();
            let mut window_start_tick = last_tick;
            let heartbeat_ms = heartbeat_every_s.saturating_mul(1000);

            while RUNNING.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let now = now_tick();
                let last = last_input_tick();

                if last != last_tick {
                    // dwTime 变化 => 这一秒内有输入
                    last_tick = last;
                    active_in_window = active_in_window.saturating_add(1);
                    if was_idle {
                        was_idle = false;
                        let _ = sender.send(RawSignal::InputActive);
                    }
                }

                if !was_idle && idle_seconds(now, last) as u64 >= idle_threshold_s {
                    was_idle = true;
                    let _ = sender.send(RawSignal::IdleStart);
                }

                if now.saturating_sub(window_start_tick) >= heartbeat_ms {
                    let _ = sender.send(RawSignal::Heartbeat(active_in_window));
                    active_in_window = 0;
                    window_start_tick = now;
                }
            }
        }
    });
}

pub fn stop_input_poller() {
    RUNNING.store(false, Ordering::SeqCst);
}

pub fn make_idle_event(ts: i64) -> Event {
    Event::new(EventType::SystemIdle, ts)
}

pub fn make_resume_event(ts: i64) -> Event {
    Event::new(EventType::SystemResume, ts)
}

pub fn make_heartbeat_event(active_seconds: u8, ts: i64) -> Event {
    Event::new(
        EventType::InputHeartbeat(InputHeartbeatPayload { active_seconds }),
        ts,
    )
}
```

更新 `src-tauri/crates/collector/src/lib.rs`：

```rust
pub mod signals;
pub mod sleep;
pub mod window;
pub mod input;
```

行为说明：

- idle 期间心跳仍会发，但 `active_seconds` 为 0。engine 后续负责不把它计入活跃（spec §7.3）。
- `was_idle` 保证状态不变时不重复发信号（spec §5.4：只有状态变化才产 Event）。
- `idle_seconds(now, last) >= idle_threshold` 用 `>=`，与 spec §5.3 阈值语义一致。

- [ ] **Step 5: 运行单测确认通过**

Run: `cd src-tauri && cargo test -p activity-collector --test input_logic`
Expected: PASS（3 tests）。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/crates/collector/
git commit -m "feat(collector): add input idle detection and heartbeat polling"
```

---

## Task 9: Tauri commands + 事件总线接线（collector → storage → IPC）

**Files:**
- Create: `src-tauri/src/lib.rs`（重写，含 state、commands、事件消费线程）
- Modify: `src-tauri/crates/collector/src/lib.rs`（追加 `consumer` 模块）
- Create: `src-tauri/crates/collector/src/consumer.rs`
- Create: `src-tauri/crates/collector/tests/consumer_smoke.rs`（`#[ignore]`）

**Interfaces:**
- Consumes: `RawSignal`、`make_*_event`、`BatchWriter`
- Produces:
  - `activity_collector::consumer::spawn_consumer(rx: Receiver<RawSignal>, writer: Arc<BatchWriter>)`（后台线程，收 signal → 生成 Event → `writer.push`）
  - Tauri commands（在 app crate）：
    - `get_events(date: String) -> Result<Vec<StoredEvent>, String>`：按本地时区把 `YYYY-MM-DD` 转 `[start_ms, end_ms)`，调 `get_events_in_range`
    - `get_db_path() -> String`
  - 状态：`tauri::State` 里持有 `Arc<BatchWriter>`（连接也需可查，BatchWriter 已带 conn）

- [ ] **Step 1: 实现 `src-tauri/crates/collector/src/consumer.rs`**

```rust
use crate::signals::RawSignal;
use crate::window::{make_focus_event, make_title_event};
use crate::input::{make_idle_event, make_resume_event, make_heartbeat_event};
use activity_storage::BatchWriter;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

pub fn spawn_consumer(rx: Receiver<RawSignal>, writer: Arc<BatchWriter>) {
    std::thread::spawn(move || {
        while let Ok(sig) = rx.recv() {
            let now = now_ms();
            let event = match sig {
                #[cfg(windows)]
                RawSignal::WindowFocus(h) => make_focus_event(h, now),
                #[cfg(windows)]
                RawSignal::WindowTitleChange(h) => make_title_event(h, now),
                RawSignal::IdleStart => Some(make_idle_event(now)),
                RawSignal::InputActive => Some(make_resume_event(now)),
                RawSignal::Heartbeat(sec) => Some(make_heartbeat_event(sec, now)),
                #[cfg(not(windows))]
                _ => None,
            };
            if let Some(e) = event {
                writer.push(e);
            }
        }
    });
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
```

- [ ] **Step 2: 更新 collector `lib.rs`**

```rust
pub mod consumer;
pub mod input;
pub mod signals;
pub mod sleep;
pub mod window;
```

- [ ] **Step 3: 写 `day_range_ms` 的失败单测 `src-tauri/src/date_range.rs`**

> 先拆出可测的日期换算模块，再写 `lib.rs` 接线。`time` crate 已在 Task 2 Step 1 声明。

```rust
use time::{format_description::well_known, Date, PrimitiveDateTime, UtcOffset};

/// 把 `YYYY-MM-DD` 转成本地时区下该日的半开区间 `[00:00, 次日00:00)` 的 Unix 毫秒。
///
/// Review Focus #2：应用 24h 常驻，跨零点后查询“今天”必须落在新日期内，
/// 而昨天已落库的数据仍应能查出来——这要求端点用本地时区且是半开区间。
pub fn day_range_ms(date: &str, offset: UtcOffset) -> Result<(i64, i64), String> {
    let d = Date::parse(date, &well_known::Date).map_err(|e| format!("bad date {date}: {e}"))?;
    let next = d.next_day().map_err(|_| format!("date out of range: {date}"))?;
    let start = PrimitiveDateTime::new(d, time::Time::MIDNIGHT).assume_offset(offset);
    let end = PrimitiveDateTime::new(next, time::Time::MIDNIGHT).assume_offset(offset);
    Ok((
        start.unix_timestamp() * 1000,
        end.unix_timestamp() * 1000,
    ))
}

/// 取当前本地 UTC 偏移。传入而非内部读取，是为了让单测能固定偏移、不依赖机器时区。
pub fn local_offset() -> Result<UtcOffset, String> {
    UtcOffset::current_local_offset().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::day_range_ms;
    use time::{macros::offset, UtcOffset};

    const CST: UtcOffset = offset!("+08:00");

    #[test]
    fn range_is_24h_in_fixed_offset() {
        let (s, e) = day_range_ms("2026-10-01", CST).unwrap();
        assert_eq!(e - s, 86_400_000);
    }

    #[test]
    fn start_matches_local_midnight() {
        let (s, _e) = day_range_ms("2026-10-01", CST).unwrap();
        // 2026-10-01T00:00:00+08:00 == 2026-09-30T16:00:00Z
        assert_eq!(s, 1_772_236_800_000);
    }

    #[test]
    fn end_is_next_day_midnight_not_inclusive() {
        let (s, e) = day_range_ms("2026-10-01", CST).unwrap();
        let (s2, _e2) = day_range_ms("2026-10-02", CST).unwrap();
        assert_eq!(e, s2, "end of one day must equal start of the next");
        assert!(e > s);
    }

    #[test]
    fn offset_shifts_window_without_changing_length() {
        let (s8, e8) = day_range_ms("2026-10-01", CST).unwrap();
        let (s0, e0) = day_range_ms("2026-10-01", UtcOffset::UTC).unwrap();
        assert_eq!(e8 - s8, e0 - s0);
        assert_eq!(s8 - s0, 8 * 3_600_000);
    }

    #[test]
    fn leap_day_is_accepted() {
        let (s, e) = day_range_ms("2028-02-29", CST).unwrap();
        assert_eq!(e - s, 86_400_000);
    }

    #[test]
    fn invalid_dates_are_rejected() {
        assert!(day_range_ms("2026-13-01", CST).is_err());
        assert!(day_range_ms("2026-02-30", CST).is_err());
        assert!(day_range_ms("not-a-date", CST).is_err());
        assert!(day_range_ms("", CST).is_err());
    }
}
```

- [ ] **Step 4: 运行单测确认失败**

Run: `cd src-tauri && cargo test --lib date_range`
Expected: FAIL（`day_range_ms` 未实现）。

> 此时 `src-tauri/src/lib.rs` 还是 Task 6 的最小版本，`cargo test --lib` 不会编译到本模块。
> 把 Step 5 的 `lib.rs`（含 `mod date_range;`）先写出来再跑本步，或直接跳到 Step 5 后跑 Step 6。

- [ ] **Step 5: 改写 `src-tauri/src/lib.rs`（完整接线）**

```rust
mod date_range;

use activity_collector::signals::RawSignal;
use activity_storage::{open_file, BatchWriter, StoredEvent};
use date_range::{day_range_ms, local_offset};
use std::sync::mpsc::channel;
use std::sync::Arc;
use tauri::Manager;

struct AppState {
    writer: Arc<BatchWriter>,
}

fn db_path() -> std::path::PathBuf {
    let appdata = std::env::var("APPDATA").expect("APPDATA env var");
    std::path::Path::new(&appdata)
        .join("time-scope")
        .join("time-scope.db")
}

#[tauri::command]
fn get_events(
    state: tauri::State<'_, AppState>,
    date: String,
) -> Result<Vec<StoredEvent>, String> {
    let (start_ms, end_ms) = day_range_ms(&date, local_offset()?)?;
    activity_storage::get_events_in_range(state.writer.conn(), start_ms, end_ms)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_db_path() -> String {
    db_path().to_string_lossy().into_owned()
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let path = db_path();
            let conn = open_file(&path).expect("open db");
            let writer = Arc::new(BatchWriter::new(Arc::new(conn), 5_000, 100));

            let (tx, rx) = channel::<RawSignal>();
            activity_collector::window::spawn_window_watcher(tx.clone());
            activity_collector::input::spawn_input_poller(tx.clone(), 300, 10);
            activity_collector::consumer::spawn_consumer(rx, Arc::clone(&writer));

            app.manage(AppState { writer });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_events, get_db_path])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

说明：

- `db_path()` 对应 spec §8.1 的 `%APPDATA%/time-scope/time-scope.db`；`open_file` 内部会 `create_dir_all`。
- 三个线程（window watcher / input poller / consumer）在 `setup` 里拉起，符合 spec §3 的线程模型。
- `BatchWriter::new(..., 5_000, 100)` 即 spec §8.1 的“每 5s 或满 100 条”。
- `idle_threshold=300`、`heartbeat=10` 对应 spec §5.3 的默认参数。

- [ ] **Step 6: 全量编译 + 全部 Rust 测试**

Run: `cd src-tauri && cargo test`
Expected: core 6 + storage 9 + input_logic 3 + date_range 6 = 24 tests 全 PASS；collector 的 smoke 测试因 `#[ignore]` 跳过。

Run: `cd src-tauri && cargo build --release -p time-scope`
Expected: 成功（确认 tauri 宏、capabilities、icon 配置在 release 下也成立）。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/
git commit -m "feat(tauri): wire collector->BatchWriter->IPC with get_events command and local-time day range"
```

---

## Task 10: React 前端 — 调用 get_events 并渲染 24h 事件时间线

**Files:**
- Create: `src/types.ts`
- Create: `src/components/Timeline.tsx`
- Create: `src/components/EventDetail.tsx`
- Modify: `src/App.tsx`
- Create: `src/components/Timeline.test.tsx`
- Create: `src/__mocks__/tauri.ts`（测试用 mock）

**Interfaces:**
- Consumes: Tauri command `get_events(date) -> StoredEvent[]`；`StoredEvent = { id, timestamp, type, payload }`
- Produces: 24h 横向时间线，事件按 timestamp 定位为色块；点击显示详情。

- [ ] **Step 1: 创建 `src/types.ts`**

```ts
export interface StoredEvent {
  id: string;
  timestamp: number;
  type: string;
  payload: string;
}

export type EventCategory =
  | "window_focus"
  | "window_title_change"
  | "system_idle"
  | "system_resume"
  | "session_lock"
  | "session_unlock"
  | "input_heartbeat";

export interface WindowFocusPayload {
  process_name: string;
  window_title: string | null;
  exe_path: string | null;
}

export async function getEvents(date: string): Promise<StoredEvent[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<StoredEvent[]>("get_events", { date });
}

export function todayString(): string {
  const d = new Date();
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const dd = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${dd}`;
}
```

- [ ] **Step 2: 写失败测试 `src/components/Timeline.test.tsx`**

```tsx
import { describe, it, expect } from "vitest";
import { render } from "@testing-library/react";
import Timeline from "./Timeline";
import type { StoredEvent } from "../types";

const base: StoredEvent[] = [
  { id: "1", timestamp: 0, type: "window_focus", payload: JSON.stringify({ process_name: "Code.exe", window_title: "a", exe_path: null }) },
  { id: "2", timestamp: 86_399_000, type: "system_idle", payload: "{}" },
];

describe("Timeline", () => {
  it("renders one rect per event", () => {
    const { container } = render(<Timeline events={base} dayStartMs={0} onSelect={() => {}} />);
    const rects = container.querySelectorAll("rect");
    expect(rects.length).toBe(2);
  });

  it("does not crash on null window_title payload", () => {
    const ev: StoredEvent = {
      id: "3",
      timestamp: 1000,
      type: "window_focus",
      payload: JSON.stringify({ process_name: "x.exe", window_title: null, exe_path: null }),
    };
    const { container } = render(<Timeline events={[ev]} dayStartMs={0} onSelect={() => {}} />);
    expect(container.querySelectorAll("rect").length).toBe(1);
  });

  it("positions a midnight event at x=0 and end-of-day near right edge", () => {
    const { container } = render(<Timeline events={base} dayStartMs={0} onSelect={() => {}} />);
    const rects = container.querySelectorAll("rect");
    const firstX = rects[0].getAttribute("x");
    const lastX = rects[1].getAttribute("x");
    expect(Number(firstX)).toBeCloseTo(0, 5);
    expect(Number(lastX)).toBeGreaterThan(900); // width 1000
  });
});
```

- [ ] **Step 3: 运行测试确认失败**

Run: `pnpm test src/components/Timeline.test.tsx`
Expected: FAIL（`Timeline` 未定义）。

- [ ] **Step 4: 实现 `src/components/Timeline.tsx`**

```tsx
import type { StoredEvent } from "../types";

interface Props {
  events: StoredEvent[];
  dayStartMs: number;
  width?: number;
  height?: number;
  onSelect: (e: StoredEvent) => void;
}

const COLOR: Record<string, string> = {
  window_focus: "#4c8dff",
  window_title_change: "#7fb0ff",
  system_idle: "#9aa0a6",
  system_resume: "#66bb6a",
  session_lock: "#8e24aa",
  session_unlock: "#ab47bc",
  input_heartbeat: "#26a69a",
};
const DAY_MS = 86_400_000;

export default function Timeline({ events, dayStartMs, width = 1000, height = 48, onSelect }: Props) {
  return (
    <svg width="100%" height={height} viewBox={`0 0 ${width} ${height}`} role="img" aria-label="24h 活动时间线">
      {events.map((e) => {
        const offset = Math.max(0, Math.min(DAY_MS - 1, e.timestamp - dayStartMs));
        const x = (offset / DAY_MS) * width;
        return (
          <rect
            key={e.id}
            x={x}
            y={4}
            width={3}
            height={height - 8}
            fill={COLOR[e.type] ?? "#bdbdbd"}
            onClick={() => onSelect(e)}
          >
            <title>{`${new Date(e.timestamp).toLocaleTimeString()} ${e.type}`}</title>
          </rect>
        );
      })}
    </svg>
  );
}
```

- [ ] **Step 5: 运行测试确认通过**

Run: `pnpm test src/components/Timeline.test.tsx`
Expected: PASS（3 tests）。

- [ ] **Step 6: 实现 `src/components/EventDetail.tsx`**

```tsx
import type { StoredEvent, WindowFocusPayload } from "../types";

export default function EventDetail({ event }: { event: StoredEvent | null }) {
  if (!event) return <p>选中时间线上的事件查看详情。</p>;
  let detail = "";
  if (event.type === "window_focus" || event.type === "window_title_change") {
    const p = JSON.parse(event.payload) as WindowFocusPayload;
    detail = `${p.process_name} — ${p.window_title ?? "(无标题)"}`;
  } else {
    detail = event.type;
  }
  return (
    <div>
      <h2>{new Date(event.timestamp).toLocaleString()}</h2>
      <p>{detail}</p>
    </div>
  );
}
```

- [ ] **Step 7: 更新 `src/App.tsx`**

```tsx
import { useEffect, useMemo, useState } from "react";
import Timeline from "./components/Timeline";
import EventDetail from "./components/EventDetail";
import { getEvents, todayString, type StoredEvent } from "./types";

export default function App() {
  const [date, setDate] = useState(todayString());
  const [events, setEvents] = useState<StoredEvent[]>([]);
  const [selected, setSelected] = useState<StoredEvent | null>(null);
  const [status, setStatus] = useState<"loading" | "ok" | "error">("loading");

  useEffect(() => {
    let cancelled = false;
    setStatus("loading");
    getEvents(date)
      .then((evts) => {
        if (cancelled) return;
        setEvents(evts);
        setStatus("ok");
      })
      .catch(() => {
        if (!cancelled) setStatus("error");
      });
    return () => {
      cancelled = true;
    };
  }, [date]);

  const dayStartMs = useMemo(() => new Date(`${date}T00:00:00`).getTime(), [date]);

  return (
    <main style={{ padding: 16 }}>
      <h1>Time Scope</h1>
      <div>
        <button onClick={() => setDate(shiftDate(date, -1))}>前一天</button>
        <input type="date" value={date} onChange={(e) => setDate(e.target.value)} />
        <button onClick={() => setDate(shiftDate(date, 1))}>后一天</button>
      </div>
      {status === "loading" && <p>加载中…</p>}
      {status === "error" && <p role="alert">加载失败，请确认后端已启动。</p>}
      {status === "ok" && (
        <>
          <Timeline events={events} dayStartMs={dayStartMs} onSelect={setSelected} />
          <EventDetail event={selected} />
        </>
      )}
    </main>
  );
}

function shiftDate(date: string, days: number): string {
  const d = new Date(`${date}T00:00:00`);
  d.setDate(d.getDate() + days);
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const dd = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${dd}`;
}
```

- [ ] **Step 8: 全量前端测试 + build**

Run: `pnpm test && pnpm build`
Expected: App smoke 1 + Timeline 3 全 PASS；`tsc` 无错。

- [ ] **Step 9: 提交**

```bash
git add src/
git commit -m "feat(frontend): render 24h event timeline with date navigation and detail panel"
```

---

## Task 11: 端到端冒烟 + 文档

**Files:**
- Create: `docs/superpowers/plans/2026-10-01-phase1-scaffold-verification.md`（验证清单）
- Modify: `README.md`（创建）

**Interfaces:**
- Consumes: 全部前置 task
- Produces: 端到端可验证的骨架 + 运行说明。

- [ ] **Step 1: 写验证清单文档**

`docs/superpowers/plans/2026-10-01-phase1-scaffold-verification.md`：

```markdown
# Phase 1 骨架 — 端到端验证清单

## 前置
- Rust: `rustc --version` → 1.9x
- Node: `node --version` → v22
- pnpm: `pnpm --version`

## 步骤
1. `pnpm install`
2. `cd src-tauri && cargo test` → 全部 PASS
3. `pnpm tauri dev` → 窗口打开，显示 "Time Scope"
4. 在 dev 窗口打开期间，切换若干应用/浏览器标签
5. 等待 ~10s（BatchWriter 5s 刷盘）
6. 关闭 dev 应用
7. 检查 `%APPDATA%/time-scope/time-scope.db` 存在
8. 重新 `pnpm tauri dev`，查看时间线是否出现色块
9. 停手 5+ 分钟 → 出现 system_idle（灰）；动一下鼠标键盘 → system_resume（绿）
10. 停手 10 分钟+ → 出现 input_heartbeat（青）

## 预期
- 步骤 8 时间线随切换的窗口数出现对应数量蓝/浅蓝色块
- 步骤 9/10 颜色符合 Timeline.tsx 的 COLOR 映射
- 重启后当天数据仍在（DB 持久化生效）

## 已知限制（Phase 1 骨架）
- 无分类着色（全蓝/灰），无分桶粒度切换，无汇总——属后续 engine task
- 窗口标题未脱敏——属 spec §11 后续 task
- 无托盘常驻/自启/单实例——属后续 task
```

- [ ] **Step 2: 创建 `README.md`**

````markdown
# Time Scope

Windows 本地个人时间账本。采集使用活动 → 聚合为可解释的 Activity Segment → 时间线展示。

## 当前状态

Phase 1 骨架：端到端垂直切片（采集 → 事件 → SQLite → 时间线渲染）。

## 运行

```bash
pnpm install
pnpm tauri dev
```

## 测试

```bash
cd src-tauri && cargo test   # Rust 单元测试
pnpm test                    # 前端 vitest
```

## 设计文档

- Phase 1 设计：`docs/superpowers/specs/2026-10-01-time-scope-phase1-design.md`
- 骨架实施计划：`docs/superpowers/plans/2026-10-01-phase1-scaffold.md`
````

- [ ] **Step 3: 手动执行验证清单（步骤 1–10）**

按清单逐条执行并记录结果。至少验证：切换窗口产生蓝色块、5 分钟后 idle 灰色块、10 分钟心跳青色块、重启数据仍在。

- [ ] **Step 4: 提交**

```bash
git add README.md docs/superpowers/plans/2026-10-01-phase1-scaffold-verification.md
git commit -m "docs: add Phase 1 scaffold verification checklist and README"
```

---

## Self-Review

**1. Spec 覆盖：**

| Spec 章节 | 覆盖 task | 状态 |
|---|---|---|
| §3 总体架构（Tauri 单进程 / Event Bus / Batch Writer / 核心原则 1-2） | Task 6, 9 | ✅ |
| §4 代码结构（crate 划分、边界约束） | Task 2, 3, 6, 7（engine 留给后续计划） | ✅ |
| §5.1 线程模型（专用消息泵线程、回调只塞 HWND） | Task 7 | ✅ |
| §5.2 窗口事件（SetWinEventHook、注册参数、进程名获取） | Task 7 | ✅ |
| §5.3 idle 与心跳（GetLastInputInfo、cbSize、300s/10s） | Task 8 | ✅ |
| §5.3 SessionLock/Unlock、睡眠唤醒 | **未覆盖** | ⚠️ 见下方缺口 |
| §5.4 状态变化才产 Event（`was_idle` 去重） | Task 8 | ✅ |
| §6 数据模型（Event / EventType 全 7 种） | Task 2 | ✅ |
| §8 Schema（3 表 + 3 索引 + WAL） | Task 3 | ✅ |
| §8.1 写入策略（5s/100 条批写、DB 路径） | Task 5, 9 | ✅ |
| §8.2 分桶（前端纯函数） | **未覆盖** | ⚠️ 属 engine/UI 后续计划 |
| §9 IPC（commands + events） | Task 9（`get_events`；`get_segments` / `segment-updated` 属 engine 后续） | ✅ |
| §10 前端（时间线） | Task 10 | ✅ |
| §11 隐私（脱敏规则） | **未覆盖** | ⚠️ 属后续计划（本计划已在 Global Constraints 声明） |
| §12 常驻与功耗（托盘/自启/单实例） | **未覆盖** | ⚠️ 属后续计划 |
| §13 测试策略（各层测试方式） | 全部 Rust crate 单测 + storage 内存 SQLite + vitest | ✅ |

**本计划明确不覆盖的 spec 部分**（属 Phase 1 的后续 task，不是遗漏）：

- **SessionLock / SessionUnlock / 睡眠唤醒**（§5.3）。骨架阶段不实现；`EventType` 变体已在 Task 2 预留，
  后续加采集时只需补 `WTSRegisterSessionNotification` / `PowerRegisterSuspendResumeNotification`，
  无需改 schema 或 engine。
- **engine crate**（§7）：segmenter 状态机、TOML 规则分类器、grace period / min_segment_duration。
  骨架阶段不建 crate，避免空壳代码；Task 2 预留了 `type_tag` 和 `Event` 形状，engine 接入时无需改 schema。
- **activities 写入**：Task 3 建了表和索引，但没有写入路径——因为写入者是 engine。
- **窗口标题脱敏**（§11）：骨架阶段原样存储，Global Constraints 已标注。

**2. 占位符扫描：**

`grep -n "TBD\|TODO\|fill in\|implement later\|Similar to Task\|appropriate error handling"` → 无匹配。✅

初稿存在、本轮已修掉的 plan failure：

- Task 1 给了两版 `App.test.tsx` + “执行时改为……” → 已合并为单一版本，testing-library 写进 Step 1 的依赖表。
- Task 2 测试里自建 `COUNTER`/`next_id` 与 `lib.rs` 的重复 + 后续“修正测试”步骤 → 已删掉重复与修正步骤，测试直接用 `Event::new`。
- Task 2 workspace members 指向尚不存在的 `crates/storage`/`crates/collector` → 改为分步加入（Task 3 加 storage，Task 6 加 collector），并说明 `cargo` 会自己报错。
- Task 4 重复定义了 `type_tag` 又让执行时去 core 加 → 已在 Task 2 定义 `type_tag`，Task 4 直接调用。
- Task 5 测试 import 了不存在的 `SystemIdleOrFocus`、调用了不存在的 `conn_for_test()` → 已改正。
- Task 6 workspace 依赖在 Step 1 和 Step 5 写了两遍 → 合并到 Step 1，Step 5 只管 `main.rs`/`lib.rs`。
- Task 8 先实现后写测试（违反 TDD） → 已重排为 features → 测试 → 跑失败 → 实现 → 跑通过。
- Task 9 给了手写 civil-days 近似 + `time` crate 两套 `day_range_ms`，并留 `// TODO 精确实现` → 只保留 `time` crate 版本，offset 作为参数注入以便固定时区测试。
- Task 7 `stop_window_watcher` 用 `PostThreadMessageW(0, ...)` 投给错误线程 → 已改为记录 `GetCurrentThreadId`。
- Task 9 缺“全量测试”步骤 → 已补 Step 6（含 release build 验证）。

**3. 类型一致性：**

| 类型 / 函数 | 定义处 | 使用处 |
|---|---|---|
| `Event`、`EventType`、三个 `*Payload` | Task 2 Step 5 | Task 4, 7, 8, 9 |
| `EventType::type_tag()` | Task 2 Step 5 | Task 2 测试、Task 4 `insert_events` |
| `Event::new(event_type, timestamp) -> Event` | Task 2 Step 5 | Task 4, 7, 8 测试 |
| `StoredEvent { id, timestamp, type_, payload }` | Task 4 Step 4 | Task 5 测试、Task 9 `get_events`、Task 10 `types.ts` |
| `insert_events(&Connection, &[Event])` | Task 4 Step 4 | Task 5 `BatchWriter` |
| `get_events_in_range(&Connection, i64, i64)` | Task 4 Step 4 | Task 5 测试、Task 9 `get_events` |
| `BatchWriter::new(Arc<Connection>, u64, usize)` / `push` / `flush` / `conn` | Task 5 Step 3 | Task 9 `setup`、`consumer.rs` |
| `RawSignal` 全 5 变体 | Task 7 Step 2 | Task 8、Task 9 `consumer.rs` |
| `idle_seconds(u64, u64) -> u32` | Task 8 Step 4 | Task 8 测试 |
| `day_range_ms(&str, UtcOffset) -> Result<(i64,i64), String>` | Task 9 Step 3 | Task 9 Step 5 `get_events` |
| `StoredEvent.type_` ↔ TS `type` | Task 4（`#[serde(rename = "type")]`） | Task 10 `types.ts` |

**4. Review Focus 覆盖：**

| # | 关注点 |  pinning 测试 | 位置 |
|---|---|---|---|
| 1 | 强杀/崩溃丢最后几秒 Event | Task 5 `background_thread_auto_flushes_without_manual_call`（不靠手动 flush 也能落盘）+ Task 11 步骤 6-8（重启后当天数据仍在） | Task 5 / 11 |
| 2 | 跨零点 / 本地时区 | `day_range_ms` 6 个单测（24h 长度、本地午夜具体值、相邻日衔接、偏移平移、闰日、非法日期）+ Task 4 `range_is_half_open_and_sorted` | Task 4 / 9 |
| 3 | 高 DPI / 容器 0 宽 | `Timeline` 用 `viewBox` + `width="100%"`（Task 10），骨架期不测像素级渲染 | Task 10 |
| 4 | 窗口标题 null | `window_title_change_with_null_title_roundtrips`（serde）+ `does not crash on null window_title payload`（渲染）+ `window_info` 对 `GetWindowTextLengthW <= 0` 返回 `None` | Task 2 / 7 / 10 |
| 5 | 进程名大小写 / 同进程多窗口 | Task 4 测试确认 `process_name` 原样存取、不做大写归一；`window_info` 按 HWND 而非进程名取信息 | Task 4 / 7 |

**Plan 内已固化的 Win32 陷阱**（spec §15 风险表的实现层落实）：

- `SetWinEventHook` 必须配消息循环 → Task 7 专用线程 + `GetMessageW` 循环。
- `GetLastInputInfo` 的 `cbSize` → Task 8 显式设置并注释。
- `PostThreadMessageW` 必须投给 watcher 线程 id → Task 7 `WATCHER_TID`。

**已知的实现期不确定性**（不是占位符，是编译器会直接告诉你的事）：

- `windows` crate 0.58 的 `WINEVENT` / `HWND` / `GetLastInputInfo` 具体签名可能与上文略有出入。
  Task 7 Step 6 和 Task 8 Step 7 都写了“编译检查”，按 `cargo build` 的报错调整即可，测试不受影响。
- `pnpm tauri icon` 需要一张 ≥256px 的源图；Task 6 Step 7 给了 PowerShell 生成占位图的命令。

---

## 执行交接

计划已保存到 `docs/superpowers/plans/2026-10-01-phase1-scaffold.md`。请审阅。

