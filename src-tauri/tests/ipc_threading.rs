//! spec §3 的线程模型：IPC 命令不许跑在主线程（审计 §2 A3）。
//!
//! Tauri v2 官方文档（Calling Rust from the Frontend）写明：命令没有 `async`
//! 关键字时默认跑在主线程，除非标成 `#[tauri::command(async)]`。
//!
//! 而 `get_top_titles` 要跑的那条 SQL（`json_extract(payload,'$.window_title')`
//! 加 `GROUP BY`）必须回表取 payload、无法用索引覆盖，只能全表扫。真实库实测
//! 那 4.7 天的数据就要 262ms，按每天 14,265 个事件外推，一年 ≈ 20 秒。
//! 这 20 秒占住主线程，WebView2 收不到消息，窗口显示"未响应"——用户十有八九
//! 直接强杀进程，而那一刻内存队列里正堆着还没落库的事件。
//!
//! 这条测试扫源码而不是跑界面：命令的行为（跑在哪个线程上）只有把整个
//! Tauri 应用拉起来才观察得到，而那需要真实窗口。源码扫描是这里能拿到的
//! 最强保证，形态与 `crates/engine/tests/boundaries.rs` 一致。

use std::path::PathBuf;

/// 至少要有这么多条命令，否则这个检查可能因为"文件被清空"而空转。
const MIN_COMMANDS: usize = 5;

#[test]
fn every_ipc_command_is_marked_async() {
    let lib_rs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
    let src = std::fs::read_to_string(&lib_rs).expect("read src/lib.rs");

    let mut checked = 0usize;
    let mut bare = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let t = line.trim();
        // 只认真正的属性行，不认注释或字符串里出现的同名字样
        if !t.starts_with("#[tauri::command") {
            continue;
        }
        checked += 1;
        if t != "#[tauri::command(async)]" {
            bare.push(format!("{}:{}  {}", lib_rs.display(), i + 1, t));
        }
    }

    assert!(
        checked >= MIN_COMMANDS,
        "只扫到 {checked} 条 #[tauri::command]，少于预期的 {MIN_COMMANDS} —— \
         这条测试可能已经空转（lib.rs 被改名或命令换了写法？）"
    );
    assert!(
        bare.is_empty(),
        "这些 IPC 命令没标 async，会跑在主线程上：\n{}\n\
         加 `async`：`#[tauri::command(async)]`（审计 §2 A3）",
        bare.join("\n")
    );
}