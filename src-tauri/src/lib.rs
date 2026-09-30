//! Tauri 应用入口。Task 6 只放最小壳；Task 9 会在这里接上 collector/storage/IPC。

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
