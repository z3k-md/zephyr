use tauri::{AppHandle, Emitter, Manager, State};

use super::publish;
use crate::shortcut;
use crate::state::{AppState, Persisted};
use crate::window;

/// While a shortcut is being recorded, Zephyr's own shortcuts are off, so pressing one of
/// them records it instead of opening the bar.
#[tauri::command]
pub fn pause_shortcuts(
    app: AppHandle,
    state: State<'_, AppState>,
    paused: bool,
) -> Result<(), String> {
    if paused {
        shortcut::pause(&app);
        return Ok(());
    }
    let snapshot = state.snapshot()?;
    shortcut::register(&app, &snapshot.summon_shortcut, snapshot.extra_shortcuts())
}

/// Saves a hotkey and alias for one command, app, bookmark, launch group or settings page
/// (both empty removes it), then registers the hotkeys again.
#[tauri::command]
pub fn save_binding(
    app: AppHandle,
    state: State<'_, AppState>,
    binding: crate::state::Binding,
) -> Result<Persisted, String> {
    let current = state.snapshot()?;
    let clipboard = if current.clipboard.enabled {
        current.clipboard.shortcut.as_str()
    } else {
        ""
    };
    let taken = [
        (current.summon_shortcut.as_str(), "the bar"),
        (clipboard, "clipboard history"),
        (current.notes_shortcut.as_str(), "notes"),
    ];
    let updated = crate::bindings::upsert(&current.bindings, binding, &taken)?;
    let snapshot = state.update(|persisted| {
        persisted.bindings = updated;
        Ok(())
    })?;
    shortcut::register(&app, &snapshot.summon_shortcut, snapshot.extra_shortcuts())?;
    publish(&app, snapshot)
}

/// The targets whose hotkey couldn't be registered because another app holds it.
#[tauri::command]
pub fn binding_failures() -> Vec<String> {
    shortcut::failed_bindings()
}

/// A target the bar asked Settings to set a hotkey or alias for.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingBinding {
    pub target: String,
    pub label: String,
}

fn pending_binding() -> &'static std::sync::Mutex<Option<PendingBinding>> {
    static PENDING: std::sync::OnceLock<std::sync::Mutex<Option<PendingBinding>>> =
        std::sync::OnceLock::new();
    PENDING.get_or_init(Default::default)
}

/// From the bar's actions ("Set hotkey…"): opens Settings > Shortcuts on this target.
// Async: it may build the settings webview, which deadlocks a sync command on Windows (see
// open_settings).
#[tauri::command]
pub async fn edit_binding(app: AppHandle, target: String, label: String) -> Result<(), String> {
    if let Ok(mut pending) = pending_binding().lock() {
        *pending = Some(PendingBinding { target, label });
    }
    window::hide_bar(&app);
    window::open_settings(&app, Some("shortcuts"))?;
    let _ = app.emit_to("settings", "binding-pending", ());
    Ok(())
}

/// For Settings > Shortcuts: the target the bar asked about, once.
#[tauri::command]
pub fn take_pending_binding() -> Option<PendingBinding> {
    pending_binding().lock().ok()?.take()
}

/// Runs what a binding's hotkey points at: a mode or scoped search opens the bar there, an
/// app, bookmark, launch group or settings page opens without the bar.
pub fn run_binding(app: &AppHandle, target: &str) {
    let (kind, value) = target.split_once(':').unwrap_or((target, ""));
    let result = match (kind, value) {
        ("view", "notes") => crate::note_windows::show(app, None),
        ("view", "clip") => {
            window::show_clipboard(app);
            Ok(())
        }
        ("view", _) | ("scope", _) => {
            window::show_bar(app);
            let opened = if kind == "scope" {
                target.to_string()
            } else {
                value.to_string()
            };
            app.emit_to("main", "bar-open", opened)
                .map_err(|err| err.to_string())
        }
        ("app", id) => super::apps::launch(app, &app.state::<AppState>(), id).map(|_| ()),
        ("bookmark", url) => super::search::open_url_now(app, url.to_string()),
        ("group", id) => crate::launch_groups::launch(app, id),
        ("setting", id) => super::search::open_setting_page(app, id).map(|_| ()),
        _ => Err(format!("Nothing to run for {target}")),
    };
    if let Err(err) = result {
        log::error!("couldn't run a shortcut: {err}");
    }
}
