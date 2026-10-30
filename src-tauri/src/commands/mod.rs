//! Everything the pages can invoke, grouped by what it's for. Command names are the API
//! `src/api.ts` calls, so they stay put when code moves between these files.

use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::{AppState, Persisted};

pub mod apps;
pub mod bar;
pub mod capture;
pub mod clipboard;
pub mod files;
pub mod media;
pub mod net;
pub mod notes;
pub mod power;
pub mod resources;
pub mod search;
pub mod settings;
pub mod shell;
pub mod shortcuts;
pub mod stocks;
pub mod sync;
pub mod updates;
pub mod weather;

/// Runs blocking work (files, processes, the registry, the keychain) on a worker thread.
/// Sync commands run on the main thread, which every window's event loop shares, so a slow
/// one would freeze them all.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| err.to_string())?
}

fn publish(app: &AppHandle, snapshot: Persisted) -> Result<Persisted, String> {
    let _ = app.emit("state-changed", &snapshot);
    Ok(snapshot)
}

/// Saves a note of something that already happened (a launch, an open, a search). A failed
/// write is only logged: what the user asked for has been done.
fn record(app: &AppHandle, state: &AppState, what: &str, change: impl FnOnce(&mut Persisted)) {
    match state.update(|persisted| {
        change(persisted);
        Ok(())
    }) {
        Ok(recorded) => {
            let _ = publish(app, recorded);
        }
        Err(err) => log::error!("couldn't record {what}: {err}"),
    }
}

#[tauri::command]
pub fn get_snapshot(state: State<'_, AppState>) -> Result<Persisted, String> {
    state.snapshot().map(|snapshot| Persisted::clone(&snapshot))
}

/// Saves a settings change and sends every window the new snapshot. The write is synced to
/// disk, so it runs on a worker thread, never the main one (see [`blocking`]).
async fn save(
    app: AppHandle,
    change: impl FnOnce(&mut Persisted) -> Result<(), String> + Send + 'static,
) -> Result<Persisted, String> {
    blocking(move || save_now(&app, change)).await
}

/// [`save`] for work already on a worker thread.
fn save_now(
    app: &AppHandle,
    change: impl FnOnce(&mut Persisted) -> Result<(), String>,
) -> Result<Persisted, String> {
    let snapshot = app.state::<AppState>().update(change)?;
    publish(app, snapshot)
}
