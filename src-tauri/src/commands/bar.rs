use tauri::{AppHandle, State};

use super::publish;
use crate::state::AppState;
use crate::window;

#[tauri::command]
pub fn set_bar_height(app: AppHandle, height: f64, width: Option<f64>) -> Result<(), String> {
    window::set_bar_height(&app, height, width)
}

#[tauri::command]
pub fn hide_bar(app: AppHandle) {
    window::dismiss_bar(&app);
}

// Sync, so the usual case (Settings already built, just hidden) shows it straight from the
// main thread with no thread hop. Building a new webview inside a sync command deadlocks on
// Windows (WebView2 needs the message loop the IPC callback is blocking), so when the window
// doesn't exist yet it is built from a task instead.
#[tauri::command]
pub fn open_settings(app: AppHandle) -> Result<(), String> {
    if window::show_settings_now(&app) {
        return Ok(());
    }
    tauri::async_runtime::spawn(async move {
        if let Err(err) = window::open_settings(&app, None) {
            log::error!("couldn't open Settings: {err}");
        }
    });
    Ok(())
}

#[tauri::command]
pub fn hide_settings(app: AppHandle) {
    window::hide_settings(&app);
}

/// The page's own title strip, for a window without the system's: `drag` moves the window
/// (from a press on the strip), `minimize`, `maximize` (or restore), and `close`.
#[tauri::command]
pub fn window_chrome(window: tauri::WebviewWindow, action: String) -> Result<(), String> {
    match action.as_str() {
        "drag" => window.start_dragging(),
        "minimize" => window.minimize(),
        "maximize" if window.is_maximized().unwrap_or(false) => window.unmaximize(),
        "maximize" => window.maximize(),
        "close" => window.close(),
        _ => return Err(format!("No window action {action}")),
    }
    .map_err(|err| err.to_string())
}

/// A settings or notes page has drawn and can be shown.
#[tauri::command]
pub fn page_ready(app: AppHandle, window: tauri::WebviewWindow) {
    window::page_ready(&app, window.label());
}

/// Records a finished typing test; true when it is a new personal best for its mode.
#[tauri::command]
pub fn save_typing_result(
    app: AppHandle,
    state: State<'_, AppState>,
    result: crate::state::TypingBest,
) -> Result<bool, String> {
    let mut best = false;
    let snapshot = state.update(|persisted| {
        best = persisted.record_typing(result.clone())?;
        Ok(())
    })?;
    if best {
        let _ = publish(&app, snapshot);
    }
    Ok(best)
}

/// Makes the snap grid's window when Ctrl is first held on the bar, before any drag.
#[tauri::command]
pub async fn prepare_grid(app: AppHandle) -> Result<(), String> {
    window::prepare_grid(&app)
}

/// Ctrl+mousedown on the bar (⌘ on a Mac): the window follows the mouse until the button
/// comes up. On a Mac the page drives the move and gets [x, y, scale] back.
#[tauri::command]
pub fn start_bar_drag(app: AppHandle) -> Result<Option<(i32, i32, f64)>, String> {
    window::start_bar_drag(&app)
}

/// macOS: the bar's new top-left during a ⌘-drag, in physical pixels.
#[tauri::command]
pub fn bar_move(app: AppHandle, x: i32, y: i32) {
    #[cfg(target_os = "macos")]
    window::free_drag::moving(&app, x, y);
    #[cfg(not(target_os = "macos"))]
    let _ = (app, x, y);
}

/// macOS: the ⌘-drag ended; the bar lands on the guide it was near.
#[tauri::command]
pub fn bar_move_end(app: AppHandle) {
    #[cfg(target_os = "macos")]
    window::free_drag::end(&app);
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// Remembers the view the bar should come back to (`notes`, `clipboard`, …; empty for
/// the search bar), so it survives hiding and restarts.
#[tauri::command]
pub fn set_active_mode(state: State<'_, AppState>, mode: String) -> Result<(), String> {
    state.update(|persisted| {
        persisted.active_mode = mode.chars().take(32).collect();
        Ok(())
    })?;
    Ok(())
}
