use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

use crate::apps::{self, Catalog};
use crate::destination::Destination;
use crate::query::{self, Decision, DispatchOutcome};
use crate::shortcut;
use crate::state::{AppState, Persisted};
use crate::suggest::{self, SuggestResponse};
use crate::{updater, window};

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn publish(app: &AppHandle, snapshot: Persisted) -> Result<Persisted, String> {
    let _ = app.emit("state-changed", &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub fn get_snapshot(state: State<'_, AppState>) -> Result<Persisted, String> {
    state.snapshot()
}

#[tauri::command]
pub async fn suggest(
    state: State<'_, AppState>,
    query: String,
    destination_id: String,
    include_remote: bool,
) -> Result<SuggestResponse, String> {
    let snapshot = state.snapshot()?;
    let installed = apps::index().snapshot();
    let catalog = catalog(&installed, &snapshot);
    Ok(suggest::gather(
        &query,
        &destination_id,
        &snapshot.destinations,
        &snapshot.history,
        catalog,
        include_remote,
    )
    .await)
}

fn catalog<'a>(installed: &'a [apps::App], snapshot: &'a Persisted) -> Catalog<'a> {
    Catalog {
        apps: installed,
        launches: &snapshot.launches,
        overrides: &snapshot.app_overrides,
        now: now_secs(),
    }
}

#[tauri::command]
pub fn launch_app(
    app: AppHandle,
    state: State<'_, AppState>,
    app_id: String,
) -> Result<DispatchOutcome, String> {
    launch(&app, &state, &app_id)
}

fn launch(app: &AppHandle, state: &AppState, app_id: &str) -> Result<DispatchOutcome, String> {
    let target = apps::index()
        .find(app_id)
        .ok_or_else(|| "That app is no longer installed.".to_string())?;
    apps::launch(&target)?;
    match state.update(|persisted| {
        persisted.record_launch(&target.id, now_secs());
        Ok(())
    }) {
        Ok(recorded) => {
            let _ = publish(app, recorded);
        }
        Err(err) => log::error!("couldn't record the launch: {err}"),
    }
    window::hide_bar(app);
    log::info!("launched {}", target.name);
    Ok(DispatchOutcome::Launched { app_id: target.id })
}

#[tauri::command]
pub async fn dispatch(
    app: AppHandle,
    state: State<'_, AppState>,
    query: String,
    destination_id: String,
    interpret: bool,
) -> Result<DispatchOutcome, String> {
    let snapshot = state.snapshot()?;
    let installed = apps::index().snapshot();
    let catalog = catalog(&installed, &snapshot);
    let decision = query::decide(
        &query,
        &destination_id,
        &destination_id,
        interpret,
        &snapshot.destinations,
        catalog,
    );
    // Text that would have launched an app but went to the web by an explicit key was meant
    // for the web; remember it so plain Enter stops launching the app for it.
    let typed = query::parse_input(&query).query;
    let overrode_app = catalog.would_launch(&typed);

    match decision {
        Decision::Open {
            destination_id,
            query,
            url,
        } => {
            app.opener()
                .open_url(&url, None::<&str>)
                .map_err(|err| format!("Couldn't open the browser: {err}"))?;
            // The browser already has the search, so a failed history write is not the user's error.
            match state.update(|persisted| {
                persisted.record(&query, &destination_id, now_secs());
                if overrode_app && destination_id != "url" {
                    persisted.record_app_override(&typed);
                }
                Ok(())
            }) {
                Ok(recorded) => {
                    let _ = publish(&app, recorded);
                }
                Err(err) => log::error!("couldn't record history: {err}"),
            }
            window::hide_bar(&app);
            log::info!("opened {destination_id}");
            Ok(DispatchOutcome::Opened { destination_id })
        }
        Decision::Launch { app_id } => launch(&app, &state, &app_id),
        Decision::Arm { destination_id } => Ok(DispatchOutcome::Armed { destination_id }),
        Decision::Palette => Ok(DispatchOutcome::Palette),
        Decision::UnknownBang { trigger } => Ok(DispatchOutcome::UnknownBang { trigger }),
        Decision::Empty => Ok(DispatchOutcome::Empty),
        Decision::Rejected { message } => Err(message),
    }
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    summon_shortcut: String,
    launch_at_startup: bool,
    default_destination_id: String,
) -> Result<Persisted, String> {
    let shortcut = shortcut::canonical_shortcut(&summon_shortcut)?;
    let previous = state.snapshot()?;
    let shortcut_changed = shortcut != previous.summon_shortcut;

    // Registering unregisters the old shortcut first, so every failure path has to put it back.
    let restore_shortcut = || {
        if shortcut_changed && let Err(err) = shortcut::register(&app, &previous.summon_shortcut) {
            log::error!("couldn't restore the summon shortcut: {err}");
        }
    };

    if shortcut_changed && let Err(err) = shortcut::register(&app, &shortcut) {
        restore_shortcut();
        return Err(err);
    }
    if let Err(err) = apply_autostart(&app, launch_at_startup) {
        restore_shortcut();
        return Err(err);
    }

    match state.update(|persisted| {
        persisted.set_general(
            shortcut.clone(),
            launch_at_startup,
            default_destination_id.clone(),
        )
    }) {
        Ok(snapshot) => publish(&app, snapshot),
        Err(err) => {
            restore_shortcut();
            let _ = apply_autostart(&app, previous.launch_at_startup);
            Err(err)
        }
    }
}

#[tauri::command]
pub fn save_destination(
    app: AppHandle,
    state: State<'_, AppState>,
    destination: Destination,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| persisted.upsert(destination))?;
    publish(&app, snapshot)
}

#[tauri::command]
pub fn remove_destination(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| persisted.remove(&id))?;
    publish(&app, snapshot)
}

#[tauri::command]
pub fn move_destination(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    delta: i32,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| persisted.move_destination(&id, delta))?;
    publish(&app, snapshot)
}

#[tauri::command]
pub fn clear_history(app: AppHandle, state: State<'_, AppState>) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| {
        persisted.clear_history();
        Ok(())
    })?;
    publish(&app, snapshot)
}

#[tauri::command]
pub fn set_bar_height(app: AppHandle, height: f64) -> Result<(), String> {
    window::set_bar_height(&app, height)
}

#[tauri::command]
pub fn hide_bar(app: AppHandle) {
    window::hide_bar(&app);
}

#[tauri::command]
pub fn show_bar(app: AppHandle) {
    window::show_bar(&app);
}

#[tauri::command]
pub fn open_settings(app: AppHandle) -> Result<(), String> {
    window::open_settings(&app)
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<String, String> {
    updater::check_for_updates(&app).await
}

pub fn apply_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let autostart = app.autolaunch();
    let currently = autostart
        .is_enabled()
        .map_err(|err| format!("Couldn't read startup setting: {err}"))?;
    if enabled && !currently {
        autostart
            .enable()
            .map_err(|err| format!("Couldn't turn on startup: {err}"))?;
    } else if !enabled && currently {
        autostart
            .disable()
            .map_err(|err| format!("Couldn't turn off startup: {err}"))?;
    }
    Ok(())
}
