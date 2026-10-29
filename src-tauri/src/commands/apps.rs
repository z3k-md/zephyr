use tauri::{AppHandle, Manager, State};

use super::{blocking, publish, record};
use crate::apps;
use crate::query::DispatchOutcome;
use crate::shortcut;
use crate::state::{AppState, Persisted};
use crate::util::now_secs;
use crate::window;

#[tauri::command]
pub async fn launch_app(
    app: AppHandle,
    app_id: String,
    new_window: Option<bool>,
    private: Option<bool>,
) -> Result<DispatchOutcome, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        if private == Some(true) {
            return start_private(&app, &state, &app_id);
        }
        if new_window == Some(true) {
            return start(&app, &state, &app_id);
        }
        launch(&app, &state, &app_id)
    })
    .await
}

/// Opens an app for a launch group: the same as Enter on its row, but the bar is already
/// gone and a launch group reports failures itself.
pub fn launch_quietly(app: &AppHandle, app_id: &str) -> Result<(), String> {
    launch(app, &app.state::<AppState>(), app_id).map(|_| ())
}

/// Opens an app, or switches to its front window when it's already running.
pub(super) fn launch(
    app: &AppHandle,
    state: &AppState,
    app_id: &str,
) -> Result<DispatchOutcome, String> {
    if let Some(target) = apps::index().find(app_id) {
        let windows = crate::running::all();
        if let Some(front) = crate::running::of_app(&target, &windows).first()
            && crate::running::focus(front.handle)
        {
            window::hide_bar(app);
            remember_launch(app, state, &target.id);
            log::info!("switched to {}", target.name);
            return Ok(DispatchOutcome::Launched { app_id: target.id });
        }
    }
    start(app, state, app_id)
}

/// Starts the app: a new instance, even when one is running.
fn start(app: &AppHandle, state: &AppState, app_id: &str) -> Result<DispatchOutcome, String> {
    let target = apps::index()
        .find(app_id)
        .ok_or_else(|| "That app is no longer installed.".to_string())?;
    apps::launch(&target)?;
    // The bar goes first; saving the launch can wait the moment it takes.
    window::hide_bar(app);
    remember_launch(app, state, &target.id);
    log::info!("launched {}", target.name);
    Ok(DispatchOutcome::Launched { app_id: target.id })
}

/// Opens a browser straight into a private window (Incognito, InPrivate).
fn start_private(
    app: &AppHandle,
    state: &AppState,
    app_id: &str,
) -> Result<DispatchOutcome, String> {
    let target = apps::index()
        .find(app_id)
        .ok_or_else(|| "That app is no longer installed.".to_string())?;
    apps::launch_private(&target)?;
    window::hide_bar(app);
    remember_launch(app, state, &target.id);
    log::info!("opened a private window of {}", target.name);
    Ok(DispatchOutcome::Launched { app_id: target.id })
}

/// Saves the launch on a thread of its own: the write (with its retries) shouldn't hold up the
/// bar, which has already hidden.
fn remember_launch(app: &AppHandle, _state: &AppState, app_id: &str) {
    let (app, app_id) = (app.clone(), app_id.to_string());
    std::thread::spawn(move || {
        record(&app, &app.state::<AppState>(), "the launch", |persisted| {
            persisted.record_launch(&app_id, now_secs())
        });
    });
}

/// Switches to an open window (a "Windows" row, or one picked from an app's actions).
#[tauri::command]
pub fn window_focus(app: AppHandle, handle: i64) -> Result<(), String> {
    crate::running::supported()?;
    if !crate::running::focus(handle) {
        return Err("That window is gone.".into());
    }
    window::hide_bar(&app);
    Ok(())
}

/// Asks an open window to close, as its own close button would.
#[tauri::command]
pub fn window_close(handle: i64) -> Result<(), String> {
    crate::running::supported()?;
    let asked = crate::running::close(handle);
    crate::running::refresh();
    if asked {
        Ok(())
    } else {
        Err("That window is gone.".into())
    }
}

/// Quits an app gracefully: every one of its windows is asked to close, so it can still
/// offer to save. Nothing is force-killed.
#[tauri::command]
pub fn app_quit(app_id: String) -> Result<(), String> {
    crate::running::supported()?;
    let target = apps::index()
        .find(&app_id)
        .ok_or_else(|| "That app is no longer installed.".to_string())?;
    let windows = crate::running::all();
    let mut asked = false;
    for window in crate::running::of_app(&target, &windows) {
        asked |= crate::running::close(window.handle);
    }
    crate::running::refresh();
    if asked {
        Ok(())
    } else {
        Err(format!("{} has no windows open.", target.name))
    }
}

#[tauri::command]
pub fn save_launch_groups(
    app: AppHandle,
    state: State<'_, AppState>,
    groups: Vec<crate::state::LaunchGroup>,
) -> Result<Persisted, String> {
    crate::launch_groups::validate(&groups)?;
    let snapshot = state.update(|persisted| {
        // A deleted group's hotkey and alias go with it.
        persisted
            .bindings
            .retain(|binding| match binding.target.strip_prefix("group:") {
                Some(id) => groups.iter().any(|group| group.id == id),
                None => true,
            });
        persisted.launch_groups = groups;
        Ok(())
    })?;
    shortcut::register(&app, &snapshot.summon_shortcut, snapshot.extra_shortcuts())?;
    publish(&app, snapshot)
}

/// Opens everything in a launch group; the bar closes at once.
#[tauri::command]
pub async fn launch_group(app: AppHandle, id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::launch_groups::launch(&app, &id))
        .await
        .map_err(|err| err.to_string())?
}
