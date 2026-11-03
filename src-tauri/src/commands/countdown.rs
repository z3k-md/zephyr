use tauri::AppHandle;

use super::{blocking, save};
use crate::countdown::{self, Countdown};
use crate::focus::FocusSettings;
use crate::state::Persisted;

/// Starts a timer for `seconds`, named `label` (may be empty). `focus` makes it a focus
/// session, which replaces one already running.
#[tauri::command]
pub async fn timer_start(
    app: AppHandle,
    seconds: i64,
    label: String,
    focus: bool,
) -> Result<Countdown, String> {
    // Off the main thread: it saves the settings file and may minimize windows.
    blocking(move || countdown::start(&app, seconds, label, focus)).await
}

/// Saves Settings › Focus. A session already running picks up a new site list.
#[tauri::command]
pub async fn save_focus(app: AppHandle, focus: FocusSettings) -> Result<Persisted, String> {
    let focus = focus.normalized()?;
    let snapshot = save(app.clone(), move |persisted| {
        persisted.focus = focus;
        Ok(())
    })
    .await?;
    crate::focus::publish(&app);
    Ok(snapshot)
}

#[tauri::command]
pub async fn timer_cancel(app: AppHandle, id: u64) -> Result<(), String> {
    blocking(move || {
        countdown::cancel(&app, id);
        Ok(())
    })
    .await
}
