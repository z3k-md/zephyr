use tauri::AppHandle;

use super::blocking;
use crate::countdown::{self, Countdown};

/// Starts a timer for `seconds`, named `label` (may be empty).
#[tauri::command]
pub async fn timer_start(app: AppHandle, seconds: i64, label: String) -> Result<Countdown, String> {
    // Off the main thread: it saves the settings file.
    blocking(move || countdown::start(&app, seconds, label)).await
}

#[tauri::command]
pub async fn timer_cancel(app: AppHandle, id: u64) -> Result<(), String> {
    blocking(move || {
        countdown::cancel(&app, id);
        Ok(())
    })
    .await
}
