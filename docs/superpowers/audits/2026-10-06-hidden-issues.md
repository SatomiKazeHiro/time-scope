# Time Scope 隐藏问题审计（2026-10-06）

**给接手的 agent 的话**：这份清单里每一条都**不是**靠读代码猜的 —— 要么在真实数据库上量到了数字，要么把触发序列手推到底、并说明了为什么现有测试抓不到。请按 §1 的顺序修，**不要**顺手改 §4 里的东西（那些看起来像 bug 但已经验证过是设计）。

- 审计范围：`src/**`（前端）、`src-tauri/src/**`、`src-tauri/crates/{core,collector,engine,storage}/**`
- 审计时的验证基线（**改动前先自己跑一遍确认是绿的**）：
  - `pnpm test` → 21 文件 / 273 用例全过
  - `cargo test --workspace` → 全过（**必须带 `--workspace`**，不带只跑根包、0 用例、显示绿色 —— STATUS 里记过这个坑）
  - `cargo clippy --workspace --all-targets` → 0 warning
- 实测用的真实库：`%APPDATA%\time-scope\time-scope.db`（67,050 events / 654 activities / 4.7 天 / 24 MB）
- ⚠️ 本机受限沙箱下 `cargo`/`pnpm` 会因为 `src-tauri/target` 写入与 esbuild 子进程管道被拒而失败（`spawn EPERM` / `.cargo-build-lock 拒绝访问`）。跑测试需要把会话切到**完全权限**，这不是代码问题。

---

## 1. 优先级总表

| # | 问题 | 位置 | 严重度 | 现状 |
|---|---|---|---|---|
| A1 | `pending` 顺序被破坏 → `absorb` 复活"错误的那一段" → **产出重叠段** | `engine/src/segmenter.rs` | **P0** | **已在真实库造成 76 对重叠行** |
| A2 | 段归属只用 `start_at` 窗口（不是区间相交）→ 跨日段在第二天消失 + 当天汇总虚高 | `storage/src/activity.rs`、`storage/src/summary.rs` | **P0** | 真实库中已有跨午夜段 |
| A3 | 同步 IPC 命令在主线程跑 + 全程只持一条 `Mutex<Connection>` → 「窗口标题 Top」冻结界面约 20 秒，同时卡住采集落库 | `src-tauri/src/lib.rs`、`storage/src/summary.rs` | **P0** | 实测 4.7 天 = 262 ms，线性外推 1 年 ≈ 20 s |
| A4 | 标题**读**路径没过 `Redactor` → 加了脱敏规则后，旧标题仍明文显示 | `src-tauri/src/titles.rs`、`storage/src/summary.rs` | **P0（隐私）** | 与两处注释/文档承诺矛盾 |
| B1 | `range_pacing` 借道 `get_segments_in_range`：每段一次 evidence 查询 + 无用大 Vec | `storage/src/summary.rs:244` | P1 | 654 段 → 654 次查询、66,800 条证据白读 |
| B2 | `classifier_version` 只由**数量**派生 → 改规则内容版本号不变 | `engine/src/classifier.rs:130` | P1 | 影响将来"重算历史段" |
| B3 | events 表只增不减，无保留期/归档 | 全局 | P1 | 实测 ≈5 MB/天 ≈1.8 GB/年 |
| B4 | DB 打不开直接 `expect` panic（config/rules 都有容错，唯独 DB 没有） | `src-tauri/src/lib.rs:208` | P1 | 磁盘满/文件被占 → 启动即崩 |
| B5 | 重放「删当天 + 写当天」不是同一事务，且 `claim` 早于删除 | `src-tauri/src/day_replay.rs` | P2 | 半途失败 → 那天本进程内永久空白 |
| B6 | 段详情只依赖 `segment.id` → 正在生长的段永远显示旧标题 | `src/components/EventDetail.tsx:19-42` | P1 | 面板内自相矛盾（"N 条事件支撑"在涨） |
| B7 | 切粒度不复位 `selectedBucket` → 选中环消失但提示还在 | `src/App.tsx:31-34` | P2 | — |
| B8 | 时间线色块键盘不可达（`role="img"` 把整棵树对 AT 遮蔽）；热力图日格无可访问名 | `SegmentTimeline.tsx`、`ContributionWall.tsx` | P1（可用性） | — |
| B9 | `hoverIndex` 在"段下标/桶下标"两套语义间复用，且有未加保护的取下标 | `SegmentTimeline.tsx:137,429,450` | P1（防御性） | 只在"鼠标悬停 + 键盘切模式"下可达 |
| B10 | `is_absorption` 用 `(ts - t).abs()` → 时钟回退时会吞掉一整段 | `engine/src/segmenter.rs:310` | P2 | 需系统时钟向后跳 |
| B11 | 段 id 仅由 `start_at` 生成 + `INSERT OR REPLACE` → 同毫秒两次切段会静默吞掉一段 | `engine/src/segmenter.rs:372` | P2（潜在） | 实测 5 天里同毫秒换应用 = 0 次 |
| B12 | `merge_into` 按 `ready` 下标合并，不校验时间相邻 → 可能把空档吞进长段 | `engine/src/segmenter.rs:247-285` | P2（未证实） | 我没能构造出可复现路径 |

---

## 2. P0 —— 逐条

### A1 `pending` 顺序被破坏，`absorb` 弹错段，产出重叠行

**证据**

```rust
// engine/src/segmenter.rs（`reduce` 尾部）
st.pending = still;        // still = 还不够老（时间上更新）的段
...
st.pending.extend(keep);   // keep = 已经够老、但没长邻居可并的**更老**段 → 被追加到末尾
```

于是 `pending` 不再是按时间排序的数组，而下面两处都**假定 `pending.last()` 就是"刚离开的那一段"**：

```rust
// is_absorption
let revisitable = st.pending.last()
    .map(|p| p.application == st.previous_application).unwrap_or(false);
// absorb
let Some(mut prev) = st.pending.pop() else { return; };
prev.end_at = prev.end_at.max(ts);
```

**触发序列**（默认 `min_segment_duration_s=30`、`grace_period_s=60` → `hold = max(min,grace) = 60s`、`give_up = hold*3 = 180s`）：

| t | 事件 | 结果 |
|---|---|---|
| 0 | Code | 开段 A |
| 5 000 | chrome | 关 A（`seg-0`，短 5 s）；开 B |
| 70 000 | Code | 关 B（`seg-5000`，长）；开 C。A 够老但无长邻居 → 进 `keep` → `pending = [B, A]`（**顺序已倒**） |
| 80 000 | chrome | 关 C（`seg-70000`）；开 D。`pending = [B, C, A]`，`last() = A`（12 秒前的老段） |
| 85 000 | Code | `previous_application == Code` ✓、`last()` 也是 Code ✓、`|85000-80000| < 60000` ✓ → **absorb 弹出 A**，把它拉到 85 000 |

结果 `current_segment = [0, 85000] Code`，而 `still` 里的 `seg-5000`、`seg-70000` 稍后照样落库 → **三条互相重叠的行**。正确行为应该是弹出 `seg-70000` 并延到 85 000（那样只有两段、互不重叠）。

**后果**：`SUM(end_at - start_at)` 重复计数、真实发生的 chrome 时间被记成 Code/work。这段 85 s 的序列会算出 160 s。

**真实库证据（已损坏，不是理论）**

```
重叠段对:            76 对（其中 49 对是"一段完全包住另一段"）
重叠总时长:          75.4 分钟 / 84 小时 = 1.49%
按天虚高:            10-02 +6.5min(0.74%) → 10-03 +7.4min(1.02%)
                     → 10-04 +19.3min(1.44%) → 10-05 +23.0min(1.94%) → 10-06 +6.6min
典型现场（同一 app、同秒起、互相包住）:
  15:55:33 → 16:13:12  work     WindowsTerminal.exe
  15:55:44 → 15:57:12  unknown  explorer.exe     ← 起点只差 44 ms
  15:55:44 → 15:56:55  unknown  explorer.exe     ← 被前者整段包住
参与重叠的 app: WindowsTerminal 53 / msedge 40 / explorer 24 / Seer 22
```

**为什么现有测试抓不到（关键）**：`engine/src/tests/segmenter.rs` 里 4 条 absorb 测试**全部**设了 `min_segment_duration_s(0)`，而 `min_ms = 0` 意味着没有任何段算"短段" → `keep` **恒为空** → 破坏顺序的那条路径在整个测试套件里**结构上不可达**。另一条 `a_lone_short_segment_is_held_rather_than_emitted` 只断言单段留在 `pending`，从不再喂一个事件触发 absorb。

**修法**
1. 让 `pending` 恒按 `end_at` 有序（`keep` 回填后排序，或 `keep` 与 `still` 归并排序）。
2. `is_absorption` / `absorb` 不取 `last()`，改取"`end_at` 最大且 `<= ts`、且与当前时间段**相邻**"的那一段；找不到就不吸收。
3. 加回归测试：`min_segment_duration_s > 0`（例如 30）、`grace_period_s = 60`，跑上面那 5 个事件，断言落库段数 = 2 且**任意两段不相交**。

---

### A2 段归属只用 `start_at` 窗口：跨日段在第二天消失，同一天的数字对不上

**证据**（四处同一口径，全部是 `start_at >= start AND start_at < end`）

```rust
// storage/src/activity.rs:95   get_segments_in_range（时间线与全部按段查询都走它）
WHERE start_at >= ?1 AND start_at < ?2
// storage/src/activity.rs:79   delete_segments_for_day
// storage/src/summary.rs:126   range_totals（总时长/圆环/应用 Top）
// storage/src/summary.rs:41    daily_calendar（date(start_at/1000,'unixepoch','localtime')）
```

而 `engine_runtime.rs` 的注释与测试说"归属用**区间相交**…两天都能看到它" —— 那只对**内存里正在生长的那一段**成立。翻它自己的测试：`rt.segments_for_day(vec![], …)` 传的是**空的已落库列表**，所以这条测试永远证明不了落库后的行为。`STATUS.md` §4.1 把"跨零点的段两天都看不到"标成 ✅ 已修，**实际只修了一半**，而且已归档成"勿再回头看"。

**真实库证据**：`seg-1791215866412`，2026-10-05 23:57:46 → 2026-10-06 00:00:22（2.6 min，work）—— 它的 `start_at` 落在 10-05，所以 10-06 的时间线**完全没有它**。

**后果（用这一条举例）**

- 10-05 的汇总/热力图：整段 2.6 min 都算 10-05（含跨过午夜的那 22 秒）
- 10-05 的时间线：只画到 24:00
- 10-06 的汇总：**0**；10-06 的时间线：00:00–00:00:22 缺一块

段越长越明显（关机前 23:50 开段、次日 00:30 收尾那种），而且**同一个数字在三处对不上**，这恰恰是 spec 最在意的"三处口径必须同源"。

**修法**：读取与删除统一改区间相交 `start_at < :end AND end_at > :start`；`daily_calendar` 要么按交集切分到两天，要么明确"按开始日归属"并在 UI 里说明。**注意与 A1 一起改**，A2 的修法会改变重放时的删除范围（顺带修掉"重放某天后旧跨日段残留 → 与新段重叠"这条同源问题）。

---

### A3 同步 IPC 命令在主线程跑，配上"一条连接一把锁"，会冻结整个应用

**证据**

```rust
// src-tauri/src/lib.rs
#[tauri::command]              // ← 没有 async
fn get_top_titles(state: tauri::State<'_, AppState>, from: String, to: String, limit: Option<usize>)
    -> Result<Vec<MergedTitle>, String> { ... }
```

Tauri v2 官方文档（[Calling Rust from the Frontend](https://v2.tauri.app/develop/calling-rust/)）：*"Commands without the async keyword are executed on the main thread unless defined with `#[tauri::command(async)]`."* 五条命令全是同步的。

它执行的 SQL（`storage/src/summary.rs:297-305`）：

```sql
SELECT json_extract(payload,'$.window_title') AS t, COUNT(*) AS n
FROM events
WHERE type IN ('window_focus','window_title_change')
  AND timestamp >= ?1 AND timestamp < ?2
  AND json_extract(payload,'$.window_title') IS NOT NULL
GROUP BY t ORDER BY n DESC
```

`json_extract` 必须回表取 `payload`，**无法用索引覆盖** → 全表扫。而默认范围就是 53 周（`views/SummaryPage.tsx:60-61` 的 `wallWindow(todayString())`，标签写"近一年"）。

**实测**：真实库 4.7 天（67,050 events）跑这条 SQL = **262 ms**；按 14,265 events/天外推，一年 ≈ 520 万行 → **≈ 20 秒**。（`lib.rs:166-169` 的注释自己也估过"一年约 15s"，但没意识到它在主线程上。）

**两层后果**

1. 主线程被占 → WebView2 收不到消息，窗口"未响应"，用户可能直接强杀进程（那会丢队列里的事件）。
2. 命令里 `state.writer.conn()` 拿的是**全局唯一那条连接**（`storage/src/lib.rs:27` 的 `type SharedConn = Arc<Mutex<Connection>>`，writer / engine / 所有命令共用）。这 20 秒里 `BatchWriter` 与 engine 线程**都写不进去**，内存队列一直涨 —— 偏偏是在"最可能被杀"的时刻。

**修法**（按性价比排序）
1. 五条命令加 `async`（或 `#[tauri::command(async)]`）—— 一行改动，先解冻界面。
2. 读操作单开一条**只读连接**（WAL 支持多读者），别和写路径抢同一把锁。
3. 标题统计做预聚合表/增量维护，或至少给 `(type, timestamp)` + 标题哈希建索引。

---

### A4 标题**读**路径没过 `Redactor`：加了脱敏规则后旧标题仍是明文

**证据**

```rust
// src-tauri/src/titles.rs:28-73  —— 直接返回 events.payload 里的原文，
// 只用 title.contains(PLACEHOLDER) 判断"是否已脱敏"，全程没有 Redactor
// storage/src/summary.rs:298    —— Top 标题排行同样把原文 GROUP BY 出来
```

而两处文档/注释都声称已经盖住了：

- `src-tauri/src/day_replay.rs:72-74`：「界面重放时经过这里…界面上就不会再露出旧数据里的敏感片段」
- `engine/src/classifier.rs:191-192`（会写进用户的 `rules.toml`）：「但界面重放时也会脱敏，**所以不会露出来**」

**触发**：任何"先采集、后加 `[[redact]]`"的用户 —— 也就是正常用户，因为默认规则文件里**一条脱敏规则都没有**（`DEFAULT_RULES_TOML` 注释明确"默认不预置任何脱敏规则"）。

**后果**：段详情面板的「窗口标题」列表与汇总页的「标题 Top」仍显示明文客户名/订单号，而用户已经按提示配好正则并以为生效了。重放做的脱敏只影响引擎看到的 context，不影响这两条读路径。

**修法**：`get_segment_titles` / `get_top_titles` 返回前过一遍 `Redactor`（app 层已持有 `Arc<Redactor>`，放进 `AppState` 即可）。顺带把两处注释改成事实。

---

## 3. P1/P2

### B1 `range_pacing` 借道重查询（顺带放大 A3）
`summary.rs:244` 调 `get_segments_in_range`，而它**对每个段都跑一次** evidence 查询（`activity.rs:114-119`）；`range_pacing` 只用 `start_at/end_at/category`（`:247-258`）。实测：654 段 → 654 次查询、66,800 条 evidence 白读进内存。修法：给 `get_segments_in_range` 加 `with_evidence: bool`，或给 pacing 单写一条三列查询。

### B2 `classifier_version` 只由数量派生
`classifier.rs:130`：`format!("rules:{}+redact:{}", rules.len(), redact.len())`。把某条规则 `category` 从 `work` 改成 `study`、或改一条正则的文本（条数不变）→ 版本号一模一样，注释里"改了规则就能被区分开"不成立。修法：用规则内容的哈希。

### B3 events 表没有保留策略
全仓 grep 不到 `VACUUM` / `DELETE FROM events` / 保留期。实测 4.7 天 24 MB（另 5.5 MB WAL）→ **≈5 MB/天 ≈1.8 GB/年**，其中 68% 是 `window_title_change`（45,544/67,050）。叠加 A3 的无索引全表扫 = "越用越卡"是曲线而不是天花板。修法：保留期 + 定期 `VACUUM`，或设置页暴露。

### B4 数据库打不开就 panic
`src-tauri/src/lib.rs:208`：`open_file_shared(...).expect("open db")`。文件被占/磁盘满/WAL 损坏 → 用户看到 Rust panic 而不是可诊断提示。同文件里 config、rules 都做足了容错。修法：失败时弹一个说明性对话框/降级到只读，别 `expect`。

### B5 重放不是原子的，且 `claim` 早于删除
`day_replay.rs:58` 先 `claim`，`:98` 删（一个事务），`:106` 写（另一个事务）。删成功、写失败（`SQLITE_FULL` 等）→ 那天被清空且**本进程内不再重放**，用户看到空白直到重启。修法：storage 提供 `replace_day_segments(conn, range, segs)` 单事务，`claim` 移到成功之后。

### B6 段详情对"正在生长的那一段"永远显示旧标题
`components/EventDetail.tsx:19-20` 用 `evidenceIds` 发请求，但 `:42` 的依赖是 `[key]`（`segment.id`），还配了 `eslint-disable exhaustive-deps` 把警告压掉。段还在长 → `evidenceEventIds` 每 5 秒变长而 `id` 不变 → effect 不重跑，"N 条事件支撑"在涨、标题列表不动。修法：依赖改成 `key + evidenceIds.length`。

### B7 切粒度不复位 `selectedBucket`
`App.tsx:31-34` 只在切**指标**时清 `selectedBucket`；`:187` 的 `intervalMs` 会改变桶数。10 分粒度点中第 130 格后切成 120 分（只剩 12 桶）→ 圈消失，但 `:172` 还写着「已选中 · 再次点击取消」。修法：切粒度时按时间中点重算或直接清空。

### B8 时间线色块键盘不可达
`SegmentTimeline.tsx:246` 的 `<svg role="img">` 把整棵树对辅助技术声明成"一张图"，`:301-338` 的 `<rect onClick>` 没有 `tabIndex`/`role`/键盘处理 → 核心操作「点色块看这段在干什么」**纯键盘完全到不了**。`ContributionWall.tsx:148-166` 的日格是 `<button>`（能 Tab），但只有 `title` 没有 `aria-label`。修法：色块加 `tabIndex={0}` + `role="button"` + `aria-label`（时间/类别/时长）+ Enter 触发；日格补 `aria-label`。

### B9 `hoverIndex` 两套下标语义混用
`SegmentTimeline.tsx:137` 一个 state 同时承载"段下标"（`:325`）与"桶下标"（`:280`），而 `:429`/`:450` 直接 `bucketCells[hoverIndex].blank/.step` 没有存在性判断（`:234-235` 的 `buckets[hoverIndex]` 才是安全写法）。鼠标场景下切模式必然先 `onMouseLeave` 清空，所以只有"鼠标悬停 + 键盘切模式"能触发。修法：`metric` 变化时清 hover + 可选链。

### B10 `is_absorption` 用了 `.abs()`
`segmenter.rs:310`：`(ts - t).abs() < grace_ms` —— 时间戳**回退**也会被判成"在宽限窗口内"，而 `absorb` 里 `prev.end_at = max(prev.end_at, ts)` 不会把被丢弃段的末尾补回来 → 那段区间从账本消失。修法：`ts >= t && ts - t < grace_ms`。

### B11 段 id 只用 `start_at`
`segmenter.rs:372`：`id: format!("seg-{}", open.start_at)`，配合 `activity.rs:34-53` 的 `INSERT OR REPLACE`：同毫秒内两次切段 → 后者静默覆盖前者并删掉它的 evidence，`insert_segments` 仍返回 `Ok`。实测你库里 8,121 个时间戳含多条窗口事件，但**同毫秒内换应用 = 0 次**（5 天），所以是潜在问题而非现患。修法：id 带上应用名或序号；写入前检测同批重复 id。

### B12 `merge_into` 不校验时间相邻
`segmenter.rs:247-248` 按 `ready` 下标挑邻居，`merge_into`（`:281-285`）用 min/max 合并 → 理论上会把两段之间的空档一起吞进长段（空档被记成 work）。**我没能构造出可复现路径**（锁屏/空闲都会在中间插一个 idle 段），但断言缺失是真的。修法：合并前校验 `extra.end_at >= target.start_at && extra.start_at <= target.end_at`；idle 段不并入非 idle 段。

---

## 4. 已确认**不是**问题（别改）

| 事项 | 结论 |
|---|---|
| `INSERT OR REPLACE` + evidence 外键 | 实测同构库（`foreign_keys=ON`）替换父行**不会** FK 报错；`STATUS.md` §4.1 撤回那条 review finding 是对的 |
| `GetWindowTextW` 跨进程会挂住采集线程 | 微软文档写明 by design：只对**自己进程**的无响应窗口才会连带卡住；`window_info` 也没有句柄泄漏（`CloseHandle` 到位） |
| DST 差一小时 | 前端固定 `+86_400_000`、后端固定当日 offset、SQLite 用 `localtime`，三处一致地近似；`STATUS.md` #16 已登记为全项目取舍，用户不在 DST 区 |
| 前端"每 5 秒闪一次加载中" | 不成立：`setStatus("loading")` 在 `App.tsx:38` 的 effect **体内**，该 effect 依赖 `[date]`，只在挂载/切日期时跑 |
| 跨午夜界面"说谎" | 基本不成立：5 秒一次的轮询会触发重渲染，`isToday` 随即变 false、「回到今天」按钮出现 |
| React `key` 重复警告（跑测试时报 `TitleList`） | 是测试 mock 形状不对（`App.integration.test.tsx:62` 让所有 invoke 都返回 segment 数组）；生产路径 `titles.rs:62` 按 title 字符串去重，key 唯一 |
| 死锁 | `get_segments` 里 conn 的 guard 在 `lib.rs:83-86` 的块作用域内就释放了，之后才锁 engine；engine 线程是 engine→conn |
| 迁移原子性 / SUM 的 NULL / 半开区间一致性 / 索引覆盖 | 逐条核过，无问题（唯一例外是 A3 的 `json_extract` 回表，属已知代价） |

---

## 5. 本次已经改掉的：贡献墙（`ContributionWall`）

**改了什么**

1. **选中态不再用 `::after` 伪元素**，改为 `box-shadow` 外环，并且虚线全部搬进 `src/styles/theme.css` 的 `@layer components`（和 `.panel` 同层）：组件因此不再往 DOM 里注入 `<style>`，也不再多一棵不参与布局的伪元素树。
2. **描边用两个局部组合变量**（`--wall-ring` / `--wall-stroke`）串成一条 `box-shadow: var(--wall-ring), var(--wall-stroke)`：这样"哪种底色"和"有没有选中"正交，加一种底色不需要再补一条"底色 × 选中"的笛卡尔积规则。色值仍来自 `--color-line` / `--color-line-strong` / `--color-ink`，**没有新增 token**。
3. **压暗（聚光灯）方案被否掉，原因是算过对比度**：最低档卡在 ordinal 的 2:1 底线上（深色 2.29:1 / 浅色 2.07:1，对轨道 2.07 / 1.90）。实测压到 **α=0.85 就掉到 1.99:1**，α=0.4 只剩 **1.32:1** —— 任何全局压暗都破线。所以只用描边，外环用 `--color-ink`（对面板 14.59:1 / 18.29:1，纯明度表达、不借色相）。
4. **算法优化**：`weekStartOf` 从 `cells.find(...)`（53 列 × 371 格 ≈ 每次渲染两万次比较）改成 **O(1) 下标** `cells[ci * 7]`（网格是列优先铺的，这个等式由 `buildWall` 的 `i = col*7 + row` 保证）。另外把 `gridVars()`、周条内描边、补齐位样式提到 `useMemo`/模块常量，不再每次渲染新建对象。
5. 清掉了组件里一段**自相矛盾的旧注释**（它说"框是跨格的一整块绝对定位矩形"，而实现早已改成逐格判断）。
6. `summary.ts` 的 `scaleStroke` / `uninstalledStroke` 换成 `toneOf(cell): "empty" | "uninstalled" | undefined`：描边既然住在 CSS，TS 侧只负责给出**档名**（组件写成 `data-tone`），档名的语义测试也还在。

**顺带回答"天格子到底多少个"**：槽位**恒为 371**（53 列 × 7 行，DOM 子节点数固定，差额是补齐位）；其中真正可点的「天」按钮 = `364 + 今天在墙里的行号 + 1` = **365（周一）… 371（周日）**。今天是 2026-10-06（周二）→ **366 个**。窗口本身也由它决定：`wallWindow()` 是"52 周前的周一 → 今天"，所以默认范围其实是 365~371 天，不是整整一年。

**怎么验证这次改动**

```bash
pnpm exec tsc --noEmit     # 类型
pnpm test                  # 280 passed（原 273 + 新增 7）
pnpm build                 # 构建产物里应能看到下面四条规则、且没有 data-in-range]::after
```

构建后可在 `dist/assets/index-*.css` 里核对（已核对过）：

```css
.wall-cell{--wall-ring:0 0 0 0 transparent;--wall-stroke:0 0 0 0 transparent;box-shadow:var(--wall-ring),var(--wall-stroke)}
.wall-cell[data-tone=empty]{--wall-stroke:inset 0 0 0 1px var(--color-line-strong)}
.wall-cell[data-tone=uninstalled]{--wall-stroke:inset 0 0 0 1px var(--color-line)}
.wall-cell[data-in-range]{--wall-ring:0 0 0 1.5px var(--color-ink)}
```

新增/改写的测试：

- `src/lib/tokens.test.ts` → 新增 3 条：外环半径必须 = `GAP/2`（写成 `GAP/2` 而不是硬编码，间隙一改就红）、两条 box-shadow 必须可叠加、`.wall-cell` 任何块里都不许出现 `opacity`（把"不压暗"这条设计决定钉成可执行断言）。
- `src/lib/summary.test.ts` → 新增：槽位恒 371 且「天」格子 = `364 + rowOf(today) + 1`（7 种星期全覆盖）、`toneOf` 对"填色 / 补齐位"返回 `undefined`；改写原来那条描边测试为断言档名。
- `src/components/ContributionWall.test.tsx` → 把原来断言 `::after` 的用例改成断言"不再有 `<style>` / 不再有伪元素 / 不在 JS 里拼 box-shadow"，新增 `data-tone` 三态用例与"满窗口 371 槽位 / 366 个天格子"用例；保留"框外不压暗"。

---

## 6. 给修复者的两条环境提醒

1. `cargo test` **必须** `--workspace`（不带只跑根包、0 用例、显示绿色）。
2. 受限沙箱下 `cargo` / `pnpm` 会因 `src-tauri/target` 写入被拒、esbuild 子进程管道被拒而失败，需要**完全权限**跑测试；这与代码无关，别去"修"它。
