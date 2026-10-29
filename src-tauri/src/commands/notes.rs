use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use super::{blocking, publish};
use crate::notes;
use crate::shortcut;
use crate::state::{AppState, Persisted};
use crate::window;

#[tauri::command]
pub async fn notes_list(query: String) -> Vec<notes::NoteSummary> {
    tauri::async_runtime::spawn_blocking(move || notes::list(&query))
        .await
        .unwrap_or_default()
}

// Notes are files, so reading and writing them stays off the main thread.
#[tauri::command]
pub async fn note_get(id: String) -> Result<notes::Note, String> {
    blocking(move || notes::get(&id)).await
}

#[tauri::command]
pub async fn note_create(body: String) -> Result<String, String> {
    blocking(move || notes::create(&body)).await
}

#[tauri::command]
pub async fn note_save(id: String, body: String) -> Result<i64, String> {
    blocking(move || notes::save(&id, &body)).await
}

#[tauri::command]
pub async fn note_delete(id: String) -> Result<(), String> {
    blocking(move || notes::delete(&id)).await
}

#[tauri::command]
pub async fn reveal_notes(app: AppHandle) -> Result<(), String> {
    blocking(move || {
        app.opener()
            .open_path(notes::folder()?, None::<&str>)
            .map_err(|err| err.to_string())
    })
    .await
}

/// Opens the notes window, on `id` when given, or on a new note holding `body`.
// Async: it may build a webview, which deadlocks a sync command on Windows (see open_settings).
#[tauri::command]
pub async fn open_notes(
    app: AppHandle,
    id: Option<String>,
    body: Option<String>,
) -> Result<(), String> {
    let id = match (id, body) {
        (Some(id), _) => Some(id),
        (None, Some(body)) => Some(blocking(move || notes::create(&body)).await?),
        (None, None) => None,
    };
    window::hide_bar(&app);
    crate::note_windows::show(&app, id.as_deref())
}

/// From a note's window: opens `id`, or a new note when none is given, in a window of its
/// own (or brings its window forward). Returns the note's id.
// Async: it may build a webview, which deadlocks a sync command on Windows (see open_settings).
#[tauri::command]
pub async fn open_note_window(app: AppHandle, id: Option<String>) -> Result<String, String> {
    let id = match id {
        Some(id) => id,
        None => blocking(|| notes::create("")).await?,
    };
    crate::note_windows::open(&app, &id, true)?;
    Ok(id)
}

/// From a note's window whose save failed while closing: it stays open, with the edits.
#[tauri::command]
pub fn keep_note_window(window: tauri::WebviewWindow) {
    crate::note_windows::keep_open(window.label());
}

/// From a note's window, once it has saved: closes that window. The note stays.
#[tauri::command]
pub fn close_note_window(app: AppHandle, window: tauri::WebviewWindow) {
    crate::note_windows::close(&app, &window);
}

#[tauri::command]
pub fn save_notes_shortcut(
    app: AppHandle,
    state: State<'_, AppState>,
    shortcut: String,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| persisted.set_notes_shortcut(shortcut))?;
    if let Err(err) =
        shortcut::register(&app, &snapshot.summon_shortcut, snapshot.extra_shortcuts())
    {
        log::error!("{err}");
    }
    publish(&app, snapshot)
}
