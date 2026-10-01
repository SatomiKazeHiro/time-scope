# Time Scope Phase 1 第三步「打磨」— 常驻、锁屏采集与参数配置

**日期**：2026-10-02
**状态**：待审阅
**对应 spec**：[`2026-10-01-time-scope-phase1-design.md`](2026-10-01-time-scope-phase1-design.md) §5.3 / §12
**范围**：本轮一次性做完「常驻」+「锁屏/睡眠采集」+「参数配置化」三件事

---

## 1. 目标与动机

骨架和引擎两步已完成，应用已经能采集 → 分类 → 时间线展示。现在它还不是一个**能一直开着**的工具，缺三块：

1. **常驻**（spec §12）。当前关窗口即退出，一天要重开十几次；没有开机自启；没有托盘。
2. **锁屏/睡眠采集**（spec §5.3）。当前锁屏 8 小时会记成 8 小时活跃，合盖午休会留下一大段假 idle。
3. **参数配置化**。`idle_threshold` 等写死在代码里（`src-tauri/src/lib.rs` 的 `spawn_input_poller(tx, 300, 10)`），
   验证一次 idle 要干等 5 分钟，也没法按个人习惯调。

这三件事是**同一类问题**：程序在用户不在场的时候该怎么记账，以及用户怎么控制它。分三轮做会让
"锁屏 8 小时算成活跃"这个明显错误在中间版本里存在一段时间，所以放在一轮做。

### 1.1 成功标准

- 应用最小化到托盘后**继续正常采集**，CPU ≈ 0
- 锁屏期间的时间线只出现 idle 段，不出现 active 段
- 合盖午休产生一段 idle，而不是"断掉"或"继续算活跃"
- 关窗行为符合用户第一次的选择，且只问一次
- 改 `config.toml` 后重启即可生效；文件缺失或损坏时用默认值而不是崩溃

### 1.2 明确不做

- **不做"暂停采集"开关**。`rules.toml` 的 `[[redact]]` 已经是隐私调节阀；
  再加一个"到底在不在记"的开关会让状态变含糊。
- **不做托盘图标的动态变化**（例如"正在录制"徽章）。托盘图标常驻就是足够的可见信号。
  它是本地个人工具，不是屏幕共享工具，不需要那套心理负担。
- **不做多显示器/多窗口模型**。仍是一个主窗口。
- **不做 `segment-updated` 推送**（spec §9）。5 秒轮询够用，那是独立的一项。
- **不设开机自启默认值**。默认关，由用户在托盘菜单里主动开（见 §4.3）。

---

## 2. 架构

新增代码集中在两个地方，**core / engine / storage 三个 crate 的对外接口不变**：

```
src-tauri/
├── src/
│   ├── config.rs        [新] 读 config.toml → 各层参数（纯逻辑 + IO）
│   ├── tray.rs          [新] 托盘图标/菜单/关闭拦截
│   ├── residency.rs     [新] 把 config + tray + 单实例 + 自启组合起来
│   └── lib.rs           改：setup 里拉起以上三者
└── crates/collector/src/
    ├── session.rs       [新] 锁屏/睡眠监听（唯一新增 Win32 代码）
    ├── window.rs        改：暴露主窗口 HWND 给 session.rs 用
    ├── signals.rs       改：加 SessionLock / SessionUnlock 变体
    ├── consumer.rs      改：信号 → Event 的映射表加两行
    └── lib.rs           改：挂载 session 模块
```

**engine 需要一处改动**。现状：`segmenter.rs` 的 `reduce()` 只在 idle 翻转那一步认
`SystemIdle` / `SystemResume`（`src/segmenter.rs:72-75`）；`SessionLock` / `SessionUnlock`
既不翻转 `is_idle`，也不触发切段，最终落到 context 分支被当作一次无意义事件。
本轮让它们参与 idle 状态翻转。`activity_engine` 的其他一切不变。

### 2.1 为什么配置不放进 rules.toml

`rules.toml` 的内容会派生出一个 `classifier_version`（`rules:{n}+redact:{m}`），
写进每条 Activity 用于区分重算前后的结果（spec §7.2）。把 `idle_threshold` 塞进去会导致
**改一个行为参数就改变 classifier_version**，让"哪些段是同一版规则算出来的"这个问题失去意义。
而且两个文件的变更频率、读者都不同：规则是"我的分类偏好了"，配置是"我的行为参数"。

### 2.2 为什么不用全局配置单例

现有构造函数已经是参数注入的形状：

```rust
spawn_input_poller(tx, idle_threshold, heartbeat)                    // collector
EngineRuntime::new(rules, config)  // EngineConfig 已含 idle_threshold_s
```

配置在 app 层读出来、按需传进去即可。引入全局单例会带来隐式依赖，且会让
`crates/engine/tests/boundaries.rs` 那个"engine 是纯库"的守卫形同虚设。

---

## 3. 锁屏与睡眠采集

### 3.1 事件流

```
WTS_SESSION_LOCK / PBT_APMSUSPEND
    → RawSignal::SessionLock
    → EventType::SessionLock
    → engine: is_idle = true，关闭当前段，开一个 Category::Idle 的段

WTS_SESSION_UNLOCK / PBT_APMRESUME
    → RawSignal::SessionUnlock
    → EventType::SessionUnlock
    → engine: 关闭 idle 段，下一个事件开新的 context 段
```

**复用 spec §6 已有的 `EventType::SessionLock/Unlock` 变体**（骨架阶段就预留了）。

**为什么不直接发 `IdleStart`**：那会让重放时无法区分"因为没动鼠标而 idle"和"因为锁屏而 idle"。
用户点开证据链时这个区别有用，而且 spec 本来就定义了这两个变体。

### 3.2 监听方式

- `WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)` —— 锁屏/解锁，走消息
- `PowerRegisterSuspendResumeNotification(DEVICE_NOTIFY_WINDOW_HANDLE, hwnd, &handle)` —— 合盖/唤醒

**两者都需要一个 HWND**，所以监听必须注册在主窗口上，而不是像现在这样纯后台线程起。
这是本轮唯一一处结构性变化：`setup()` 里拿到主窗口的 `hwnd()` 后再启动 session 监听。

### 3.3 已知限制（明确记录，不假装没有）

- **合盖不会发 "Unlock"**。Windows 把 suspend 记成普通睡眠，物理开盖时我们发 Unlock，
  所以唤醒事件会**晚于**实际开盖时刻。中间那段仍是一个正确的 idle 段，只是结束时间偏晚。
  这比现在的"合盖 30 分钟不产生任何事件、时间线断一段"要好。
- `WTSRegisterSessionNotification` 需要用户在**已登录的交互式会话**中才有意义。
  快速用户切换（Fast User Switching）不在本轮范围。

---

## 4. 常驻

### 4.1 托盘

- `TrayIconBuilder` 建一个常驻托盘图标（复用 `icons/32x32.png`）
- 右键菜单（**纯数据，可单测**）：

| 菜单项 | 类型 | 行为 |
|---|---|---|
| 打开时间线 | 普通项 | 显示主窗口 + 置前 |
| 开机自启 | 勾选项 | 勾上则启用自启，取消则停用 |
| 退出 | 普通项 | 显式 flush 队列后退出 |

- 双击托盘图标 = 打开时间线

### 4.2 关闭窗口的行为

由 `config.toml` 的 `close_behavior` 决定：

| 值 | 行为 |
|---|---|
| `ask`（**默认，也是初始状态**） | 第一次关窗时弹确认框「最小化到托盘 / 退出」 |
| `minimize` | 直接隐藏到托盘 |
| `quit` | 直接退出 |

`ask` 只会是**初始状态**：用户在确认框里的选择会把 `close_behavior` 覆写成
`minimize` 或 `quit` 存回 `config.toml`，之后不再询问。
想重新回到"每次都问"的状态，手动把这一行改回 `ask` 即可（随后又会问一次并被覆写）。

**为什么默认 `ask` 而不是直接隐藏**：用户点 ✕ 时，那个窗口其实还在运行、**还在记录窗口标题**。
这是本应用最反直觉的一点，用户有权被告知一次。之后托盘图标本身就是持续的信号，不需要重复确认。

- 「退出」路径必须走显式 flush（`src-tauri/src/exit_flush.rs`，已存在）
- 托盘菜单的「退出」是唯一的退出途径（spec §12）

### 4.3 开机自启

- `tauri-plugin-autostart` 2.7.0
- **默认关闭**。一个会自动启动、自动读取窗口标题的程序，不该替用户做这个决定。
  由用户在托盘菜单里主动开启。
- 状态从 `config.toml` 的 `autostart` 读，启动时同步一次到系统
- 用户也可以在 Windows 的"启动应用"里手动加/删，两边最终一致

---

## 5. 配置

### 5.1 文件

`%APPDATA%/time-scope/config.toml`，与 `rules.toml` 同目录。

**文件缺失时自动创建**（带注释的默认模板），逻辑与 `rules.toml` 一致（见 `src/rules.rs`）。

### 5.2 参数

| 参数 | 默认 | 范围 | 作用 |
|---|---|---|---|
| `idle_threshold_s` | 300 | ≥ 10 | 无输入多少秒算空闲 |
| `grace_period_s` | 60 | ≥ 0 | 短暂上下文切换宽限 |
| `min_segment_duration_s` | 30 | ≥ 0 | 短于此的段并入相邻段 |
| `heartbeat_every_s` | 10 | ≥ 1 | 心跳聚合窗口 |
| `autostart` | `false` | bool | 开机自启 |
| `close_behavior` | `"ask"` | ask/minimize/quit | 关窗行为 |

**解析规则（与 `rules.toml` 一致）**：
- 任何字段缺失或类型不对 → 用默认值，**不报错、不崩溃**
- 数值越界（低于下限）→ 夹到下限
- 整个文件 TOML 语法错误 → 全部用默认值，并在 stderr 说明
- **不覆盖用户的文件**（只有"文件不存在"时才写默认模板）

**不引入热重载**：改配置后重启生效。当前轮询模型下，配置变更需要重建 engine 状态
（`EngineConfig` 是构造进去的），热重载会引入"重放当天"这种副作用，不值当。

---

## 6. 数据流

```
                        ┌─ window watcher 线程（SetWinEventHook + 消息泵）
                        ├─ input poller 线程（GetLastInputInfo，每 1s）
   Win32  ──────────────┤
                        └─ session 监听（主窗口消息：锁屏/解锁/睡眠/唤醒）
                                          │
                                    RawSignal channel
                                          │
                                   consumer 线程
                                          │
                             生成 Event（+ 脱敏）─┬─→ BatchWriter ─→ events 表
                                                    └─→ engine 线程 ─→ activities 表
                                                                    
   用户点 ✕ ──→ tray.rs 按 close_behavior 处理（隐藏 or 退出）
   托盘菜单 ──→ tray.rs（打开 / 切自启 / 退出+flush）
```

`config.toml` 只在启动时被读一次，参数注入 `spawn_input_poller` 与 `EngineRuntime::new`。

---

## 7. 错误处理

| 情况 | 行为 |
|---|---|
| `config.toml` 缺失 | 写默认模板，用默认值 |
| `config.toml` 语法错 | 全用默认值 + stderr 提示，**不覆盖文件** |
| 单个字段值非法 | 该字段用默认值，其余照常生效 |
| `WTSRegisterSessionNotification` 失败 | stderr 提示，锁屏采集降级为不可用，**其余功能不受影响**（不 panic） |
| `PowerRegisterSuspendResumeNotification` 失败 | 同上 |
| 托盘创建失败 | 退回"关窗即退出"并 stderr 提示，保证应用仍可用 |
| 自启启用失败 | 托盘菜单勾选态回滚 + 提示 |

**原则**：常驻相关的任何增强功能失败，都不能让"采集 + 时间线"这条主链路不可用。

---

## 8. 测试策略

沿用前两步的约定：能纯函数化的绝不留给手动验证。

| 层 | 测什么 | 怎么测 |
|---|---|---|
| `config.rs` | 默认值 / 解析 / 越界夹取 / 单字段坏 / 整文件坏 / 不覆盖用户文件 | 纯函数单测（`#[cfg(test)]` 落在文件内） |
| `close_behavior` 决策 | 首次是否询问、后续是否直接执行 | 纯函数单测 |
| 托盘菜单 | 菜单项结构与勾选态 | 抽成纯数据（`Vec<MenuItem>`）后单测 |
| 锁屏映射 | `RawSignal::SessionLock → EventType::SessionLock` | 纯映射表单测 |
| **engine 处理 SessionLock** | 锁屏产出 idle 段；解锁关闭它；锁屏期间的心跳不计入活跃 | 扩展 `segmenter.rs` 表驱动测试 |
| `WTS` / `Power` 注册 | 失败时不 panic、其余链路正常 | 抽出"注册失败 → 降级"的决策为纯函数后单测 |
| 真实托盘 / 自启 / 锁屏 | 托盘图标出现、关窗隐藏、自启生效、锁屏 5 分钟看时间线 | **人工**，见 §9 |

**边界守卫保持**：本轮不碰 engine 的 IO，所以 `crates/engine/tests/boundaries.rs` 必须仍然全绿。

---

## 9. 人工验证清单

自动化测不到的部分，逐条勾选：

- [ ] 托盘图标出现在系统托盘
- [ ] 点 ✕ 首次弹出确认框；选"最小化到托盘"后窗口消失、数据继续增长
- [ ] 再次启动应用：老窗口被叫醒置前，新进程不残留
- [ ] 托盘菜单能打开窗口、能切自启、能退出（退出前数据已落盘）
- [ ] 勾上自启 → 重启系统 → 应用自动启动
- [ ] 锁屏后等 1 分钟再解锁：时间线出现一段 idle，没有 active 段
- [ ] 合盖午休 30 分钟后开盖：出现一段 idle
- [ ] 改 `config.toml` 里 `idle_threshold_s = 10` → 重启 → 停手 15 秒就出现 idle
- [ ] 把 `config.toml` 写成乱字符 → 重启仍正常（用默认值）
- [ ] 关机 → 重开 → 托盘图标与配置仍在

---

## 10. 与已有 spec 的关系

本文件是 `2026-10-01-time-scope-phase1-design.md` §5.3 / §12 的**实施细化**，不取代它。
实施完成后应把本文件的实现约定回写到主 spec（沿用 §16 "实施后修订记录" 的做法）。

主 spec 中本轮**未**覆盖、仍待后续的：§9 的 `segment-updated` 推送、§7.2 的规则热重载。
