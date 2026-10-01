# Phase 1 骨架 — 端到端人工验证清单

自动化测试覆盖了纯逻辑和 Win32 采集链路，但**界面上的真实交互**只能人工确认。
按顺序走一遍，每步都应看到预期结果。

## 已自动验证（2026-10-01，实现过程中实测）

以下几项在实现时已跑过真实程序并核对，不再需要人工重复：

| 项 | 结果 |
|---|---|
| Rust 全量测试 | **51 passed**（collector 16 / core 6 / storage 21 / app 8） |
| 前端全量测试 | **24 passed**（4 个文件） |
| `pnpm build` | 通过，tsc 无错 |
| 应用能启动、建库于 `%APPDATA%\time-scope\time-scope.db` | 通过 |
| `PRAGMA journal_mode` | `wal` |
| 真的采到数据 | 15 条（12 `window_title_change` + 3 `input_heartbeat`） |
| 采到真实窗口信息 | `WindowsTerminal.exe` / 标题 / 完整 exe 路径 |
| **重启不丢数据**（Review Focus #1） | 杀进程后重启：2 条 → 9 条，旧数据全部保留 |
| `WINEVENT_SKIPOWNPROCESS` 行为 | 已确认：切到 Time Scope 自己不产生 focus 事件（符合预期） |

下面 2–9 步仍需人工在真实桌面上确认（UI 交互与 5 分钟级时间跨度无法自动化）。

## 0. 前置

```bash
node --version      # v22.x
pnpm --version      # 12.x
rustc --version     # 1.9x, x86_64-pc-windows-msvc
pnpm install
```

删除旧数据以保证观察干净（可选）：

```bash
rm -rf "$APPDATA/time-scope"
```

## 1. 测试全绿

```bash
cargo test --manifest-path src-tauri/Cargo.toml --workspace   # 期望 51 passed
pnpm test                                                      # 期望 24 passed
pnpm build                                                     # 期望无 tsc 报错
```

## 2. 启动

```bash
pnpm tauri dev
```

- [ ] 窗口打开，标题 "Time Scope"
- [ ] 页面显示日期选择器、24h 时间条、"点击时间线上的色块查看详情"
- [ ] 刚启动时若库是空的，显示"这一天还没有采集到事件"（正常，不是 bug）
- [ ] 终端出现 `[time-scope] db: C:\Users\<你>\AppData\Roaming\time-scope\time-scope.db`

## 3. 采集窗口切换

- [ ] 切到**别的**程序（浏览器、编辑器等），停 2 秒
- [ ] 切回 Time Scope，时间条上出现新的蓝色/浅蓝色块
- [ ] 点击任意色块，右侧显示"切换到窗口"、进程名、窗口标题

> ⚠️ 验证时务必切到 Time Scope **之外**的窗口。`WINEVENT_SKIPOWNPROCESS` 会过滤
> 本进程自己的窗口，所以切到自己不会产生 focus 事件。

## 4. 输入心跳

- [ ] 保持活跃操作（打字/动鼠标）约 30 秒
- [ ] 出现青色（`input_heartbeat`）色块
- [ ] 点开心跳色块，显示"本窗口内活跃 N 秒"，N 在 0–10 之间

## 5. 空闲检测（耗时较长，可选）

- [ ] 停止一切键鼠操作，等 **5 分钟**以上
- [ ] 出现灰色（`system_idle`）色块
- [ ] 动一下鼠标，出现绿色（`system_resume`）色块
- [ ] 点击空闲色块，显示"开始空闲"

> 空闲阈值是 300 秒（spec §5.3），所以这一步至少要等 5 分钟。
> 期间心排名块仍会继续出现，但"活跃秒数"为 0——这是预期行为。

## 6. 日期切换

- [ ] 点"← 前一天"：时间条清空（昨天没数据）或显示昨天的数据
- [ ] 回到今天，能看到今天采到的事件仍在
- [ ] 手动把日期改成昨天再改回今天，数据一致
- [ ] 今天是默认选中时，"回到今天"按钮不出现

## 7. 持久化（Review Focus #1）

- [ ] 正常关闭应用（关窗口即退出，Phase 1 无托盘）
- [ ] 重新 `pnpm tauri dev`
- [ ] 回到今天，**之前采到的事件还在**（时间条非空）

> 强杀（任务管理器结束进程）允许丢最后几秒 Event（内存队列，spec §8.1 明确接受），
> 但更早的事件必须还在。
>
> ✅ 这一条已在实现时自动验证过（2 条 → 9 条，旧数据全保留）。人工再确认一次是为了
> 覆盖"关窗口正常退出"这条不同路径。

## 8. 数据库落盘

- [ ] 关闭应用后检查 `%APPDATA%\time-scope\`：`time-scope.db`、`-wal`、`-shm` 三个文件存在
- [ ] 三个文件一起复制到别处，用任意 SQLite 工具打开 `events` 表
- [ ] 能看到 `window_focus` / `input_heartbeat` 等行，`payload` 列是 JSON
- [ ] `PRAGMA journal_mode` 返回 `wal`

## 9. 常驻行为

- [ ] 开着应用放置 5 分钟，CPU 占用接近 0（任务管理器观察）
- [ ] 内存占用 < 80 MB（spec §12 的预算）

## 结果记录

| 步骤 | 通过 | 备注 |
|---|---|---|
| 1 测试全绿 | | |
| 2 启动 | | |
| 3 窗口切换 | | |
| 4 输入心跳 | | |
| 5 空闲/恢复 | | |
| 6 日期切换 | | |
| 7 持久化 | | |
| 8 数据库 | | |
| 9 常驻 | | |
