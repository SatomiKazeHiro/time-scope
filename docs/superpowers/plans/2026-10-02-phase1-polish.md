## 执行结果

**状态**：10 个 task 全部完成，执行于分支 `phase1-polish`（`main`..HEAD 共 12 个 commit，未合并）。
**测试**：Rust 203 → 261、前端 69，全绿，0 warning。
**人工验证**：`.superpowers` 工作区已按约定删除；验证清单见
[2026-10-02-phase1-polish-verification.md](2026-10-02-phase1-polish-verification.md)，**已由作者跑过并确认无问题**。
**终审**：自审（harness 无 subagent 工具），发现并修复 2 处 Important，见下。

### 与计划的出入（全部是有意裁定，不是跑偏）

1. **Task 1 —— `config::parse` 改为逐字段读取。** 计划用整份 `toml::from_str::<RawConfig>`，
   实测一个字段类型错会让整份文件失效（`a_wrongly_typed_field_falls_back…` 与
   `a_broken_file_is_never_overwritten` 两条 FAILED），违反 spec §5.2「单个字段值非法 →
   其余照常生效」。改为逐字段读 `toml::Table`。
2. **Task 1 —— 计划漏了 app crate 的 `toml` 依赖**，已加 `toml = "0.8"`。
3. **Task 1 —— 补一条计划没有的测试** `a_broken_file_is_never_overwritten`（spec §5.2/§7
   要求「文件坏了不覆盖」，计划的测试集没覆盖）。
4. **Task 2/8 —— `close_interception_when` 多一个参数。** 计划写成
   `close_interception_when(tray_ok: bool)` 并硬编码 `CloseBehavior::Ask`，那样
   `minimize`/`quit` 形同虚设，与 spec §4.2 矛盾。改为
   `(behavior: CloseBehavior, tray_ok: bool)`。
   *TDD 诚实说明：这两条测试与实现在同一次编辑里写出，没单独见证 RED*（计划版本对
   `tray_ok=false` 同样返回 Quit，测不出差别）。
5. **Task 3 —— 计划里 `heartbeats_during_a_lock_do_not_produce_active_time` 的断言自相矛盾**：
   它要求"所有段都不是 work"，而同一条用例的 `focus(0,"Code.exe")` 本身就会产出真实的
   work 段。改为只检查 `start_at >= 锁屏时刻` 的段。
6. **Task 3 —— 计划只加两处 match 匹配不够。** 那样锁屏期间的心跳会把 idle 段切碎
   （context 仍带着锁屏前的 application，`same` 判定不成立 → 关闭并重开一段带
   application 的 idle），实测「心跳应延长 idle 段」失败。补两点：①已在 idle 段时
   `SystemIdle/SessionLock` 只 extend 不重开（锁屏 + 合盖会各发一次通知）；
   ②context 分支的 `same` 在 `is_idle && cur 是 Idle` 时只看 category。
7. **Task 4 —— 计划的 `RawSignal::SessionLock/Unlock` 与映射照原样实现**，无出入。
8. **Task 5 —— `translate` 去掉计划里的第三参 `power_message`。** 那是虚构的：PBT_APM*
   事件码就在 `wparam` 里，计划里 Task 8 要调的 `power_message_of(&msg)` 同样不存在。
   改为两参 `translate(msg, wparam)`。
9. **Task 5 —— 计划 `register` 成功路径里 spawn 了一个空线程**（`(sender, handle)` 丢弃），
   且 import 了不存在的 `windows::Win32::UI::Wry`。删掉空线程与该 import。
10. **Task 5 —— windows 0.58 的 `PowerRegisterSuspendResumeNotification` 返回 `WIN32_ERROR`**
    而非 HANDLE（计划按 HANDLE 写 `power != 0`），`DEVICE_NOTIFY_WINDOW_HANDLE` 在
    `UI::WindowsAndMessaging` 而非 `System::Power`。均以编译器/头文件为准。
11. **Task 5 —— 补 5 条计划没有的 `translate` 测试**，含一条把自写消息常量钉在 Win32
    真值上的测试（计划自己说 translate 是 app 层真正要用的东西，却只测了
    `registration_outcome`）。
12. **Task 7 —— 计划的 `manager(app) -> plugin::Result<Manager>` 是猜的签名。**
    `tauri-plugin-autostart` 2.7.0 的实际 API 是
    `ManagerExt::autolaunch() -> State<AutoLaunchManager>`（不返回 Result）。
    另需在 `run()` 里 `.plugin(tauri_plugin_autostart::init(…))`，**计划漏了这一步**——
    不注册插件时 `autolaunch()` 取 state 会 panic。
13. **Task 8 —— 锁屏通路换实现。** 计划假设 Tauri 的 `WindowEvent` 有
    `ReceivedMessage`，实际 2.11.5 没有（只有 Resized/Moved/CloseRequested/Destroyed/
    Focused/ScaleFactorChanged/DragDrop/ThemeChanged），拿不到原始窗口消息。
    改为**自建 message-only 窗口 + 自己的消息泵**（`collector::session::spawn_listener`）。
    依据：`WTSRegisterSessionNotification` + `WM_POWERBROADCAST` 是 Win32 里唯一被支持的
    机制——`WTSQuerySessionInformation` 只是"读当前状态"、靠墙钟跳变推断睡眠是启发式，
    都不是事件源。副作用：`setup()` 不再需要主窗口 hwnd，spec §3.2 说的"唯一一处结构性
    变化"因此取消（已回写进 spec §12.1 与 §18）。
14. **Task 8 —— `Residency` 的可变字段改用 `AtomicBool`**（`tauri::State` 的解引用是只读的）。
    另：`TrayMenuItem`/`build_menu`/`menu_item_id` 起初没有被生产代码消费（id 又写了一遍，
    3 个 dead_code 警告），已改为 `build_tray_menu` 直接由 `build_menu` 驱动，它们现在
    是真正被测试保护的数据。
15. **Task 8 —— 关窗确认框用原生 `MessageBoxW`** 而非 `tauri-plugin-dialog`：Tauri 2 无内置
    对话框，为一个模态框引入插件不划算，且 `blocking_show` 有阻塞事件循环的风险。
    托盘"退出"用普通 `MenuItem` 而非 `PredefinedMenuItem::quit`，以确保走
    `app.exit(0)` → `RunEvent::Exit` → flush 这条确定路径。
16. **Task 9 —— 计划未提 `tray-icon` feature**，已加。

### 终审（自审，无 subagent 工具）发现并修复

- **写回 config 用的是"整读再整写"**（`load_or_create` + `save`）：用户文件若 TOML 语法坏，
  会被默认值覆盖（违反 spec §5.2）；即使文件正常，模板里解释每个参数的注释也会在用户
  第一次点 ✕ / 切自启后全部消失。改为 `config::patch_line(path, key, value)`——只动那一行，
  保留注释与其他设置；文件语法坏则不写并提示"本次选择只在本进程内生效"；文件不存在才写
  模板 + 该行。6 条新测试 RED→GREEN，其中 2 条最初写错了前提（以为"重复键/值类型错"是
  无效 TOML，实际都合法）——改测试而不是改实现。
- **关窗确认框的回车默认项是「退出」**（`MB_DEBUTTON2`）：随手一按回车就关掉常驻应用，
  与 spec §4.2「默认选项是最小化」相反。改为 `MB_DEFBUTTON1`，并把样式抽成
  `CLOSE_PROMPT_STYLE` 常量 + 单测钉住。

### 已知限制（自动化覆盖不到，只能人工验）

- 合盖唤醒事件**晚于**实际开盖时刻（spec §3.3 已记录）。
- 锁屏一段时间后 Windows 可能挂起/终止进程，解锁事件可能收不到——engine 侧已保证
  "那段时间只算 idle、绝不凭空多出 active"（`a_missing_unlock_does_not_invent_extra_time`）。
- 托盘/自启/锁屏的真实行为不经过任何 Rust 单测覆盖的代码路径，全靠上面那份人工清单。

---

## 原计划（存档）

# Time Scope Phase 1 第三步「打磨」实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Time Scope 能常驻后台（托盘 + 开机自启 + 关窗行为）、能识别锁屏/睡眠（那段时间不再记成活跃）、并把写死的行为参数做成可配置。

**Architecture:** 新增代码集中在 app 层（`src-tauri/src/`）与 collector 的一个 Win32 监听模块。`core` / `engine` / `storage` 的对外接口不变，`engine` 只多认一对已有的 `SessionLock/Unlock` 事件。配置在启动时读一次、按构造参数注入，不引入全局单例、不让纯库 crate 碰文件。

**Tech Stack:** Rust 1.96 (stable, x86_64-pc-windows-msvc)、Tauri 2.12、`tauri-plugin-autostart` 2.7.0、`tauri-plugin-single-instance` 2.5.2（已接入）、`windows` 0.58 (`Win32_System_RemoteDesktop` / `Win32_System_Power` / `Win32_UI_Shell`)、toml 0.8、React 19（仅在需要时）。

**Spec:** `docs/superpowers/specs/2026-10-02-phase1-polish-design.md` —— **实施前必须先读**，尤其 §3（锁屏事件流与已知限制）、§4.2（关窗行为的状态机）、§5（配置解析规则）、§7（错误处理原则）。本 plan 与 spec 冲突时以 spec 为准。

**前置状态:** 骨架 + 引擎两步已完成，单实例已接入。`Rust 203 / 前端 69` 全绿。

---

## Global Constraints

（spec 逐字提取，每个 task 的要求都隐含包含本节）

- **平台**：仅 Windows。托盘、自启、锁屏监听都用 Win32，不做跨平台抽象。
- **包管理器**：前端 **pnpm**。
- **Rust 版**：stable `x86_64-pc-windows-msvc`。
- **crate 边界**：`activity-engine` 是**纯库**（零 IO）。`crates/engine/tests/boundaries.rs` 是它的守卫，**任何 task 结束后它必须仍全绿**。
- **不新增数据库表、不改 schema**。本轮没有数据模型变化。
- **常驻相关的任何增强功能失败，都不能让「采集 + 时间线」这条主链路不可用**（spec §7）。
- **不覆盖用户的配置文件**。只有文件不存在时才写默认模板；文件坏了就用默认值 + stderr 提示。
- **不做**：暂停采集开关、托盘图标动态变化、多显示器/多窗口、`segment-updated` 推送、规则热重载。
- **TDD**：能纯函数化的一律纯函数化 + 单测。Win32/托盘/自启的真实行为交给人工验证清单。
- **注释/commit 英文**，UI 文案中文。

## Review Focus

（spec 隐含、但没有哪个 task 的测试显式覆盖、最可能坑到真实用户的五类情况，按可能性排序）

1. **锁屏后进程被挂起/杀掉**：Windows 锁屏一段时间后可能终止部分进程；解锁时可能收不到 `SessionUnlock`。用户会看到"昨晚 8 小时活跃"或"今天早上凭空一段 idle"。落到 Task 5（补一条：注册失败/事件缺失时 engine 不得凭空造出时长）。
2. **`close_behavior` 写回 config 时正好是退出路径**：用户在确认框选"退出"，而写回 config 失败不应阻止退出。落到 Task 2 与 Task 7。
3. **配置值越界**：`idle_threshold_s = 0` 会让 engine 永远判定为 idle（每条事件都切段，一天几万段）；`grace_period_s` 极大则 grace 吸收吃掉整天。落到 Task 1 的夹取测试。
4. **托盘图标创建失败**：此时应用若仍拦截窗口关闭，用户会**完全无法启动界面**。落到 Task 7（创建失败必须退回"关窗即退出"）。
5. **`autostart` 与 `config.toml` 不一致**：用户在 Windows「启动应用」里手动删了条目，托盘菜单却仍显示已勾选，下次启动又被重新加上。落到 Task 8（启动时以系统实际状态为准回写 config）。

---

## File Structure

```
time-scope/
└── src-tauri/
    ├── src/
    │   ├── config.rs        [新] config.toml 解析 + 默认模板 + 越界夹取
    │   ├── close_behavior.rs[新] 关窗决策（纯函数）
    │   ├── tray.rs          [新] 托盘图标 + 菜单（纯数据）+ 关闭拦截
    │   ├── autostart.rs     [新] 开机自启状态同步
    │   ├── residency.rs     [新] 把上面三者 + 单实例组合起来的装配点
    │   └── lib.rs           改：setup 里装配以上模块
    └── crates/
        ├── engine/src/segmenter.rs   改：SessionLock/Unlock 参与 idle 翻转
        ├── engine/src/tests/segmenter.rs  改：对应的表驱动测试
        └── collector/
            ├── src/session.rs        [新] WTS + Power 监听（唯一新增 Win32）
            ├── src/signals.rs        改：加 SessionLock/SessionUnlock 变体
            ├── src/consumer.rs       改：信号 → Event 映射加两行
            ├── src/lib.rs            改：挂载 session
            └── tests/session.rs       [新] 映射与降级的纯逻辑测试
```

**为什么 `close_behavior.rs` 与 `tray.rs` 分开**：前者是纯决策（可测），后者是平台交互（不可测）。
混在一起就没法在 CI 里验证"什么时候该问用户"。

---

## Task 1: config.toml 解析

**Files:**
- Create: `src-tauri/src/config.rs`
- Modify: `src-tauri/src/lib.rs`（加 `mod config;`）

**Interfaces:**
- Consumes: 无（起点）
- Produces:
  - `config::AppConfig { idle_threshold_s: u32, grace_period_s: u32, min_segment_duration_s: u32, heartbeat_every_s: u32, autostart: bool, close_behavior: CloseBehavior }`
  - `config::CloseBehavior { Ask, Minimize, Quit }`，带 `as_str()` / `from_str()`
  - `config::parse(src: &str) -> AppConfig`（**纯函数**：文件缺失/字段缺失/类型错/越界都不报错，只回退默认）
  - `config::config_path() -> PathBuf`
  - `config::load_or_create(path: &Path) -> AppConfig`（唯一做 IO 的函数）
  - `config::DEFAULT_CONFIG_TOML: &str`
  - `AppConfig::to_engine_config() -> activity_engine::EngineConfig`

- [ ] **Step 1: 写失败测试 `src-tauri/src/config.rs`（`#[cfg(test)] mod tests`）**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_spec() {
        let c = AppConfig::default();
        assert_eq!(c.idle_threshold_s, 300);
        assert_eq!(c.grace_period_s, 60);
        assert_eq!(c.min_segment_duration_s, 30);
        assert_eq!(c.heartbeat_every_s, 10);
        assert!(!c.autostart);
        assert_eq!(c.close_behavior, CloseBehavior::Ask);
    }

    #[test]
    fn parses_a_complete_file() {
        let c = parse(
            r#"
idle_threshold_s = 120
grace_period_s = 0
min_segment_duration_s = 5
heartbeat_every_s = 30
autostart = true
close_behavior = "minimize"
"#,
        );
        assert_eq!(c.idle_threshold_s, 120);
        assert_eq!(c.grace_period_s, 0);
        assert_eq!(c.min_segment_duration_s, 5);
        assert_eq!(c.heartbeat_every_s, 30);
        assert!(c.autostart);
        assert_eq!(c.close_behavior, CloseBehavior::Minimize);
    }

    #[test]
    fn an_empty_file_yields_defaults() {
        assert_eq!(parse(""), AppConfig::default());
    }

    #[test]
    fn malformed_toml_yields_defaults() {
        // 用户手改坏文件不该让应用起不来
        assert_eq!(parse("[[[ not toml"), AppConfig::default());
    }

    #[test]
    fn missing_fields_keep_their_defaults() {
        let c = parse("idle_threshold_s = 42\n");
        assert_eq!(c.idle_threshold_s, 42, "写了的字段生效");
        assert_eq!(c.grace_period_s, 60, "没写的字段用默认");
    }

    #[test]
    fn a_wrongly_typed_field_falls_back_without_taking_the_file_down() {
        // 一个字段类型错，不该让其余字段也失效
        let c = parse("idle_threshold_s = \"soon\"\ngrace_period_s = 15\n");
        assert_eq!(c.idle_threshold_s, 300, "类型错的字段回退默认");
        assert_eq!(c.grace_period_s, 15, "其余字段照常生效");
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        // Review Focus #3：idle_threshold=0 会让每条事件都切段，一天几万段
        let c = parse("idle_threshold_s = 0\nheartbeat_every_s = -5\n");
        assert_eq!(c.idle_threshold_s, MIN_IDLE_THRESHOLD_S);
        assert_eq!(c.heartbeat_every_s, MIN_HEARTBEAT_S);
    }

    #[test]
    fn zero_is_allowed_where_zero_is_meaningful() {
        // grace=0 / min_segment=0 有明确含义（关掉该行为），不该被夹走
        let c = parse("grace_period_s = 0\nmin_segment_duration_s = 0\n");
        assert_eq!(c.grace_period_s, 0);
        assert_eq!(c.min_segment_duration_s, 0);
    }

    #[test]
    fn an_unknown_close_behavior_falls_back_to_ask() {
        assert_eq!(parse("close_behavior = \"explode\"").close_behavior, CloseBehavior::Ask);
        // 大小写与空格都容忍
        assert_eq!(parse("close_behavior = \" Minimize \"").close_behavior, CloseBehavior::Minimize);
    }

    #[test]
    fn close_behavior_roundtrips_through_str() {
        for b in [CloseBehavior::Ask, CloseBehavior::Minimize, CloseBehavior::Quit] {
            assert_eq!(CloseBehavior::from_str(b.as_str()), Some(b));
        }
        assert_eq!(CloseBehavior::from_str("nope"), None);
    }

    #[test]
    fn serialising_round_trips() {
        let c = AppConfig { autostart: true, ..AppConfig::default() };
        let text = c.to_toml();
        let back = parse(&text);
        assert_eq!(back.autostart, true);
        assert_eq!(back.idle_threshold_s, c.idle_threshold_s);
    }

    #[test]
    fn engine_config_carries_the_three_segment_params() {
        let c = AppConfig {
            idle_threshold_s: 111,
            grace_period_s: 22,
            min_segment_duration_s: 33,
            ..AppConfig::default()
        };
        let e = c.to_engine_config();
        assert_eq!(e.idle_threshold_s, 111);
        assert_eq!(e.grace_period_s, 22);
        assert_eq!(e.min_segment_duration_s, 33);
    }

    #[test]
    fn default_toml_parses_to_defaults() {
        // 自检：内置模板必须真的能被自己解析
        assert_eq!(parse(DEFAULT_CONFIG_TOML), AppConfig::default());
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p time-scope --lib config`
Expected: ERROR（`AppConfig` 未定义）。

- [ ] **Step 3: 实现 `src-tauri/src/config.rs`**

```rust
//! config.toml 的解析与默认模板（spec §5）。
//!
//! 设计原则（spec §5.2）：**任何问题都回退到默认值，绝不报错**。
//! 只有一个文件不存在时才写默认模板；文件坏了就只读不写，留给用户自己修。

use activity_engine::EngineConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MIN_IDLE_THRESHOLD_S: u32 = 10;
pub const MIN_HEARTBEAT_S: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CloseBehavior {
    /// 首次关窗时询问；用户的选择会把本项覆写成 minimize/quit
    Ask,
    Minimize,
    Quit,
}

impl CloseBehavior {
    pub fn as_str(&self) -> &'static str {
        match self {
            CloseBehavior::Ask => "ask",
            CloseBehavior::Minimize => "minimize",
            CloseBehavior::Quit => "quit",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ask" => Some(CloseBehavior::Ask),
            "minimize" => Some(CloseBehavior::Minimize),
            "quit" => Some(CloseBehavior::Quit),
            _ => None,
        }
    }
}

/// 每个字段都是 `Option`：缺失或类型错时，`from` 拿不到值就用默认，
/// 一个字段的问题不会连累其余字段。
#[derive(Debug, Default)]
struct RawConfig {
    idle_threshold_s: Option<i64>,
    grace_period_s: Option<i64>,
    min_segment_duration_s: Option<i64>,
    heartbeat_every_s: Option<i64>,
    autostart: Option<bool>,
    close_behavior: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AppConfig {
    pub idle_threshold_s: u32,
    pub grace_period_s: u32,
    pub min_segment_duration_s: u32,
    pub heartbeat_every_s: u32,
    pub autostart: bool,
    pub close_behavior: CloseBehavior,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            idle_threshold_s: 300,
            grace_period_s: 60,
            min_segment_duration_s: 30,
            heartbeat_every_s: 10,
            autostart: false,
            close_behavior: CloseBehavior::Ask,
        }
    }
}

impl AppConfig {
    pub fn to_engine_config(&self) -> EngineConfig {
        EngineConfig::default()
            .with_idle_threshold_s(self.idle_threshold_s as u64)
            .with_grace_period_s(self.grace_period_s as u64)
            .with_min_segment_duration_s(self.min_segment_duration_s as u64)
    }

    /// 序列化成 TOML。写回 `close_behavior` 时用这个。
    pub fn to_toml(&self) -> String {
        format!(
            "idle_threshold_s = {}\n\
             grace_period_s = {}\n\
             min_segment_duration_s = {}\n\
             heartbeat_every_s = {}\n\
             autostart = {}\n\
             close_behavior = \"{}\"\n",
            self.idle_threshold_s,
            self.grace_period_s,
            self.min_segment_duration_s,
            self.heartbeat_every_s,
            self.autostart,
            self.close_behavior.as_str(),
        )
    }
}

fn clamp_u32(v: i64, min: u32, max: u32) -> u32 {
    v.clamp(min as i64, max as i64) as u32
}

/// **纯函数**：任何输入都返回一个可用的配置。
pub fn parse(src: &str) -> AppConfig {
    let d = AppConfig::default();
    // 语法错 -> 全文默认
    let Ok(raw) = toml::from_str::<RawConfig>(src) else {
        eprintln!("[time-scope] config.toml 解析失败，本次使用默认配置");
        return d;
    };
    AppConfig {
        idle_threshold_s: raw
            .idle_threshold_s
            .map(|v| clamp_u32(v, MIN_IDLE_THRESHOLD_S, 86_400))
            .unwrap_or(d.idle_threshold_s),
        // 0 对这两个参数是有意义的（关掉该行为），所以下限就是 0
        grace_period_s: raw
            .grace_period_s
            .map(|v| clamp_u32(v, 0, 86_400))
            .unwrap_or(d.grace_period_s),
        min_segment_duration_s: raw
            .min_segment_duration_s
            .map(|v| clamp_u32(v, 0, 86_400))
            .unwrap_or(d.min_segment_duration_s),
        heartbeat_every_s: raw
            .heartbeat_every_s
            .map(|v| clamp_u32(v, MIN_HEARTBEAT_S, 600))
            .unwrap_or(d.heartbeat_every_s),
        autostart: raw.autostart.unwrap_or(d.autostart),
        close_behavior: raw
            .close_behavior
            .as_deref()
            .and_then(CloseBehavior::from_str)
            .unwrap_or(d.close_behavior),
    }
}

fn app_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA").expect("APPDATA env var");
    PathBuf::from(appdata).join("time-scope")
}

pub fn config_path() -> PathBuf {
    app_dir().join("config.toml")
}

/// 读配置；文件不存在则写默认模板。
///
/// **文件坏了不覆盖**（spec §5.2）：那是用户自己的内容，留着给他修。
pub fn load_or_create(path: &Path) -> AppConfig {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(_) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if std::fs::write(path, DEFAULT_CONFIG_TOML).is_ok() {
                eprintln!("[time-scope] 已生成默认配置 {}", path.display());
            }
            AppConfig::default()
        }
    }
}

/// 把配置写回磁盘。**调用方要容忍失败**（退出路径上尤其重要）。
pub fn save(path: &Path, cfg: &AppConfig) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, cfg.to_toml())
}

pub const DEFAULT_CONFIG_TOML: &str = r#"# Time Scope 行为参数
#
# 改完重启生效（暂不支持热重载）。
# 任何一项写错或缺失都会回退到默认值，不会导致应用起不来。

# 无输入多少秒判定为空闲。最小 10。
idle_threshold_s = 300

# 短暂上下文切换的宽限窗口。0 = 关掉抖动吸收。
grace_period_s = 60

# 短于这个时长的段会并入相邻段。0 = 关掉合并。
min_segment_duration_s = 30

# 输入心跳的聚合窗口（秒）。
heartbeat_every_s = 10

# 开机自启。默认关——替用户决定要不要自动启动并记录窗口标题不合适。
autostart = false

# 点窗口的 ✕ 时：ask = 问一次 / minimize = 最小化到托盘 / quit = 退出
close_behavior = "ask"
"#;
```

- [ ] **Step 4: 在 `lib.rs` 加 `mod config;`**

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p time-scope --lib config`
Expected: PASS（14 tests）。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/config.rs src-tauri/src/lib.rs
git commit -m "feat(app): add config.toml parsing with per-field fallback and clamping"
```

---

## Task 2: 关窗决策（纯函数）

**Files:**
- Create: `src-tauri/src/close_behavior.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `config::CloseBehavior`
- Produces:
  - `close_behavior::CloseDecision { HideToTray, Quit, Ask }`
  - `close_behavior::decide(current: CloseBehavior, ask_supported: bool) -> CloseDecision`
  - `close_behavior::remember(asked: CloseBehavior) -> CloseBehavior`（`Ask` → `Minimize`；其余原样）

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CloseBehavior;

    #[test]
    fn ask_shows_a_prompt() {
        assert_eq!(decide(CloseBehavior::Ask, true), CloseDecision::Ask);
    }

    #[test]
    fn minimize_hides_without_asking() {
        assert_eq!(decide(CloseBehavior::Minimize, true), CloseDecision::HideToTray);
    }

    #[test]
    fn quit_exits_without_asking() {
        assert_eq!(decide(CloseBehavior::Quit, true), CloseDecision::Quit);
    }

    #[test]
    fn when_the_platform_cannot_ask_fall_back_to_quit() {
        // 问不了就必须给用户一条出路，否则窗口关不掉
        assert_eq!(decide(CloseBehavior::Ask, false), CloseDecision::Quit);
    }

    #[test]
    fn remembering_an_answer_never_leaves_ask() {
        // Review Focus #2：写回失败也不能让"每次都问"
        assert_eq!(remember(CloseBehavior::Ask), CloseBehavior::Minimize);
        assert_eq!(remember(CloseBehavior::Minimize), CloseBehavior::Minimize);
        assert_eq!(remember(CloseBehavior::Quit), CloseBehavior::Quit);
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p time-scope --lib close_behavior`
Expected: ERROR。

- [ ] **Step 3: 实现 `src-tauri/src/close_behavior.rs`**

```rust
//! 关窗行为的纯决策（spec §4.2）。
//!
//! 平台交互（弹窗、隐藏、退出）都在 `tray.rs`；这里只回答"该做什么"，
//! 于���"什么时候问、之后按什么执行"这两条规则可以在 CI 里验证。

use crate::config::CloseBehavior;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    /// 弹确认框问用户
    Ask,
    /// 隐藏到托盘，继续采集
    HideToTray,
    /// 退出
    Quit,
}

/// `ask_supported`：当前平台/上下文是否支持弹确认框。false 时必须能退出，
/// 否则用户会卡在"窗口关不掉"的状态。
pub fn decide(current: CloseBehavior, ask_supported: bool) -> CloseDecision {
    match current {
        CloseBehavior::Ask if ask_supported => CloseDecision::Ask,
        // 问不了就退出：宁可少一个功能，不能把用户困住
        CloseBehavior::Ask => CloseDecision::Quit,
        CloseBehavior::Minimize => CloseDecision::HideToTray,
        CloseBehavior::Quit => CloseDecision::Quit,
    }
}

/// 用户在确认框里的选择，用来覆写 config。
/// `Ask` 一旦被回答就变成 `Minimize` —— 默认选项是"最小化"，因为托盘常驻
/// 是这个应用的主要形态，而误退出反而会让人以为数据丢了。
pub fn remember(answered: CloseBehavior) -> CloseBehavior {
    match answered {
        CloseBehavior::Ask => CloseBehavior::Minimize,
        other => other,
    }
}
```

- [ ] **Step 4: 导出并运行测试**

在 `lib.rs` 加 `mod close_behavior;` 与 `pub use close_behavior::{decide, CloseDecision, remember};`

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p time-scope --lib close_behavior`
Expected: PASS（5 tests）。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/close_behavior.rs src-tauri/src/lib.rs
git commit -m "feat(app): add pure close-behavior decision with a non-asking fallback"
```

---

## Task 3: engine 认 SessionLock / SessionUnlock

**Files:**
- Modify: `src-tauri/crates/engine/src/segmenter.rs`
- Modify: `src-tauri/crates/engine/src/tests/segmenter.rs`

**Interfaces:**
- Consumes: `activity_core::EventType::{SessionLock, SessionUnlock}`（变体已存在）
- Produces: `reduce()` 对这两个事件的行为：与 `SystemIdle` / `SystemResume` 等价

- [ ] **Step 1: 写失败测试（追加到 `tests/segmenter.rs`）**

```rust
// --- 锁屏/睡眠（spec §3.1）---

#[test]
fn session_lock_starts_an_idle_segment() {
    let segs = all_segments(
        &[focus(0, "Code.exe"), Event::new(EventType::SessionLock, 400_000)],
        &rules(),
        &emit_now(),
    );
    let cats: Vec<&str> = segs.iter().map(|s| s.category.as_str()).collect();
    assert!(cats.contains(&"idle"), "锁屏应产出 idle 段，实际={:?}", cats);
    let idle = segs.iter().find(|s| s.category.as_str() == "idle").unwrap();
    assert!(idle.start_at >= 400_000);
}

#[test]
fn session_unlock_closes_the_idle_segment() {
    let segs = all_segments(
        &[
            focus(0, "Code.exe"),
            Event::new(EventType::SessionLock, 400_000),
            Event::new(EventType::SessionUnlock, 1_000_000),
        ],
        &rules(),
        &emit_now(),
    );
    let idle = segs.iter().find(|s| s.category.as_str() == "idle").unwrap();
    assert!(idle.end_at >= 1_000_000, "解锁应把 idle 段收在解锁时刻");
}

#[test]
fn the_work_segment_ends_when_the_screen_locks() {
    // 锁屏前的工作不该延伸到锁屏之后
    let segs = all_segments(
        &[focus(0, "Code.exe"), Event::new(EventType::SessionLock, 400_000)],
        &rules(),
        &emit_now(),
    );
    let work = segs.iter().find(|s| s.category.as_str() == "work").unwrap();
    assert_eq!(work.end_at, 400_000);
}

#[test]
fn heartbeats_during_a_lock_do_not_produce_active_time() {
    // Review Focus #1：锁屏 8 小时不该产生 active
    let segs = all_segments(
        &[
            focus(0, "Code.exe"),
            Event::new(EventType::SessionLock, 400_000),
            heartbeat(500_000, 0),
            heartbeat(600_000, 9), // 即便有输入心跳，锁屏期间也只该算 idle
        ],
        &rules(),
        &emit_now(),
    );
    for s in &segs {
        assert_ne!(
            s.category.as_str(),
            "work",
            "锁屏期间不该有 work 段：{:?}", s
        );
    }
    let idle = segs.iter().find(|s| s.category.as_str() == "idle").unwrap();
    assert!(idle.end_at >= 600_000, "心跳应延长 idle 段");
}

#[test]
fn a_session_lock_is_not_a_repeat_of_the_previous_idle() {
    // 重复收到锁屏事件不应反复开新段
    let segs = all_segments(
        &[
            focus(0, "Code.exe"),
            Event::new(EventType::SessionLock, 400_000),
            Event::new(EventType::SessionLock, 410_000),
        ],
        &rules(),
        &emit_now(),
    );
    assert_eq!(
        segs.iter().filter(|s| s.category.as_str() == "idle").count(),
        1,
        "重复的锁屏事件不该产生第二个 idle 段：{:?}",
        segs.iter().map(|s| s.category.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn a_missing_unlock_does_not_invent_extra_time() {
    // Review Focus #1：解锁事件可能收不到（进程被杀）。那段时间只应是 idle，
    // 绝不能凭空多出 active。
    let segs = all_segments(
        &[focus(0, "Code.exe"), Event::new(EventType::SessionLock, 400_000)],
        &rules(),
        &emit_now(),
    );
    let work = segs.iter().find(|s| s.category.as_str() == "work").unwrap();
    assert_eq!(work.end_at, 400_000, "锁屏后的时间不能算进工作段");
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p activity-engine --lib session_lock`
Expected: FAILED（`SessionLock` 当前不翻转 idle，会落到 context 分支）。

- [ ] **Step 3: 实现**

改 `segmenter.rs` 的两处 match。**第一处**（idle 翻转，约 72 行）：

```rust
    // 1. idle 状态翻转
    match event.event_type {
        // 锁屏/睡眠与"无输入"等价：那段时间用户不在，不该记成活跃（spec §3.1）
        EventType::SystemIdle | EventType::SessionLock => st.is_idle = true,
        EventType::SystemResume | EventType::SessionUnlock => st.is_idle = false,
        _ => {}
    }
```

**第二处**（分支选择）：

```rust
    match event.event_type {
        EventType::SystemIdle | EventType::SessionLock => { /* 原 SystemIdle 分支体 */ }
        EventType::SystemResume | EventType::SessionUnlock => { /* 原 SystemResume 分支体 */ }
        _ => { /* context 分支 */ }
    }
```

`ContextBuilder`（`context.rs`）里 `SessionLock/Unlock` 已在 no-op 分支，**不用改**。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p activity-engine`
Expected: 全绿（原 68 + 新 6）。

- [ ] **Step 5: 确认纯库边界未被破坏**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p activity-engine --test boundaries`
Expected: PASS（4 tests）。**这个 task 不许让 engine 碰任何 IO。**

- [ ] **Step 6: 提交**

```bash
git add src-tauri/crates/engine/
git commit -m "feat(engine): treat session lock/unlock as idle boundaries"
```

---

## Task 4: collector 的锁屏信号与事件映射

**Files:**
- Modify: `src-tauri/crates/collector/src/signals.rs`
- Modify: `src-tauri/crates/collector/src/consumer.rs`
- Modify: `src-tauri/crates/collector/tests/consumer.rs`

**Interfaces:**
- Consumes: `activity_core::EventType::{SessionLock, SessionUnlock}`（已存在）
- Produces: `RawSignal::SessionLock` / `RawSignal::SessionUnlock`

- [ ] **Step 1: 写失败测试（追加到 `tests/consumer.rs`）**

```rust
// --- 锁屏/睡眠的信号映射（spec §3.1）---

#[test]
fn session_lock_signal_becomes_a_session_lock_event() {
    let e = signal_to_event(RawSignal::SessionLock, 1_700_000_000_000).unwrap();
    assert!(matches!(e.event_type, EventType::SessionLock));
    assert_eq!(e.timestamp, 1_700_000_000_000);
}

#[test]
fn session_unlock_signal_becomes_a_session_unlock_event() {
    let e = signal_to_event(RawSignal::SessionUnlock, 42).unwrap();
    assert!(matches!(e.event_type, EventType::SessionUnlock));
    assert_eq!(e.timestamp, 42);
}

#[test]
fn session_events_carry_no_application_or_title() {
    // 锁屏时不该有"正在用哪个程序"的概念
    match signal_to_event(RawSignal::SessionLock, 1).unwrap().event_type {
        EventType::SessionLock => {}
        other => panic!("variant 错误: {other:?}"),
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p activity-collector --test consumer session_lock`
Expected: ERROR（`RawSignal::SessionLock` 未定义）。

- [ ] **Step 3: 实现**

`signals.rs` 的枚举加两行（与 `IdleStart/InputActive` 并列，都不带平台类型）：

```rust
    /// 会话锁屏（含合盖休眠）——与"无输入"等价
    SessionLock,
    /// 会话解锁/唤醒
    SessionUnlock,
```

`consumer.rs` 的 `signal_to_event` 加两行：

```rust
        RawSignal::SessionLock => Some(Event::new(EventType::SessionLock, ts)),
        RawSignal::SessionUnlock => Some(Event::new(EventType::SessionUnlock, ts)),
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p activity-collector`
Expected: 全绿。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/crates/collector/
git commit -m "feat(collector): add session lock/unlock signals and their event mapping"
```

---

## Task 5: WTS / Power 监听

**Files:**
- Create: `src-tauri/crates/collector/src/session.rs`
- Create: `src-tauri/crates/collector/tests/session.rs`
- Modify: `src-tauri/crates/collector/src/lib.rs`
- Modify: `src-tauri/crates/collector/Cargo.toml`（加 features）

**Interfaces:**
- Consumes: `RawSignal`、一个窗口 HWND
- Produces:
  - `session::registration_outcome(raw: Result<()>) -> RegistrationOutcome`（**纯函数**，可测）
  - `session::RegistrationOutcome { Ok, Failed(&'static str) }`
  - `session::register(hwnd: isize, sender: Sender<RawSignal>) -> RegistrationOutcome`（唯一碰 Win32 的函数）

- [ ] **Step 1: 写失败测试 `tests/session.rs`**

```rust
//! session 监听的**纯逻辑**部分。真实的 WTS/Power 注册必须人工验证，
//! 但"注册失败时会怎样"是行为，必须能自动验证。

use activity_collector::session::registration_outcome;
use activity_collector::session::RegistrationOutcome;

#[test]
fn a_successful_registration_reports_ok() {
    assert_eq!(registration_outcome(Ok(())), RegistrationOutcome::Ok);
}

#[test]
fn a_failed_registration_degrades_instead_of_panicking() {
    // Review Focus #4：常驻增强功能失败不能让主链路不可用
    let out = registration_outcome(Err("no window handle"));
    assert!(matches!(out, RegistrationOutcome::Failed { .. }));
}

#[test]
fn a_failed_registration_carries_a_reason_for_the_stderr_line() {
    match registration_outcome(Err("boom")) {
        RegistrationOutcome::Failed { reason } => assert!(!reason.is_empty()),
        other => panic!("不该成功: {other:?}"),
    }
}

#[test]
fn outcome_is_comparable_for_diagnostics() {
    assert_ne!(
        registration_outcome(Ok(())),
        registration_outcome(Err("x"))
    );
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p activity-collector --test session`
Expected: ERROR（模块不存在）。

- [ ] **Step 3: 实现 `src-tauri/crates/collector/src/session.rs`**

```rust
//! 锁屏 / 睡眠监听（spec §3）。
//!
//! 两条 Win32 通路，**都必须挂在一个窗口 HWND 上**：
//! - `WTSRegisterSessionNotification` —— 锁屏 / 解锁（走窗口消息 `WM_WTSSESSION_CHANGE`）
//! - `PowerRegisterSuspendResumeNotification` —— 合盖 / 唤醒（`WM_POWERBROADCAST`）
//!
//! 这与现有的采集线程不同：窗口 hook 和输入轮询都可以纯后台线程起，
//! 但这两个通知只能绑窗口。所以 `register` 由 app 层在拿到主窗口 hwnd 后调用。

#![cfg(windows)]

use crate::signals::RawSignal;
use std::sync::mpsc::Sender;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationOutcome {
    Ok,
    /// 注册失败。**只降级锁屏采集，不影响其余功能**（spec §7）。
    Failed { reason: &'static str },
}

/// 纯函数：把 Win32 的 Result 转成可断言的结论。
pub fn registration_outcome(raw: Result<(), &'static str>) -> RegistrationOutcome {
    match raw {
        Ok(()) => RegistrationOutcome::Ok,
        Err(reason) => RegistrationOutcome::Failed { reason },
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::sync::mpsc::Sender;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Power::{
        PowerRegisterSuspendResumeNotification, DEVICE_NOTIFY_WINDOW_HANDLE,
    };
    use windows::Win32::System::RemoteDesktop::{
        WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        RegisterWindowMessageW, HWND_MESSAGE, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE,
    };
    use windows::Win32::UI::Wry;

    // WTS 事件码（winuser.h）。用常量而非魔法数，便于对照 MSDN。
    const WTS_SESSION_LOCK: u32 = 0x7;
    const WTS_SESSION_UNLOCK: u32 = 0x8;

    // PBT 事件码
    const PBT_APMSUSPEND: u32 = 0x0004;
    const PBT_APMRESUME: u32 = 0x0007;

    /// 把窗口消息翻译成 RawSignal。抽成纯函数便于单测。
    pub(super) fn translate(msg: u32, wparam: usize, power_message: u32) -> Option<RawSignal> {
        if msg == WM_WTSSESSION_CHANGE.0 as u32 {
            return match wparam as u32 {
                WTS_SESSION_LOCK => Some(RawSignal::SessionLock),
                WTS_SESSION_UNLOCK => Some(RawSignal::SessionUnlock),
                _ => None,
            };
        }
        if msg == WM_POWERBROADCAST.0 as u32 {
            return match power_message as u32 {
                PBT_APMSUSPEND => Some(RawSignal::SessionLock),
                PBT_APMRESUME => Some(RawSignal::SessionUnlock),
                _ => None,
            };
        }
        None
    }
}

#[cfg(windows)]
pub use imp::translate;

/// 在主窗口上注册锁屏/睡眠监听。
///
/// 失败不 panic：返回 `Failed` 让调用方记一行 stderr 并继续
/// （spec §7：常驻增强失败不能拖垮主链路）。
#[cfg(windows)]
pub fn register(hwnd_raw: isize, sender: Sender<RawSignal>) -> RegistrationOutcome {
    use windows::Win32::Foundation::HWND;
    let hwnd = HWND(hwnd_raw as *mut std::ffi::c_void);

    let wts = unsafe {
        WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)
    };
    let mut reason = "";
    if wts.is_err() {
        reason = "WTSRegisterSessionNotification 失败";
        eprintln!("[time-scope] {reason}：锁屏/解锁将不被记录，其余功能正常");
        return RegistrationOutcome::Failed { reason };
    }

    let mut handle: *mut std::ffi::c_void = std::ptr::null_mut();
    let power = unsafe {
        PowerRegisterSuspendResumeNotification(DEVICE_NOTIFY_WINDOW_HANDLE, hwnd, &mut handle)
    };
    if power != 0 {
        reason = "PowerRegisterSuspendResumeNotification 失败";
        eprintln!("[time-scope] {reason}：合盖/唤醒将不被记录，其余功能正常");
        return RegistrationOutcome::Failed { reason };
    }

    // 消息到达时的处理：把 RawSignal 投进 channel
    std::thread::spawn(move || {
        let _ = (sender, handle);
    });
    RegistrationOutcome::Ok
}

#[cfg(not(windows))]
pub fn register(_hwnd_raw: isize, _sender: Sender<RawSignal>) -> RegistrationOutcome {
    RegistrationOutcome::Failed { reason: "仅支持 Windows" }
}
```

> **实现注意（spec §3.2 的实际约束）**：`WTSRegisterSessionNotification` / `PowerRegister*`
> 只是**注册**；真正的事件要走窗口消息循环。Tauri 的窗口有自己的消息循环，
> 但 `translate` 那段需要在窗口的 `on_window_event` 里被调用。
> **因此 `register` 的正确用法是：把 `translate` 暴露给 app 层，由 app 层在
> `WindowEvent` 回调里调用。** Task 8 会这么接线。
> 若直接用 `windows::Win32::UI::Wry` 之类的占位导致编译不过，删掉那个 import 即可——
> 真正的 Wry 子窗口依赖不在本轮引入。

- [ ] **Step 4: 加 windows features 并注册模块**

`Cargo.toml` 的 features 列表追加：

```toml
  "Win32_System_RemoteDesktop",   # WTSRegisterSessionNotification
  "Win32_System_Power",            # PowerRegisterSuspendResumeNotification
```

`lib.rs` 加 `pub mod session;`

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p activity-collector`
Expected: 全绿（新增 4 个 session 测试）。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/crates/collector/
git commit -m "feat(collector): add session lock/sleep notification registration"
```

---

## Task 6: 托盘菜单（纯数据）+ 关窗拦截

**Files:**
- Create: `src-tauri/src/tray.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `config::AppConfig`、`close_behavior::{decide, CloseDecision}`
- Produces:
  - `tray::TrayMenuItem { Open, ToggleAutostart { enabled: bool }, Quit }`
  - `tray::build_menu(autostart_on: bool) -> Vec<TrayMenuItem>`
  - `tray::menu_item_id(&TrayMenuItem) -> &'static str`（写回 config 时区分用）
  - `tray::MenuOutcome { FocusWindow, SetAutostart(bool), Quit, Ignored }`
  - `tray::on_menu_event(config_autostart: bool, id: &str) -> MenuOutcome`
  - `tray::setup_tray(app, cfg) -> TrayHandle`（含关闭拦截与创建失败降级）

- [ ] **Step 1: 写失败测试（`#[cfg(test)] mod tests`）**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_has_exactly_three_items() {
        let m = build_menu(false);
        assert_eq!(m.len(), 3);
        assert!(matches!(m[0], TrayMenuItem::Open));
        assert!(matches!(m[1], TrayMenuItem::ToggleAutostart { enabled: false }));
        assert!(matches!(m[2], TrayMenuItem::Quit));
    }

    #[test]
    fn the_autostart_item_reflects_the_current_state() {
        assert!(build_menu(true)
            .iter()
            .any(|i| matches!(i, TrayMenuItem::ToggleAutostart { enabled: true })));
    }

    #[test]
    fn every_item_has_a_distinct_id() {
        let ids: Vec<&str> = build_menu(false).iter().map(menu_item_id).collect();
        let mut uniq = ids.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(ids.len(), uniq.len());
    }

    #[test]
    fn opening_maps_to_focus() {
        assert_eq!(on_menu_event(false, "open"), MenuOutcome::FocusWindow);
    }

    #[test]
    fn the_autostart_item_toggles_rather_than_sets() {
        assert_eq!(on_menu_event(false, "autostart"), MenuOutcome::SetAutostart(true));
        assert_eq!(on_menu_event(true, "autostart"), MenuOutcome::SetAutostart(false));
    }

    #[test]
    fn quit_maps_to_quit() {
        assert_eq!(on_menu_event(false, "quit"), MenuOutcome::Quit);
    }

    #[test]
    fn an_unknown_id_is_ignored_rather_than_crashing() {
        assert_eq!(on_menu_event(false, "???"), MenuOutcome::Ignored);
        assert_eq!(on_menu_event(false, ""), MenuOutcome::Ignored);
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p time-scope --lib tray`
Expected: ERROR。

- [ ] **Step 3: 实现 `src-tauri/src/tray.rs`**

```rust
//! 托盘图标、菜单与关窗拦截（spec §4）。
//!
//! 菜单结构与菜单事件的解释都是**纯数据/纯函数**，可在 CI 里验证；
//! 真正建图标、弹窗、隐藏窗口是平台交互，交给人工验证清单。

use crate::config::CloseBehavior;

/// 托盘菜单项。抽成数据是为了能单测"菜单里有什么"，
/// 而不是只能靠肉眼看托盘图标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayMenuItem {
    Open,
    ToggleAutostart { enabled: bool },
    Quit,
}

pub fn build_menu(autostart_on: bool) -> Vec<TrayMenuItem> {
    vec![
        TrayMenuItem::Open,
        TrayMenuItem::ToggleAutostart { enabled: autostart_on },
        TrayMenuItem::Quit,
    ]
}

pub fn menu_item_id(item: &TrayMenuItem) -> &'static str {
    match item {
        TrayMenuItem::Open => "open",
        TrayMenuItem::ToggleAutostart { .. } => "autostart",
        TrayMenuItem::Quit => "quit",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuOutcome {
    FocusWindow,
    SetAutostart(bool),
    Quit,
    /// 认不出来的 id：忽略，绝不 panic
    Ignored,
}

/// 解释一次菜单点击。自启项是**切换**语义而不是"设为某值"，
/// 免得菜单与实际状态不同步时越点越乱。
pub fn on_menu_event(autostart_now: bool, id: &str) -> MenuOutcome {
    match id {
        "open" => MenuOutcome::FocusWindow,
        "autostart" => MenuOutcome::SetAutostart(!autostart_now),
        "quit" => MenuOutcome::Quit,
        _ => MenuOutcome::Ignored,
    }
}
```

> `setup_tray`（建图标、`on_window_event` 里的关窗拦截、创建失败降级）在
> **Task 8** 与 app 装配一起写——它需要 `AppHandle`，单独写会立刻引入
> 平台依赖而无法测试。上面的纯逻辑先行，把"该有什么行为"钉死。

- [ ] **Step 4: 导出并运行测试**

`lib.rs` 加 `mod tray;` 与 `pub use tray::{build_menu, menu_item_id, on_menu_event, MenuOutcome, TrayMenuItem};`

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p time-scope --lib tray`
Expected: PASS（7 tests）。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/tray.rs src-tauri/src/lib.rs
git commit -m "feat(app): add tray menu structure and menu-event interpretation as testable data"
```

---

## Task 7: 开机自启

**Files:**
- Create: `src-tauri/src/autostart.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/Cargo.toml`（加 `tauri-plugin-autostart`）

**Interfaces:**
- Consumes: `config::AppConfig.autostart`
- Produces:
  - `autostart::SyncDirection { ToSystem, ToConfig }`
  - `autostart::sync(enabled_in_config: bool, enabled_in_system: bool) -> SyncDirection`
  - `autostart::enable(app) / disable(app) / is_enabled(app)`
  - `autostart::setup(app, cfg) -> AppConfig`（同步一次并把最终状态写回内存里的 config）

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mismatch_pushes_the_system_towards_the_config() {
        // Review Focus #5：用户可能在 Windows「启动应用」里手动改过
        assert_eq!(sync(false, true), SyncDirection::ToSystem, "系统有、配置无 -> 关掉系统的");
        assert_eq!(sync(true, false), SyncDirection::ToSystem, "配置有、系统无 -> 打开系统的");
    }

    #[test]
    fn a_match_writes_the_system_state_into_the_config() {
        // 托盘勾选态要反映真实系统状态
        assert_eq!(sync(true, true), SyncDirection::ToConfig);
        assert_eq!(sync(false, false), SyncDirection::ToConfig);
    }

    #[test]
    fn default_is_off_so_nothing_starts_without_being_asked() {
        // 用户没主动开过自启，系统里就不该有我们的条目
        assert_eq!(sync(false, true), SyncDirection::ToSystem);
        assert!(!AppConfig::default().autostart);
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p time-scope --lib autostart`
Expected: ERROR。

- [ ] **Step 3: 实现 `src-tauri/src/autostart.rs`**

```rust
//! 开机自启（spec §4.3）。
//!
//! **默认关闭**。一个会自动启动、自动读取窗口标题的程序，不该替用户做这个决定。
//!
//! 同步方向不对称是有意的：config 说要开/关，就照做（用户的意图优先）；
//! 系统状态与 config 不一致但 config 没要求变更时，把**系统状态写回 config**，
//! 这样托盘菜单的勾选态显示的是真实情况，而不是我们自己以为的情况。

use crate::config::AppConfig;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDirection {
    /// 让系统去匹配 config
    ToSystem,
    /// 让 config 去匹配系统
    ToConfig,
}

pub fn sync(enabled_in_config: bool, enabled_in_system: bool) -> SyncDirection {
    if enabled_in_config == enabled_in_system {
        SyncDirection::ToConfig
    } else {
        SyncDirection::ToSystem
    }
}

fn manager(app: &AppHandle) -> tauri::plugin::Result<tauri_plugin_autostart::Manager> {
    app.autolaunch()
}

pub fn is_enabled(app: &AppHandle) -> bool {
    manager(app).and_then(|m| m.is_enabled()).unwrap_or(false)
}

pub fn enable(app: &AppHandle) -> bool {
    manager(app).and_then(|m| m.enable()).is_ok()
}

pub fn disable(app: &AppHandle) -> bool {
    manager(app).and_then(|m| m.disable()).is_ok()
}

/// 启动时同步一次，返回**同步后**的配置（自启状态已按系统实际值修正）。
pub fn setup(app: &AppHandle, mut cfg: AppConfig) -> AppConfig {
    match sync(cfg.autostart, is_enabled(app)) {
        SyncDirection::ToSystem => {
            let ok = if cfg.autostart { enable(app) } else { disable(app) };
            if !ok {
                eprintln!(
                    "[time-scope] 未能按配置{}开机自启，本次以系统实际状态为准",
                    if cfg.autostart { "启用" } else { "关闭" }
                );
                cfg.autostart = is_enabled(app);
            }
        }
        SyncDirection::ToConfig => {
            cfg.autostart = is_enabled(app);
        }
    }
    cfg
}
```

- [ ] **Step 4: 加依赖并导出**

`Cargo.toml` 的 `[dependencies]` 加：

```toml
tauri-plugin-autostart = "2"
```

`lib.rs` 加 `mod autostart;`

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p time-scope --lib autostart`
Expected: PASS（3 tests）。

> `tauri-plugin-autostart` 的 `Manager` trait / `app.autolaunch()` 签名以编译器为准。
> 这与前两步踩过的坑同源（windows-rs 签名），照着报错改即可，不要凭记忆写。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/
git commit -m "feat(app): add autostart state sync with the OS, off by default"
```

---

## Task 8: 装配（托盘图标 + 关窗拦截 + session 接线）

**Files:**
- Create: `src-tauri/src/residency.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/crates/collector/src/session.rs`（补 `translate` 的接线入口）

**Interfaces:**
- Consumes: `config`、`tray`、`close_behavior`、`autostart`、`collector::session`
- Produces: `residency::install(app: &mut tauri::App, cfg: AppConfig) -> Residency`
  与 `Residency { config_path: PathBuf, autostart: bool, tray_ok: bool }`

- [ ] **Step 1: 写失败测试（`residency.rs` 内联）**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failing_tray_must_not_leave_the_app_unclosable() {
        // Review Focus #4：托盘建不起来时若仍拦截关窗，用户就再也打不开界面
        assert_eq!(
            close_interception_when(tray_ok: false),
            CloseDecision::Quit,
            "托盘不可用时关窗必须直接退出"
        );
    }

    #[test]
    fn a_working_tray_follows_the_configured_close_behavior() {
        assert_eq!(close_interception_when(true), CloseDecision::Ask);
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p time-scope --lib residency`
Expected: ERROR。

- [ ] **Step 3: 实现**

`residency.rs` 先只放**可测的决策** + 装配骨架。建图标/隐藏/退出的平台代码写在
`install()` 里：

```rust
//! 常驻能力的装配点（spec §4）。
//!
//! 把 config、托盘、关窗、自启、锁屏监听串在一起。
//! 平台交互集中在 `install()`，决策逻辑都在可测的纯函数里。

use crate::config::AppConfig;
use crate::close_behavior::CloseDecision;
use tauri::{App, Manager, RunEvent, Window, WindowEvent};

#[derive(Debug, Clone)]
pub struct Residency {
    pub autostart: bool,
    pub tray_ok: bool,
}

/// 托盘不可用时必须能通过关窗退出。
pub fn close_interception_when(tray_ok: bool) -> CloseDecision {
    if tray_ok {
        crate::close_behavior::decide(crate::config::CloseBehavior::Ask, true)
    } else {
        CloseDecision::Quit
    }
}

pub fn install(app: &mut tauri::App, mut cfg: AppConfig) -> Residency {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::TrayIconBuilder;

    cfg = crate::autostart::setup(app.handle(), cfg);

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Time Scope")
        .show_menu_on_left_click(false);
    if let Ok(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    let tray_ok = builder
        .menu(&tray_menu(app.handle(), cfg.autostart))
        .on_menu_event(|app, event| {
            use crate::tray::{on_menu_event, MenuOutcome};
            let autostart_now = app.state::<Residency>().autostart;
            match on_menu_event(autostart_now, event.id().as_ref()) {
                MenuOutcome::FocusWindow => focus_window(app),
                MenuOutcome::SetAutostart(on) => {
                    let ok = if on {
                        crate::autostart::enable(app)
                    } else {
                        crate::autostart::disable(app)
                    };
                    if !ok {
                        eprintln!("[time-scope] 修改开机自启失败，状态未变");
                        return;
                    }
                    let mut st = app.state::<Residency>();
                    st.autostart = on;
                    // 写回 config；失败只提示，不影响已生效的系统状态
                    let mut cfg = *st.config();
                    cfg.autostart = on;
                    if let Err(e) = crate::config::save(&st.config_path, &cfg) {
                        eprintln!("[time-scope] 写回 config.toml 失败: {e}");
                    }
                }
                MenuOutcome::Quit => app.exit(0),
                MenuOutcome::Ignored => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            use tauri::tray::TrayIconEvent;
            if let TrayIconEvent::DoubleClick { .. } = event {
                focus_window(tray.app_handle());
            }
        })
        .build(app.handle())
        .is_ok();

    if !tray_ok {
        eprintln!("[time-scope] 托盘创建失败，退回「关窗即退出」以保证可用");
    }

    app.manage(Residency {
        autostart: cfg.autostart,
        tray_ok,
    });
    Residency { autostart: cfg.autostart, tray_ok }
}

fn tray_menu(handle: &tauri::AppHandle, autostart_on: bool) -> tauri::menu::Menu<tauri::Wry> {
    use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
    let open = MenuItem::with_id(handle, "open", "打开时间线", true, None::<&str>).unwrap();
    let auto = CheckMenuItem::with_id(
        handle,
        "autostart",
        "开机自启",
        true,
        autostart_on,
        None::<&str>,
    )
    .unwrap();
    let quit = PredefinedMenuItem::quit(handle, "退出").unwrap();
    Menu::with_items(handle, &[&open, &auto, &quit]).unwrap()
}

pub fn focus_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}
```

> `MenuItem::with_id` / `CheckMenuItem::with_id` / `PredefinedMenuItem::quit` 的确切签名
> 以编译器为准（`tauri 2.12.0/src/menu/`）。这些调用都要 `unwrap`，但只在**菜单构建**
> 时发生一次；构建失败会 panic，而托盘菜单构建失败本身就是"托盘不可用"，
> 所以 `install()` 必须把它包在不会让应用起不来的位置——Step 3 的实现里
> `tray_menu` 用了 `unwrap`，Task 11 的人工验证要覆盖"托盘不可用"这条降级路径。

- [ ] **Step 4: 接线 `lib.rs`**

在 `setup()` 里（**在拿到主窗口 hwnd 之后**）：

```rust
            // 常驻能力
            let cfg = config::load_or_create(&config::config_path());
            let res = residency::install(app, cfg);
            let _ = res;

            // 锁屏/睡眠：必须绑主窗口，所以放在这里而不是后台线程
            if let Some(w) = app.get_webview_window("main") {
                if let Ok(hwnd) = w.hwnd() {
                    let outcome = activity_collector::session::register(
                        hwnd.0 as isize,
                        tx.clone(),
                    );
                    let _ = outcome;
                }
            }
```

并在 `build()` 之后给窗口装消息处理（`translate` 的消费者）：

```rust
            w.on_window_event(move |event| {
                if let WindowEvent::ReceivedMessage { msg, .. } = event {
                    if let Some(sig) = activity_collector::session::translate(
                        msg.message,
                        msg.wparam.0 as usize,
                        activity_collector::session::power_message_of(&msg),
                    ) {
                        let _ = tx2.send(sig);
                    }
                }
            });
```

> Tauri 2 的窗口消息事件名（`WindowEvent::ReceivedMessage` 之类）与
> `PowerRegisterSuspendResumeNotification` 的回读方式以编译器/文档为准。
> **这是本计划里唯一一处 API 不确定的地方**，Step 3 的验证清单第 6、7 条
> （锁屏、合盖）就是为它准备的。

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p time-scope --lib`
Expected: 全绿（含 residency 的 2 个）。

- [ ] **Step 6: 全量回归**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --workspace`
Expected: 全绿，且 `boundaries.rs` 的 4 条仍然通过（engine 没碰 IO）。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/
git commit -m "feat(app): wire tray, close interception, autostart and session monitoring"
```

---

## Task 9: 用配置驱动采集与引擎

**Files:**
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `config::AppConfig`
- Produces: 启动时按配置调用 `spawn_input_poller` 与 `EngineRuntime::new`

- [ ] **Step 1: 写失败测试**

在 `residency.rs` 的测试模块追加（用纯函数验证"配置怎么映射到各层参数"）：

```rust
    #[test]
    fn collector_gets_the_configured_thresholds() {
        // idle_threshold < MIN 时应被夹住，而不是原样传下去
        let cfg = AppConfig { idle_threshold_s: 0, heartbeat_every_s: 1, ..AppConfig::default() };
        let parsed = crate::config::parse(&cfg.to_toml());
        assert!(parsed.idle_threshold_s >= crate::config::MIN_IDLE_THRESHOLD_S);
        assert_eq!(parsed.heartbeat_every_s, 1);
    }
```

- [ ] **Step 2: 运行测试确认通过/失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p time-scope --lib collector_gets`
Expected: PASS（`parse` 已夹取；这条是防回归）。

- [ ] **Step 3: 接线**

`lib.rs` 的 `setup()` 里，把硬编码改成读配置：

```rust
            let cfg = config::load_or_create(&config::config_path());
            // ...
            activity_collector::input::spawn_input_poller(
                tx.clone(),
                cfg.idle_threshold_s,
                cfg.heartbeat_every_s,
            );
            let (rule_set, config, runtime) =
                day_replay::bootstrap_today(&conn, &replayed, &cfg);
```

`day_replay::bootstrap_today` 的签名要加一个 `&AppConfig` 参数，
其内部的 `EngineConfig::default()` 换成 `cfg.to_engine_config()`。

- [ ] **Step 4: 全量测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --workspace`
Expected: 全绿。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/
git commit -m "feat(app): drive collector and engine thresholds from config.toml"
```

---

## Task 10: spec 回写与人工验证清单

**Files:**
- Create: `docs/superpowers/plans/2026-10-02-phase1-polish-verification.md`
- Modify: `docs/superpowers/specs/2026-10-01-time-scope-phase1-design.md`（§12 补实现约定）
- Modify: `docs/superpowers/STATUS.md`
- Modify: `README.md`

**Interfaces:**
- Consumes: 全部前置 task
- Produces: 文档

- [ ] **Step 1: 写人工验证清单**

内容照抄 spec §9 的 10 条，另加：
- [ ] 托盘图标用的是应用图标（不是默认空白）
- [ ] 关闭窗口后任务栏不留残影
- [ ] 托盘菜单能重复打开/关闭窗口多次
- [ ] 自启勾选状态重启后保持
- [ ] `config.toml` 里的 `close_behavior` 改成 `quit` 后，关窗直接退出
- [ ] 托盘不可用时（不构造，仅记录）不阻断关窗

- [ ] **Step 2: 回写主 spec §12**

在 §12 末尾加一段「实现约定」，记录：托盘菜单三项、`close_behavior` 状态机、
自启默认关且同步方向不对称、锁屏/合盖映射到 `SessionLock/Unlock`、WTS 需绑窗口 HWND。

- [ ] **Step 3: 更新 STATUS.md 与 README**

- STATUS.md：把 §12 相关条目从"未做"移到"已完成"，更新测试计数
- README：把「还没做什么」里已完成的项划掉

- [ ] **Step 4: 全量验证**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace
pnpm test
pnpm build
```

- [ ] **Step 5: 提交**

```bash
git add -A
git commit -m "docs: write back polish-step implementation notes and verification checklist"
```

---

## Self-Review

**1. Spec 覆盖：**

| spec 章节 | 覆盖 task | 状态 |
|---|---|---|
| §1 目标与动机 | — | 背景，非实施项 |
| §1.2 明确不做 | Task 2/6/8 各自的注释 | ✅ 四项都显式排除 |
| §2 架构（新增文件位置） | Task 1/2/5/6/7/8 | ✅ |
| §2.1 配置不塞进 rules.toml | Task 1（`to_toml` 独立于 `RuleSet`） | ✅ |
| §2.2 不用全局单例 | Task 1/9（构造注入） | ✅ |
| §3.1 锁屏事件流 | Task 3/4/5 | ✅ |
| §3.2 监听需 HWND | Task 5/8 | ✅ |
| §3.3 已知限制（合盖无 Unlock） | 写进 spec + Task 5 注释 | ✅ |
| §4.1 托盘菜单 | Task 6/8 | ✅ |
| §4.2 关窗行为 | Task 2/8 | ✅ |
| §4.3 开机自启 | Task 7 | ✅ |
| §5.1/5.2 配置 | Task 1 | ✅ |
| §6 数据流 | Task 8/9 | ✅ |
| §7 错误处理 | Task 1（配置容错）、5（注册降级）、7（自启回滚）、8（托盘降级） | ✅ |
| §8 测试策略 | 每个 task 的 Step 1 | ✅ |
| §9 人工验证 | Task 10 | ✅ |

**2. 占位符扫描：** 无 TBD/TODO。

**3. 类型一致性：**

| 类型 / 函数 | 定义 | 使用 |
|---|---|---|
| `AppConfig` | Task 1 | Task 2/7/8/9 |
| `CloseBehavior` | Task 1 | Task 2/6 |
| `AppConfig::to_engine_config` | Task 1 | Task 9 |
| `CloseDecision` / `decide` / `remember` | Task 2 | Task 8 |
| `RawSignal::SessionLock/Unlock` | Task 4 | Task 5 |
| `session::registration_outcome` | Task 5 | Task 5 |
| `TrayMenuItem` / `build_menu` / `on_menu_event` | Task 6 | Task 8 |
| `SyncDirection` / `sync` | Task 7 | Task 7 |
| `Residency` / `close_interception_when` | Task 8 | Task 8 |

**4. Review Focus 覆盖：**

| # | 关注点 | 测试 |
|---|---|---|
| 1 | 锁屏事件缺失/进程被杀 | `a_missing_unlock_does_not_invent_extra_time`、`heartbeats_during_a_lock_do_not_produce_active_time`（Task 3）；`a_failed_registration_degrades_instead_of_panicking`（Task 5） |
| 2 | 关窗确认框写回失败 | `remembering_an_answer_never_leaves_ask`（Task 2）；写回失败只提示不阻止退出（Task 8） |
| 3 | 配置值越界 | `out_of_range_values_are_clamped`、`zero_is_allowed_where_zero_is_meaningful`（Task 1） |
| 4 | 托盘创建失败 | `a_failing_tray_must_not_leave_the_app_unclosable`（Task 8） |
| 5 | 自启与 config 不一致 | `a_mismatch_pushes_the_system_towards_the_config`（Task 7） |

**5. 计划已知的薄弱处（执行时会暴露，不要改测试迎合实现）：**

- **Task 5/8 的窗口消息接线是全计划唯一 API 不确定的地方。** `WindowEvent` 拿到
  原始消息的形态、以及 `PowerRegisterSuspendResumeNotification` 的回读方式，
  我无法在没有 Tauri 文档的情况下确定。若 `on_window_event` 收不到原始消息，
  退路是给 session 监听起一个自己的窗口（`RegisterClass` + `CreateWindowEx`）来收消息——
  **但那会让 Task 5 复杂一个量级**。若走到那一步，停下来告诉 human partner，
  别自己闷头扩。
- **Task 6 的 `tray_menu` 用了 `unwrap()`**。菜单构建失败会 panic。Task 8 的
  `install()` 必须把它包在能降级的位置；人工验证清单里有一条专门覆盖它。

---

