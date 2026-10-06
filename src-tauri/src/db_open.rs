//! 打不开数据库时的诊断（审计 §3 B4）。
//!
//! 同一个 `setup` 里，`config.toml` 坏了退回默认配置、`rules.toml` 坏了退回
//! 内置默认规则，两处都做足了容错，还会在终端打一行说明。唯独数据库是
//! `open_file_shared(...).expect("open db")` —— 文件被别的进程占着、磁盘满、
//! WAL 损坏，用户看到的是一句
//! `thread 'main' panicked at src/lib.rs:224:39: open db`：
//! 没有路径、没有原因、更不知道下一步该干什么。
//!
//! 这里把它改成：把错误整理成一句**带路径、带原因、带下一步**的说明，
//! 由 app 层弹给用户，然后让 `setup` 返回 `Err` —— Tauri 会干净地退出，
//! 而不是 panic。
//!
//! 纯逻辑部分（`explain` / `next_step`）不碰文件系统，便于单测。

use activity_storage::SharedConn;
use std::path::Path;

/// 数据库打不开时给用户看的说明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbOpenError {
    pub path: String,
    /// 底层原因（rusqlite / io 的原始描述）
    pub reason: String,
    /// 用户下一步该做什么
    pub next_step: String,
}

impl std::error::Error for DbOpenError {}

impl std::fmt::Display for DbOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Time Scope 打不开数据库。\n\n位置：{}\n原因：{}\n\n下一步：{}",
            self.path, self.reason, self.next_step
        )
    }
}

/// 打开数据库，失败时给出可诊断的错误而不是 panic。
pub fn open_db(path: &Path) -> Result<SharedConn, DbOpenError> {
    activity_storage::open_file_shared(path).map_err(|e| explain(path, &e))
}

/// 把说明弹成一个模态框。**必须有** —— 托盘常驻的应用没有终端，
/// 只 `eprintln!` 的话用户根本看不到，等于什么都没说。
///
/// 用原生 MessageBoxW 而不是 dialog 插件：为一个错误提示引入插件不划算，
/// 与关窗确认框同一个理由（见 `close_behavior` 的注释）。
#[cfg(windows)]
pub fn show_error_dialog(err: &DbOpenError) {
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR};
    use windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE;
    use windows::core::PCWSTR;
    let message: Vec<u16> = err.to_string().encode_utf16().chain(std::iter::once(0)).collect();
    let title: Vec<u16> = "Time Scope 无法启动"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let style = MESSAGEBOX_STYLE(MB_ICONERROR.0);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            PCWSTR(title.as_ptr()),
            style,
        );
    }
}

#[cfg(not(windows))]
pub fn show_error_dialog(_err: &DbOpenError) {}

/// 把底层错误翻译成一句用户看得懂的话。
pub fn explain(path: &Path, err: &rusqlite::Error) -> DbOpenError {
    DbOpenError {
        path: path.display().to_string(),
        reason: describe(err),
        next_step: next_step(path, err).to_string(),
    }
}

/// 底层原因的短描述。用户看到的"为什么"。
fn describe(err: &rusqlite::Error) -> String {
    match err {
        rusqlite::Error::SqliteFailure(e, msg) => match msg {
            Some(m) => format!("{m}（SQLite extended code {}）", e.extended_code),
            None => format!("SQLite 错误：code {:?} / extended code {}", e.code, e.extended_code),
        },
        // io 错误在 rusqlite 里被包成 ToSqlConversionFailure，原始原因在 Display 里
        other => other.to_string(),
    }
}

/// 用户该做什么。**每一条都要能照着做**，别只说"请重试"。
fn next_step(path: &Path, err: &rusqlite::Error) -> &'static str {
    let reason = describe(err).to_lowercase();
    // 顺序有意：磁盘满比权限更常见，先说最可能的那条
    if reason.contains("disk") && reason.contains("full") {
        "磁盘满了。清理出一些空间后重新启动 Time Scope。"
    } else if reason.contains("readonly") || reason.contains("permission") || reason.contains("denied")
    {
        "没有写入权限。确认 %APPDATA%\\time-scope 目录可写，或以管理员身份运行一次。"
    } else if reason.contains("locked") || reason.contains("busy") {
        "数据库正被另一个进程占用。关掉可能残留的 Time Scope 进程（任务管理器里看），\
         再重新启动。数据不会丢。"
    } else if reason.contains("malformed") || reason.contains("corrupt") || reason.contains("not a database")
    {
        "数据库文件已损坏。把它改名备份（例如 time-scope.db.bak），\
         然后重启 —— Time Scope 会新建一个空库重新开始采集。旧数据仍在备份文件里。"
    } else if path.extension().map(|e| e.is_empty()).unwrap_or(true) {
        "路径看起来不是一个数据库文件。检查 %APPDATA%\\time-scope\\time-scope.db \
         是否被改成了别的文件或目录。"
    } else {
        "检查 %APPDATA%\\time-scope\\ 目录是否存在且可写；\
         若是文件损坏，把它改名备份后重启。"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_dir(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("time-scope-dbopen-{name}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("建临时目录");
        p
    }

    #[test]
    fn opening_a_normal_path_works_and_creates_the_file() {
        // 正常路径不该被这次改动影响：文件不存在时应当建出来。
        let dir = tmp_dir("ok");
        let path = dir.join("time-scope.db");
        let conn = open_db(&path).expect("正常路径必须能打开");
        drop(conn);
        assert!(path.exists(), "首次启动应把库建出来");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unopenable_path_returns_an_error_instead_of_panicking() {
        // B4 的核心。旧代码是 `.expect("open db")` → panic，用户看到
        // "thread 'main' panicked at ... open db"，既没有路径也没有原因。
        let dir = tmp_dir("blocked");
        // 在本该是目录的位置放一个**文件**，于是 create_dir_all 必然失败。
        let blocker = dir.join("notadir");
        std::fs::write(&blocker, b"x").unwrap();
        let path = blocker.join("time-scope.db");

        let err = open_db(&path).expect_err("这条路径必然打不开，不该 panic");
        assert_eq!(err.path, path.display().to_string(), "错误里必须带上路径");
        assert!(!err.reason.is_empty(), "错误里必须说明原因");
        assert!(!err.next_step.is_empty(), "错误里必须说下一步怎么办");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_displayed_message_carries_path_reason_and_next_step() {
        let dir = tmp_dir("msg");
        let blocker = dir.join("notadir2");
        std::fs::write(&blocker, b"x").unwrap();
        let err = explain(
            &blocker.join("time-scope.db"),
            &rusqlite::Error::ToSqlConversionFailure(Box::new(
                std::io::Error::new(std::io::ErrorKind::PermissionDenied, "拒绝访问"),
            )),
        );
        let s = err.to_string();
        assert!(s.contains("time-scope.db"), "消息里要有路径：{s}");
        assert!(s.contains("拒绝访问"), "消息里要有原因：{s}");
        assert!(s.contains("下一步"), "消息里要有下一步：{s}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_full_disk_gets_a_cleanup_instruction() {
        let err = explain(
            Path::new(r"C:\x.db"),
            &rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_FULL),
                Some("database or disk is full".into()),
            ),
        );
        assert!(err.next_step.contains("磁盘"), "实际：{}", err.next_step);
    }

    #[test]
    fn a_corrupt_file_gets_a_backup_instruction() {
        let err = explain(
            Path::new(r"C:\x.db"),
            &rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT),
                Some("file is not a database".into()),
            ),
        );
        assert!(
            err.next_step.contains("备份"),
            "损坏必须说清怎么保数据，实际：{}",
            err.next_step
        );
    }

    #[test]
    fn a_locked_file_says_data_will_not_be_lost() {
        // 用户最怕的是"占用了会不会丢数据"，这句话必须出现在提示里。
        let err = explain(
            Path::new(r"C:\x.db"),
            &rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                Some("database is locked".into()),
            ),
        );
        assert!(err.next_step.contains("不会丢"), "实际：{}", err.next_step);
    }
}