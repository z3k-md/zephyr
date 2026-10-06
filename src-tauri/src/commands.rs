use std::time::{SystemTime, UNIX_EPOCH};

use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

use crate::ai::{self, AiEvent, AiSettings};
use crate::apps::{self, Catalog};
use crate::destination::Destination;
use crate::files;
use crate::query::{self, Decision, DispatchOutcome};
use crate::settings::{self, Target};
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
        settings: settings::catalog(),
        files: files::index(),
        launches: &snapshot.launches,
        file_opens: &snapshot.file_opens,
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

// Async for the same reason as open_settings: a Zephyr setting may build the settings webview.
#[tauri::command]
pub async fn open_setting(app: AppHandle, setting_id: String) -> Result<DispatchOutcome, String> {
    open_setting_page(&app, &setting_id)
}

fn open_setting_page(app: &AppHandle, setting_id: &str) -> Result<DispatchOutcome, String> {
    let setting = settings::find(setting_id)
        .ok_or_else(|| "That setting isn't available here.".to_string())?;
    match setting.target {
        Target::Zephyr(section) => window::open_settings(app, Some(section))?,
        target => settings::open_system(target, |uri| {
            app.opener()
                .open_url(uri, None::<&str>)
                .map_err(|err| err.to_string())
        })
        .map_err(|err| format!("Couldn't open {}: {err}", setting.title))?,
    }
    window::hide_bar(app);
    log::info!("opened setting {}", setting.id);
    Ok(DispatchOutcome::SettingOpened {
        setting_id: setting.id.to_string(),
    })
}

#[tauri::command]
pub fn open_file(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<DispatchOutcome, String> {
    open_file_path(&app, &state, &path)
}

/// Only paths the bar could have shown are opened: indexed ones and ones opened before.
fn known_file(state: &AppState, path: &str) -> Result<(), String> {
    let opened_before = state
        .snapshot()?
        .file_opens
        .iter()
        .any(|open| open.app_id == path);
    if opened_before || files::index().contains(path) {
        Ok(())
    } else {
        Err("That file isn't in Zephyr's file index.".into())
    }
}

fn open_file_path(
    app: &AppHandle,
    state: &AppState,
    path: &str,
) -> Result<DispatchOutcome, String> {
    known_file(state, path)?;
    if !std::path::Path::new(path).exists() {
        return Err("That file was moved or deleted.".into());
    }
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|err| format!("Couldn't open it: {err}"))?;
    match state.update(|persisted| {
        persisted.record_file_open(path, now_secs());
        Ok(())
    }) {
        Ok(recorded) => {
            let _ = publish(app, recorded);
        }
        Err(err) => log::error!("couldn't record the open: {err}"),
    }
    window::hide_bar(app);
    Ok(DispatchOutcome::FileOpened {
        path: path.to_string(),
    })
}

#[tauri::command]
pub fn reveal_file(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<(), String> {
    known_file(&state, &path)?;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|err| format!("Couldn't show it: {err}"))?;
    window::hide_bar(&app);
    Ok(())
}

#[tauri::command]
pub fn file_index_status() -> files::Status {
    files::index().status()
}

#[tauri::command]
pub fn rebuild_file_index() {
    files::index().rescan();
}

#[tauri::command]
pub fn save_file_folders(
    app: AppHandle,
    state: State<'_, AppState>,
    roots: Option<Vec<String>>,
    excludes: Vec<String>,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| persisted.set_file_folders(roots, excludes))?;
    files::index().configure(snapshot.file_config(), file_cache_path(&app));
    publish(&app, snapshot)
}

pub fn file_cache_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager;
    app.path()
        .app_cache_dir()
        .ok()
        .map(|dir| dir.join("files.idx"))
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
            if crate::template::is_path(&url) {
                app.opener()
                    .open_path(&url, None::<&str>)
                    .map_err(|err| format!("Couldn't open {url}: {err}"))?;
            } else {
                app.opener()
                    .open_url(&url, None::<&str>)
                    .map_err(|err| format!("Couldn't open it: {err}"))?;
            }
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
        Decision::OpenSetting { setting_id } => open_setting_page(&app, &setting_id),
        Decision::OpenFile { path } => open_file_path(&app, &state, &path),
        Decision::Ask {
            destination_id,
            query,
        } => {
            match state.update(|persisted| {
                persisted.record(&query, &destination_id, now_secs());
                Ok(())
            }) {
                Ok(recorded) => {
                    let _ = publish(&app, recorded);
                }
                Err(err) => log::error!("couldn't record history: {err}"),
            }
            Ok(DispatchOutcome::Ask { query })
        }
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
    window::dismiss_bar(&app);
}

#[tauri::command]
pub fn show_bar(app: AppHandle) {
    window::show_bar(&app);
}

// Must stay async: a sync command runs inside the webview's IPC callback on the main thread,
// and building a new webview there deadlocks on Windows (WebView2) because creation needs
// the message loop that the callback is blocking.
#[tauri::command]
pub async fn open_settings(app: AppHandle) -> Result<(), String> {
    window::open_settings(&app, None)
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

/// Streams an answer into the bar. Resolves when the answer ends; a newer ask or hiding the
/// bar stops it early.
#[tauri::command]
pub async fn ai_ask(
    state: State<'_, AppState>,
    question: String,
    on_event: Channel<AiEvent>,
) -> Result<(), String> {
    let settings = state.snapshot()?.ai;
    ai::ask(&settings, &question, |event| {
        let _ = on_event.send(event);
    })
    .await;
    Ok(())
}

#[tauri::command]
pub fn ai_cancel() {
    ai::cancel();
}

#[tauri::command]
pub fn save_ai_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: AiSettings,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| persisted.set_ai(settings))?;
    publish(&app, snapshot)
}

#[tauri::command]
pub fn ai_presets() -> &'static [ai::Preset] {
    ai::PRESETS
}

#[tauri::command]
pub fn ai_set_key(provider: String, key: Option<String>) -> Result<(), String> {
    ai::set_key(&provider, key.as_deref())
}

#[tauri::command]
pub fn ai_has_key(provider: String) -> bool {
    ai::has_key(&provider)
}

#[tauri::command]
pub async fn ai_models(settings: AiSettings) -> Result<Vec<String>, String> {
    ai::models_for(&settings).await
}

#[tauri::command]
pub async fn ai_detect_local() -> Vec<ai::LocalServer> {
    ai::detect_local().await
}

/// The link a search would open, for the action panel's Copy link.
#[tauri::command]
pub fn resolve_url(
    state: State<'_, AppState>,
    query: String,
    destination_id: String,
) -> Result<String, String> {
    let snapshot = state.snapshot()?;
    let destination = crate::destination::enabled(&snapshot.destinations, &destination_id)
        .filter(|destination| !destination.is_ai())
        .ok_or("That destination has no link")?;
    crate::destination::build_url(&destination.url_template, &query::parse_input(&query).query)
}

/// Every web destination as JSON, for backup or sharing.
#[tauri::command]
pub fn export_destinations(state: State<'_, AppState>) -> Result<String, String> {
    let snapshot = state.snapshot()?;
    let web: Vec<&Destination> = snapshot
        .destinations
        .iter()
        .filter(|destination| !destination.is_ai())
        .collect();
    serde_json::to_string_pretty(&web).map_err(|err| err.to_string())
}

#[tauri::command]
pub fn import_destinations(
    app: AppHandle,
    state: State<'_, AppState>,
    json: String,
) -> Result<Persisted, String> {
    let value: serde_json::Value =
        serde_json::from_str(json.trim()).map_err(|err| format!("That isn't valid JSON: {err}"))?;
    // Accept a bare list, or an object with a destinations list such as a state file.
    let list = value.get("destinations").cloned().unwrap_or(value);
    let incoming: Vec<crate::state::ImportedDestination> = serde_json::from_value(list)
        .map_err(|err| format!("That JSON isn't a list of destinations: {err}"))?;
    let snapshot = state.update(|persisted| persisted.import(incoming).map(|_| ()))?;
    publish(&app, snapshot)
}
