use tauri::{AppHandle, State};

use super::{blocking, publish};
use crate::clipboard;
use crate::shortcut;
use crate::state::{AppState, Persisted};
use crate::window;

#[tauri::command]
pub async fn clip_list(query: String, filter: String) -> clipboard::ClipList {
    // Off the main thread: decrypting and matching a long history takes a moment.
    tauri::async_runtime::spawn_blocking(move || clipboard::list(&query, &filter, 300))
        .await
        .unwrap_or_else(|_| clipboard::list("", "", 0))
}

#[tauri::command]
pub async fn clip_detail(id: u64) -> Result<clipboard::ClipDetail, String> {
    tauri::async_runtime::spawn_blocking(move || clipboard::detail(id))
        .await
        .map_err(|err| err.to_string())?
}

/// Pastes into the app in front. Without Accessibility permission it copies instead and
/// says so.
#[tauri::command]
pub fn clip_paste(app: AppHandle, id: u64, plain: bool) -> Result<String, String> {
    match clipboard::paste(id, plain, || window::hide_bar(&app))? {
        clipboard::PasteOutcome::Pasted => Ok("pasted".into()),
        clipboard::PasteOutcome::NeedsPermission => Ok("needsPermission".into()),
    }
}

#[tauri::command]
pub fn clip_copy(app: AppHandle, id: u64) -> Result<(), String> {
    clipboard::copy(id, false)?;
    window::dismiss_bar(&app);
    Ok(())
}

// Off the main thread: these wait on the history, which can be busy loading.
#[tauri::command]
pub async fn clip_pin(id: u64, pinned: bool) -> Result<(), String> {
    blocking(move || clipboard::set_pinned(id, pinned)).await
}

#[tauri::command]
pub async fn clip_delete(id: u64) -> Result<(), String> {
    blocking(move || clipboard::delete(id)).await
}

#[tauri::command]
pub async fn clip_clear(pinned_too: bool) -> Result<usize, String> {
    blocking(move || clipboard::clear(pinned_too)).await
}

#[tauri::command]
pub fn clip_can_paste() -> bool {
    clipboard::can_paste()
}

#[tauri::command]
pub fn clip_request_paste_permission() -> bool {
    clipboard::request_paste_permission()
}

#[tauri::command]
pub async fn clip_stats() -> std::collections::HashMap<&'static str, usize> {
    // Off the main thread: it waits on the history, which can be busy loading.
    tauri::async_runtime::spawn_blocking(clipboard::stats)
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub fn save_clipboard_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: clipboard::ClipboardSettings,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| persisted.set_clipboard(settings))?;
    if let Err(err) =
        shortcut::register(&app, &snapshot.summon_shortcut, snapshot.extra_shortcuts())
    {
        log::error!("{err}");
    }
    clipboard::apply_settings(snapshot.clipboard.clone());
    publish(&app, snapshot)
}
