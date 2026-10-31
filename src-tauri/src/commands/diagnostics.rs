use tauri::{AppHandle, WebviewWindow};

use super::blocking;

/// How much of the log Copy diagnostics takes.
const DIAGNOSTIC_LINES: usize = 300;

/// A page's console line for Zephyr's log (src/log.ts). Async, so the write is off the main thread.
#[tauri::command]
pub async fn log_webview(window: WebviewWindow, level: String, page: String, text: String) {
    crate::logger::webview(&level, window.label(), &page, &text);
}

/// Settings › Dev › Copy diagnostics: the build and the end of the log, for a bug report.
/// Off the main thread: it reads up to a megabyte of log.
#[tauri::command]
pub async fn diagnostics(app: AppHandle) -> Result<String, String> {
    let version = app.package_info().version.to_string();
    blocking(move || Ok(report(&version))).await
}

fn report(version: &str) -> String {
    format!(
        "Zephyr {version} ({}) on {} {}\nLog: {}\n\n{}",
        if cfg!(debug_assertions) {
            "dev"
        } else {
            "release"
        },
        std::env::consts::OS,
        std::env::consts::ARCH,
        crate::logger::current_file().display(),
        crate::logger::tail(DIAGNOSTIC_LINES)
    )
}
