use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use super::{blocking, save};
use crate::state::{AppState, Persisted};

/// One entry for the running bug capture's log; does nothing when not recording.
#[tauri::command]
pub fn capture_event(kind: String, data: serde_json::Value) {
    crate::capture::record(&kind, data);
}

#[tauri::command]
pub fn capture_recording() -> bool {
    crate::capture::recording()
}

/// Ctrl+R in a Zephyr window: starts or stops a bug recording, when captures are on.
#[tauri::command]
pub fn capture_toggle(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if state.snapshot()?.bug_capture || crate::capture::recording() {
        crate::capture::toggle(&app);
    }
    Ok(())
}

/// Ctrl+Shift+S in a Zephyr window: a bug snapshot, when captures are on.
#[tauri::command]
pub fn capture_snapshot(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if state.snapshot()?.bug_capture {
        crate::capture::snapshot(&app);
    }
    Ok(())
}

/// Turns bug captures on or off; turning them off ends a running recording.
#[tauri::command]
pub async fn save_bug_capture(app: AppHandle, enabled: bool) -> Result<Persisted, String> {
    let snapshot = save(app.clone(), move |persisted| {
        persisted.bug_capture = enabled;
        Ok(())
    })
    .await?;
    if !enabled && crate::capture::recording() {
        crate::capture::toggle(&app);
    }
    Ok(snapshot)
}

#[tauri::command]
pub async fn reveal_captures(app: AppHandle) -> Result<(), String> {
    blocking(move || {
        let folder = crate::capture::folder()?;
        app.opener()
            .open_path(folder.to_string_lossy(), None::<&str>)
            .map_err(|err| err.to_string())
    })
    .await
}
