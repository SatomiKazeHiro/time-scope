# Time Scope — 工作进展与遗留问题

**记录时间**：2026-10-01（第二步完成后）
**分支**：`feature/phase1-scaffold`（26 commits，**未合入 `master`**）
**对应计划**：
- [`2026-10-01-phase1-scaffold.md`](superpowers/plans/2026-10-01-phase1-scaffold.md)（骨架，11 task ✓）
- [`2026-10-01-phase1-engine.md`](superpowers/plans/2026-10-01-phase1-engine.md)（引擎，11 task ✓）
**对应设计**：[`2026-10-01-time-scope-phase1-design.md`](../specs/2026-10-01-time-scope-phase1-design.md)

---

## 一、现在处于什么阶段

Phase 1 分三步。**前两步已完成**，第三步未开始。

| 步骤 | 内容 | 状态 |
|---|---|---|
| 1. 骨架 | Tauri+React 脚手架、窗口/空闲采集 → SQLite → 前端按天查原始事件 | ✅ 完成 |
| 2. 引擎 | engine crate（segmenter 状态机 + TOML 规则分类）→ 分类色块 + 粒度切换 + 汇总 | ✅ 完成 |
| 3. 打磨 | 托盘/自启/单实例、标题脱敏、锁屏采集、evidence 展示 | ⬜ 未开始 |

**现在能做的**：采集使用活动 → 按 `rules.toml` 自动分类 → 归并成 ActivitySegment →
24h 分类着色时间线 + 10/30/60/120 粒度切换 + 当日汇总 + 点开看置信度与证据数。

**现在做不到的**：窗口标题脱敏、托盘常驻、锁屏/睡眠采集、`segment-updated` 实时推送。

---

## 二、代码规模与状态

```
Rust  27 文件 / 2745 行      TS  13 文件 / 907 行
测试  160 Rust + 59 前端 = 219 条用例，全绿，0 warning
提交  26 个（master..HEAD），工作区干净
```

```
time-scope/
├── src/                          React 前端
│   ├── components/SegmentTimeline    24h 分类着色时间线（SVG）
│   ├── components/GranularityPicker  10/30/60/120 分钟粒度切换
│   ├── components/DaySummary         当日各类别时长与占比
│   ├── lib/bucket.ts                 分桶 / 汇总 / 时长格式化（纯函数）
│   └── types.ts                      ActivitySegment 的 TS 镜像 + IPC
└── src-tauri/
    ├── src/lib.rs                 Tauri 壳：commands、线程编排
    ├── src/date_range.rs          本地时区按天半开区间
    ├── src/rules.rs               rules.toml 加载 / 首次拷贝 / 容错
    ├── src/engine_runtime.rs      纯逻辑、零 IO
    ├── src/engine_thread.rs       唯一做 IO 的桥
    ├── src/day_replay.rs          按需重放任意一天（补上次崩溃的孤儿事件）
    ├── src/exit_flush.rs          退出时 flush 队列
    └── crates/
        ├── core/                  Event / EventType / 序列化，零 IO
        ├── engine/                reduce() 状态机（纯库，边界有测试守着）
        ├── storage/               版本化迁移、BatchWriter、activities/evidence
        └── collector/             window hook、input idle、consumer（事件总线）
```

依赖方向单向无环：`core ← engine`、`core ← storage ← collector ← app`。

> **`engine` 是纯库**：不碰文件、数据库、网络。`crates/engine/tests/boundaries.rs`
> 会在有人破坏这条时立刻变红（扫 Cargo.toml 依赖 + 扫源码里的 `std::fs`/`println!`）。

---

## 三、已验证 / 未验证

### 自动化已覆盖（可随时重跑）

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace   # 160 passed
pnpm test                                                      # 59 passed
pnpm build                                                     # 无 tsc 报错
```

> ⚠️ `cargo test` **必须带 `--workspace`**。`src-tauri/Cargo.toml` 同时是 workspace 根和应用包，
> 不带的话只跑根包（0 个测试）并显示绿色——实现过程中真的被这个坑过。

重点覆盖：engine 纯库边界、短段合并、幽灵段防护、grace 抖动吸收、u32 tick 回绕、
大小写不敏感规则匹配、rules.toml 容错、重放幂等、半开区间日期运算、分桶与汇总。

### 真实 Windows 上已实测

- 应用启动、生成 `rules.toml`、建库、`user_version=1`
- 采集到真实事件并落库（105 events → activities + evidence）
- 进程名大小写不敏感、路径只比文件名
- 规则文件改坏 → 退回内置默认，不崩溃、不覆盖用户文件
- 重启不丢数据、正常退出时 flush 队列

### 仍需人工确认

`docs/superpowers/plans/2026-10-01-phase1-engine-verification.md` 第 1–8 步未勾：

- [ ] 分类着色（VS Code 蓝 / 浏览器橙 / 微信青 / 未规则浅灰）
- [ ] 粒度切换（10/30/60/120，且不重查后端）
- [ ] 当日汇总
- [ ] 跨天与重放（段不跨天重叠）
- [ ] 规则改坏容错
- [ ] 常驻开销（CPU≈0、内存<80MB）

> 本次自动化用的是 `AppActivate` 切窗口，它每次都激活**同一个**窗口，
> 所以真实运行只覆盖到单一应用。**多类别着色必须人工确认。**

---

## 四、遗留问题清单

### 4.1 代码问题

**无未修项。** 两轮 review 共提出 6 + 3 条，全部修复（`b4931bb` / `676b701` / `40a0d18` /
`19d9233` / `ebadd45` / `37bbbdc`）。

| 问题 | 影响 | 状态 |
|---|---|---|
| 应用启动后心跳堆出 "unknown / 无应用" 幽灵段 | 每次启动多一个噪声段 | ✅ |
| `min_segment_duration` 只延迟落库、从不合并 | spec §7.3 未实现，64ms 噪声段会落库 | ✅ `fold_short_segments` |
| **时间线永不自动刷新** | 常驻应用看着像坏了 | ✅ 5s 轮询 |
| **跨零点的段两天都看不到** | 凌晨查看昨天"缺一块" | ✅ 改为区间相交 |
| **历史日期的孤儿事件永不分段** | 上次崩溃后翻到那天是空白 | ✅ 按需重放 |
| review Minor 5（段 id 碰撞） | — | ⛔ **撤回**，见下 |

**撤回一条 review finding**：我说 `seg-{start_at}` 的 id 可能碰撞、`INSERT OR REPLACE`
会静默覆盖。仔细想下来这不成立 —— grace 吸收是"复活前一段、丢弃新开的那段"，
所以不会有两个活着的段共享起点；重放又会先删当天。能为它写的测试根本触发不了，
所以我把那个测试撤了，而不是留一个永远不会失败的测试。

### 4.2 spec 声明本轮不覆盖

| spec 章节 | 内容 | 现状 |
|---|---|---|
| §5.3 | SessionLock/Unlock、睡眠唤醒采集 | `EventType` 变体已预留，采集未实现 |
| §9 | `segment-updated` 事件推送 | 未实现；改用 5 秒轮询顶上 |
| §11 | 窗口标题脱敏（`[[redact]]`） | **未实现，标题明文落库** |
| §12 | 托盘常驻 / 开机自启 / 单实例 | 未实现，关窗口即退出 |

> ⚠️ **§11 脱敏是隐私相关的**：spec §1 承诺"窗口标题可脱敏"。本地存储无网络请求时勉强可接受，
> 但**任何时候把数据导出/分享出去之前必须先做**。建议在「打磨」里优先。

### 4.3 Schema 变更（未发布期，无需迁移脚本）

`activity_evidence.event_id` 已从 `REFERENCES events(id)` 改为**软引用**。
原因：采集链路是「Event 压进内存队列 5s 后落库」与「engine ingest 后可能立即产段」并行，
段会先于它引用的 Event 落库，外键必然违反。`activity_id` 的外键保留。

改动直接编辑了 SCHEMA_VERSION=1 的迁移（本机无真实用户数据）。任何在此之前建过的库
需要删除重建：`rm -rf "$APPDATA/time-scope"`。

### 4.4 计划文档的偏差（均已修并同步回计划）

- windows-rs 0.58 的 API 与计划差 5 处（`SetWinEventHook` 在 `UI::Accessibility`、
  `WINEVENTPROC` 7 参数、`EVENT_*` 裸 u32…）
- Task 5 的 grace 代理判定是错的 → 改为"复活前一段"（裁定 C）
- 上下文每事件重建 → 改为沿用 `current_context`
- 落库门槛改为 `max(min, grace)`（裁定 D）
- `min_segment_duration` 缺"合并"那一半 → 新增 `fold_short_segments`（裁定 E）
- Task 9 删旧类型导致构建在 Task 10 之前挂掉
- 一个代码块少收尾 fence，导致后续 task 的 brief 全部提取失败

### 4.5 我自己犯的错（值得记住）

- **Task 9（骨架）**：手写测试期望值连错三次，其中"本地午夜比 UTC 早还是晚 8h"是真的符号错误。
  测试抓住了它们，我改的是测试不是代码。
- **Task 6（骨架）**：差点把"0 个测试全绿"当成通过。
- **Task 9（引擎）**：`toml::from_str::<Vec<RawRule>>` 编译不过（TOML 根永远是 table）。
- **Task 4（引擎）**：测试用 `context_of` 的结果去比"已有 context"，永远不可能过。
- **Task 7（引擎）**：实现和测试写在同一文件，跳过了 RED；后来补做。
- **Task 11（引擎）**：边界测试匹配到了注释里的"刻意没有 tauri"，假阳性。

---

## 五、关键决策（ruling 摘要）

完整清单在 git 历史与 `.superpowers/sdd/`（gitignored）。最重要的几条：

| 决策 | 原因 | 代价 |
|---|---|---|
| engine 零 IO、零 Tauri | spec §4 硬要求；可重放的前提 | 边界靠测试守 |
| `EngineRuntime` 也不碰 DB | 状态机可脱离 DB 单测 | IO 全在 app 层 |
| collector 用第二条 channel 转发 Event | collector 不认识 engine | consumer 多一个参数 |
| `ActivityContext` 沿用而非重建 | 否则心跳冲掉 application，每心跳切一段 | 无 |
| grace 吸收要"复活前一段" | "不切段"会留下错误 context | 需要 previous_application |
| 落库门槛 `max(min, grace)` | 吸收只能复活未落库的段 | 修了 spec 默认值 30<60 的隐患 |
| 短段找不到长邻居就压着 | 宁可不并，不可跨时间并错 | 孤立方段延迟最多 3 倍门槛 |
| `SharedConn = Arc<Mutex<Connection>>` | `Connection` 是 `Send` 非 `Sync` | 每次读写多一次锁 |
| `idle_seconds` 用 u32 + `wrapping_sub` | `dwTime` 是 u32，49.7 天回绕 | 用 u64 会"永不 idle" |
| `payload` 只存 `EventType` JSON | id/timestamp/type 各有独立列 | 存全量会有两个真相来源 |
| evidence 用软引用 | 段先于 Event 落库 | 失去引用完整性 |
| CSP 生产严格 + devCsp 放宽 | Vite dev 注入内联 script | 需 tauri-utils 2.10+ |

---

## 六、下一步

### A. Phase 1 第三步「打磨」（需要先写计划）

| 内容 | spec | 优先级 |
|---|---|---|
| 窗口标题脱敏 `[[redact]]` | §11 | **高**（隐私承诺） |
| 托盘常驻 + 关窗隐藏 + 自启 + 单实例 | §12 | 中（常驻应用的基本形态） |
| SessionLock/Unlock + 睡眠唤醒采集 | §5.3 | 低 |
| `segment-updated` 实时推送 | §9 | 低 |
| 规则热重载（spec 提了但没定义行为） | §7.2 | 低 |

### B. 先把人工验证清单跑一遍

`docs/superpowers/plans/2026-10-01-phase1-engine-verification.md`。
特别是第 2 步（分类着色）——**这是第二步唯一的核心验收点，自动化没覆盖到**。

### C. ~~冻结 spec~~ ✅ 已完成（2026-10-01）

spec 已按"以现实为准，但先判定谁错了"的原则同步完毕。逐条审计结果：
**4 条是 spec 自己的问题**（grace 措辞、min/grace 参数矛盾、evidence 外键、ULID 说法），
**2 条是 spec 对但实施漏了**（短段合并、规则匹配语义未定义），
**1 条是 spec 正确而计划写错了**（上下文"维护"）。审计表见 spec §16。

**仍未做**：spec 里的 §11 脱敏、§12 常驻、§5.3 锁屏采集、§9 推送都还是待办，
下一轮「打磨」实施完后需要再同步一次。

---

## 七、给下一个接手的建议

1. **动手前先读** spec 的 §3（架构）、§5（采集层）、§7（引擎）、§9（IPC）、§10（前端）。
2. **`cargo test` 永远带 `--workspace`**。
3. **改 engine 前后跑 `cargo test -p activity-engine --test boundaries`**，
   那是纯库边界的守卫。
4. **验证采集时切到 Time Scope 之外的窗口** —— `WINEVENT_SKIPOWNPROCESS`
   会过滤自己的窗口，切到自己不产生 focus 事件，这是设计不是 bug。
5. **手工查库**要把 `time-scope.db`、`-wal`、`-shm` 三个文件一起复制出来再打开，
   运行中原地打开会报 `SQLITE_CANTOPEN`。
6. **想复现决策过程**（60+ 条 ruling、每个 task 的测试日志），重跑
   `superpowers:executing-plans` 会重建 `.superpowers/sdd/` 目录。
