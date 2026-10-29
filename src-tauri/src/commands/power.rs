use tauri::AppHandle;

use super::blocking;
use crate::window;

/// What typed text like "shutdown in 30m" would do, for the bar's Power row.
#[tauri::command]
pub fn power_preview(query: String) -> Option<crate::power::Preview> {
    crate::power::preview(&query, chrono::Local::now())
}

/// Runs a power action now, or schedules it for `at` (unix seconds).
#[tauri::command]
pub async fn power_run(
    app: AppHandle,
    action: crate::power::Action,
    at: Option<i64>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if at.is_none() {
            window::hide_bar(&app);
        }
        crate::power::run(&app, action, at)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
pub async fn power_cancel(app: AppHandle) {
    // Off the main thread: dropping the schedule saves the settings file.
    let _ = blocking(move || {
        crate::power::cancel(&app);
        Ok(())
    })
    .await;
}
