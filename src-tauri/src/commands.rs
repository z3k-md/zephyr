use std::time::{SystemTime, UNIX_EPOCH};

use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

use crate::ai::{self, AiEvent, AiSettings};
use crate::apps::{self, Catalog};
use crate::clipboard;
use crate::destination::Destination;
use crate::files;
use crate::notes;
use crate::query::{self, Decision, DispatchOutcome};
use crate::settings::{self, Target};
use crate::shell::{self, ShellEvent, ShellInfo};
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
    let mut response = suggest::gather(
        &query,
        &destination_id,
        &snapshot.destinations,
        &snapshot.history,
        catalog,
        include_remote,
    )
    .await;
    crate::running::annotate(&mut response, &installed, &query);
    Ok(response)
}

/// Connects to the destination's suggestion service when the bar opens, so the first
/// keystroke's suggestions aren't slow.
#[tauri::command]
pub async fn warm_suggestions(
    state: State<'_, AppState>,
    destination_id: String,
) -> Result<(), String> {
    let destination = state
        .snapshot()?
        .destinations
        .into_iter()
        .find(|destination| destination.id == destination_id);
    if let Some(destination) = destination {
        suggest::warm(&destination).await;
    }
    Ok(())
}

fn catalog<'a>(installed: &'a [apps::App], snapshot: &'a Persisted) -> Catalog<'a> {
    Catalog {
        apps: installed,
        settings: settings::catalog(),
        files: files::index(),
        launches: &snapshot.launches,
        file_opens: &snapshot.file_opens,
        overrides: &snapshot.app_overrides,
        setting_opens: &snapshot.setting_opens,
        now: now_secs(),
    }
}

#[tauri::command]
pub fn launch_app(
    app: AppHandle,
    state: State<'_, AppState>,
    app_id: String,
    new_window: Option<bool>,
    private: Option<bool>,
) -> Result<DispatchOutcome, String> {
    if private == Some(true) {
        return start_private(&app, &state, &app_id);
    }
    if new_window == Some(true) {
        return start(&app, &state, &app_id);
    }
    launch(&app, &state, &app_id)
}

/// Opens an app for a launch group: the same as Enter on its row, but the bar is already
/// gone and a launch group reports failures itself.
pub fn launch_quietly(app: &AppHandle, app_id: &str) -> Result<(), String> {
    use tauri::Manager;
    launch(app, &app.state::<AppState>(), app_id).map(|_| ())
}

/// Opens an app, or switches to its front window when it's already running.
fn launch(app: &AppHandle, state: &AppState, app_id: &str) -> Result<DispatchOutcome, String> {
    if let Some(target) = apps::index().find(app_id) {
        let windows = crate::running::all();
        if let Some(front) = crate::running::of_app(&target, &windows).first()
            && crate::running::focus(front.handle)
        {
            remember_launch(app, state, &target.id);
            window::hide_bar(app);
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
    remember_launch(app, state, &target.id);
    window::hide_bar(app);
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
    remember_launch(app, state, &target.id);
    window::hide_bar(app);
    log::info!("opened a private window of {}", target.name);
    Ok(DispatchOutcome::Launched { app_id: target.id })
}

fn remember_launch(app: &AppHandle, state: &AppState, app_id: &str) {
    match state.update(|persisted| {
        persisted.record_launch(app_id, now_secs());
        Ok(())
    }) {
        Ok(recorded) => {
            let _ = publish(app, recorded);
        }
        Err(err) => log::error!("couldn't record the launch: {err}"),
    }
}

/// Switches to an open window (a "Windows" row, or one picked from an app's actions).
#[tauri::command]
pub fn window_focus(app: AppHandle, handle: i64) -> Result<(), String> {
    if !crate::running::focus(handle) {
        return Err("That window is gone.".into());
    }
    window::hide_bar(&app);
    Ok(())
}

/// Asks an open window to close, as its own close button would.
#[tauri::command]
pub fn window_close(handle: i64) -> Result<(), String> {
    crate::running::close(handle);
    crate::running::refresh();
    Ok(())
}

/// Quits an app gracefully: every one of its windows is asked to close, so it can still
/// offer to save. Nothing is force-killed.
#[tauri::command]
pub fn app_quit(app_id: String) -> Result<(), String> {
    let target = apps::index()
        .find(&app_id)
        .ok_or_else(|| "That app is no longer installed.".to_string())?;
    let windows = crate::running::all();
    for window in crate::running::of_app(&target, &windows) {
        crate::running::close(window.handle);
    }
    crate::running::refresh();
    Ok(())
}

// Async for the same reason as open_settings: opening an app setting may build the settings webview.
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
    use tauri::Manager;
    let state = app.state::<AppState>();
    match state.update(|persisted| {
        persisted.record_setting_open(setting.id, now_secs());
        Ok(())
    }) {
        Ok(recorded) => {
            let _ = publish(app, recorded);
        }
        Err(err) => log::error!("couldn't record the setting: {err}"),
    }
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

/// Lets file search into Desktop, Documents and Downloads; macOS asks for each as the index
/// first reads it. Called when file search is first used, or from Settings › Permissions.
#[tauri::command]
pub fn allow_protected_folders(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| {
        persisted.protected_folders = true;
        Ok(())
    })?;
    files::index().configure(snapshot.file_config(), file_cache_path(&app));
    publish(&app, snapshot)
}

/// CPU, memory, disk and network for the bar's footer; the bar asks about once a second
/// while it is showing.
#[tauri::command]
pub async fn system_stats() -> Result<crate::stats::Stats, String> {
    tauri::async_runtime::spawn_blocking(crate::stats::sample)
        .await
        .map_err(|err| err.to_string())
}

/// One tick of System Resources: totals, the last minute, and the rows that can show.
#[tauri::command]
pub async fn resources_snapshot(
    sort: String,
    filter: String,
    limit: usize,
) -> Result<crate::resources::Snapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::resources::snapshot(&sort, &filter, limit.clamp(1, 200))
    })
    .await
    .map_err(|err| err.to_string())
}

/// The view closed; its process table goes after half a minute unless it reopens.
#[tauri::command]
pub fn resources_closed() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(31));
        crate::resources::idle();
    });
}

#[tauri::command]
pub async fn resources_quit(name: String, pids: Vec<u32>, force: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::resources::quit(&name, &pids, force))
        .await
        .map_err(|err| err.to_string())?
}

/// Quits Finder, the Dock or Explorer so they start fresh.
#[tauri::command]
pub async fn resources_relaunch(name: String, pids: Vec<u32>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::resources::relaunch(&name, &pids))
        .await
        .map_err(|err| err.to_string())?
}

/// Brings an app forward and hides the bar; false when it has no windows to show.
#[tauri::command]
pub fn resources_switch(app: AppHandle, pids: Vec<u32>) -> bool {
    let switched = crate::resources::switch_to(&pids);
    if switched {
        window::hide_bar(&app);
    }
    switched
}

#[tauri::command]
pub fn resources_reveal(app: AppHandle, path: String) -> Result<(), String> {
    if path.is_empty() || !std::path::Path::new(&path).exists() {
        return Err("Its file isn't there anymore".into());
    }
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|err| format!("Couldn't show it: {err}"))?;
    window::hide_bar(&app);
    Ok(())
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
pub fn launch_group(app: AppHandle, id: String) -> Result<(), String> {
    crate::launch_groups::launch(&app, &id)
}

/// Every installed app as (id, name), for picking a launch group's apps in Settings.
#[tauri::command]
pub fn installed_apps() -> Vec<(String, String)> {
    let mut apps: Vec<(String, String)> = apps::index()
        .snapshot()
        .iter()
        .map(|app| (app.id.clone(), app.name.clone()))
        .collect();
    apps.sort_by_key(|(_, name)| name.to_lowercase());
    apps
}

/// Opens first-run setup again (Settings › General › Run setup again).
#[tauri::command]
pub async fn open_onboarding(app: AppHandle) -> Result<(), String> {
    window::open_onboarding(&app)
}

/// Finishing or skipping setup: it won't show again (except with the dev switch).
#[tauri::command]
pub fn finish_onboarding(app: AppHandle, state: State<'_, AppState>) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| {
        persisted.onboarding_done = true;
        Ok(())
    })?;
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("onboarding") {
        let _ = window.hide();
        window::regular_while_open(&app);
        let _ = window.close();
    }
    publish(&app, snapshot)
}

/// Dev builds: show setup on every launch.
#[tauri::command]
pub fn save_onboarding_every_launch(
    app: AppHandle,
    state: State<'_, AppState>,
    every: bool,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| {
        persisted.onboarding_every_launch = every;
        Ok(())
    })?;
    publish(&app, snapshot)
}

/// Picks the one AI the bar offers in Search with and on Tab and ⌘↵.
#[tauri::command]
pub fn save_ai_choice(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| {
        if !persisted
            .destinations
            .iter()
            .any(|destination| destination.id == id)
        {
            return Err("That destination no longer exists".into());
        }
        persisted.ai_choice = id;
        Ok(())
    })?;
    publish(&app, snapshot)
}

#[tauri::command]
pub fn save_show_stats(
    app: AppHandle,
    state: State<'_, AppState>,
    show: bool,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| {
        persisted.show_stats = show;
        Ok(())
    })?;
    publish(&app, snapshot)
}

/// Each permission the app can use on this system and whether it has it.
#[tauri::command]
pub async fn permissions_status(
    state: State<'_, AppState>,
) -> Result<Vec<crate::permissions::Permission>, String> {
    let protected = state.snapshot()?.protected_folders;
    tauri::async_runtime::spawn_blocking(move || crate::permissions::statuses(protected))
        .await
        .map_err(|err| err.to_string())
}

/// Allow on a permission: the system's own prompt when it has never been asked, otherwise
/// the exact System Settings pane. Returns the statuses afterward.
#[tauri::command]
pub async fn permission_request(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<crate::permissions::Permission>, String> {
    use crate::permissions::{self, Status};
    let open_pane = |app: &AppHandle| {
        if let Some(url) = permissions::pane(&id) {
            let _ = app.opener().open_url(url, None::<&str>);
        }
    };
    match id.as_str() {
        "accessibility" => {
            // The only one macOS won't grant from a dialog: the switch is in System Settings.
            permissions::request_accessibility(&app.config().identifier);
            open_pane(&app);
        }
        "automation" => {
            let status = tauri::async_runtime::spawn_blocking(|| permissions::automation(true))
                .await
                .map_err(|err| err.to_string())?;
            if status == Status::Denied {
                open_pane(&app);
            }
        }
        "files" => {
            let snapshot = state.update(|persisted| {
                persisted.protected_folders = true;
                Ok(())
            })?;
            files::index().configure(snapshot.file_config(), file_cache_path(&app));
            let _ = app.emit("state-changed", &snapshot);
            let status = tauri::async_runtime::spawn_blocking(permissions::probe_files)
                .await
                .map_err(|err| err.to_string())?;
            if status == Status::Denied {
                open_pane(&app);
            }
        }
        _ => return Err(format!("No permission called {id}")),
    }
    let protected = state.snapshot()?.protected_folders;
    tauri::async_runtime::spawn_blocking(move || permissions::statuses(protected))
        .await
        .map_err(|err| err.to_string())
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
    let overrode_app = catalog.would_launch(&typed)
        || (!catalog.overridden(&typed)
            && !settings::intent(
                catalog.settings,
                &typed,
                catalog.setting_opens,
                catalog.now,
                1,
            )
            .is_empty());

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
    if shortcut == previous.clipboard.shortcut {
        return Err("That shortcut already opens clipboard history".into());
    }
    if shortcut == previous.notes_shortcut {
        return Err("That shortcut already opens notes".into());
    }
    let clip = previous.extra_shortcuts();

    // Registering unregisters the old shortcut first, so every failure path has to put it back.
    let restore_shortcut = || {
        if shortcut_changed
            && let Err(err) = shortcut::register(&app, &previous.summon_shortcut, clip)
        {
            log::error!("couldn't restore the summon shortcut: {err}");
        }
    };

    if shortcut_changed && let Err(err) = shortcut::register(&app, &shortcut, clip) {
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
pub fn set_bar_height(app: AppHandle, height: f64, width: Option<f64>) -> Result<(), String> {
    window::set_bar_height(&app, height, width)
}

#[tauri::command]
pub fn hide_bar(app: AppHandle) {
    window::dismiss_bar(&app);
}

#[tauri::command]
pub fn show_bar(app: AppHandle) {
    window::show_bar(&app);
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

/// The Settings page's first frame after being shown, for the open-time log.
#[tauri::command]
pub fn settings_painted() {
    window::settings_painted();
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<String, String> {
    updater::check_for_updates(&app).await
}

/// What the update card shows now, for a card that just opened.
#[tauri::command]
pub fn update_status() -> Option<updater::Status> {
    updater::current()
}

#[tauri::command]
pub fn update_install_now(app: AppHandle) {
    updater::install_now(&app);
}

#[tauri::command]
pub fn update_later(app: AppHandle) {
    updater::later(&app);
}

/// Dev builds only: plays the update card through with a made-up update.
#[tauri::command]
pub fn rehearse_update(app: AppHandle) -> Result<(), String> {
    #[cfg(debug_assertions)]
    {
        updater::rehearse(&app);
        Ok(())
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = app;
        Err("Only dev builds can rehearse an update".into())
    }
}

/// Starts the app at sign-in, or stops doing so. On Windows that's a logon task in Task
/// Scheduler (see login_task.rs); the Run-key entry earlier versions made is removed, since it
/// started several seconds late and would now start a second copy. Dev builds leave this to
/// the installed app, so the login item never points at a debug build.
pub fn apply_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Ok(());
    }
    #[cfg(windows)]
    {
        let autostart = app.autolaunch();
        if autostart.is_enabled().unwrap_or(false)
            && let Err(err) = autostart.disable()
        {
            log::warn!("couldn't remove the old startup entry: {err}");
        }
        crate::login_task::set(enabled)
    }
    #[cfg(not(windows))]
    autolaunch(app, enabled)
}

#[cfg(not(windows))]
fn autolaunch(app: &AppHandle, enabled: bool) -> Result<(), String> {
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
    history: Option<Vec<ai::ChatTurn>>,
    on_event: Channel<AiEvent>,
) -> Result<(), String> {
    let settings = state.snapshot()?.ai;
    let history = history.unwrap_or_default();
    ai::ask(&settings, &history, &question, |event| {
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

// Sign in with ChatGPT

#[tauri::command]
pub fn chatgpt_account() -> Option<crate::chatgpt::Account> {
    crate::chatgpt::account()
}

/// Signs in through the browser, then makes ChatGPT the AI provider.
#[tauri::command]
pub async fn chatgpt_sign_in(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<crate::chatgpt::Account, String> {
    let opener = app.clone();
    let account = crate::chatgpt::sign_in(|url| {
        opener
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|err| format!("Couldn't open the browser: {err}"))
    })
    .await?;
    // Without the plan, ChatGPT can't answer, so the AI stays as it was.
    if !account.plan {
        return Ok(account);
    }
    let snapshot = state.update(|persisted| {
        let mut settings = persisted.ai.clone();
        if settings.provider != ai::CHATGPT {
            settings.provider = ai::CHATGPT.into();
            settings.model = String::new();
        }
        persisted.set_ai(settings)
    })?;
    publish(&app, snapshot)?;
    Ok(account)
}

#[tauri::command]
pub fn chatgpt_cancel_sign_in() {
    crate::chatgpt::cancel_sign_in();
}

#[tauri::command]
pub async fn chatgpt_sign_out() -> Result<(), String> {
    crate::chatgpt::sign_out().await
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

// Clipboard history

#[tauri::command]
pub fn clip_list(query: String, filter: String) -> clipboard::ClipList {
    clipboard::list(&query, &filter, 300)
}

#[tauri::command]
pub fn clip_detail(id: u64) -> Result<clipboard::ClipDetail, String> {
    clipboard::detail(id)
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

#[tauri::command]
pub fn clip_pin(id: u64, pinned: bool) -> Result<(), String> {
    clipboard::set_pinned(id, pinned)
}

#[tauri::command]
pub fn clip_delete(id: u64) -> Result<(), String> {
    clipboard::delete(id)
}

#[tauri::command]
pub fn clip_clear(pinned_too: bool) -> Result<usize, String> {
    clipboard::clear(pinned_too)
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

#[tauri::command]
pub fn open_clipboard(app: AppHandle) {
    window::show_clipboard(&app);
}

// Notes

#[tauri::command]
pub fn notes_list(query: String) -> Vec<notes::NoteSummary> {
    notes::list(&query)
}

#[tauri::command]
pub fn note_get(id: String) -> Result<notes::Note, String> {
    notes::get(&id)
}

#[tauri::command]
pub fn note_create(body: String) -> Result<String, String> {
    notes::create(&body)
}

#[tauri::command]
pub fn note_save(id: String, body: String) -> Result<i64, String> {
    notes::save(&id, &body)
}

#[tauri::command]
pub fn note_delete(id: String) -> Result<(), String> {
    notes::delete(&id)
}

#[tauri::command]
pub fn reveal_notes(app: AppHandle) -> Result<(), String> {
    app.opener()
        .open_path(notes::folder()?, None::<&str>)
        .map_err(|err| err.to_string())
}

/// Opens the notes window, on `id` when given, or on a new note holding `body`.
// Async for the same reason as open_settings: it may build a webview.
#[tauri::command]
pub async fn open_notes(
    app: AppHandle,
    id: Option<String>,
    body: Option<String>,
) -> Result<(), String> {
    let id = match (id, body) {
        (Some(id), _) => Some(id),
        (None, Some(body)) => Some(notes::create(&body)?),
        (None, None) => None,
    };
    window::hide_bar(&app);
    window::open_notes(&app, id.as_deref())
}

/// From a note's window: opens `id`, or a new note when none is given, in a window of its
/// own (or brings its window forward). Returns the note's id.
// Async for the same reason as open_settings: it may build a webview.
#[tauri::command]
pub async fn open_note_window(app: AppHandle, id: Option<String>) -> Result<String, String> {
    let id = match id {
        Some(id) => id,
        None => notes::create("")?,
    };
    crate::note_windows::open(&app, &id, true)?;
    Ok(id)
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

/// A settings or notes page has drawn and can be shown.
#[tauri::command]
pub fn page_ready(app: AppHandle, window: tauri::WebviewWindow, ms: Option<u64>) {
    window::page_ready(&app, window.label(), ms);
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

// Claude jobs

#[tauri::command]
pub fn claude_jobs() -> Vec<crate::claude::Job> {
    crate::claude::jobs()
}

#[tauri::command]
pub fn claude_approvals() -> Vec<crate::claude::hook::Approval> {
    crate::claude::hook::approvals()
}

/// `!claude [alias] task`: starts a job in the aliased project, or the last one used.
#[tauri::command]
pub fn claude_submit(
    app: AppHandle,
    state: State<'_, AppState>,
    input: String,
    project_id: Option<String>,
) -> Result<String, String> {
    let config = state.snapshot()?.claude;
    let (routed, task) = crate::claude::route(&config, &input);
    let project = match project_id {
        Some(id) => config.projects.iter().find(|project| project.id == id),
        None => routed,
    }
    .ok_or("Add a project in Settings > Claude first")?;
    let task = if task.is_empty() { input.trim() } else { task };
    let id = crate::claude::submit(&app, &project.id, task)?;
    window::dismiss_bar(&app);
    Ok(id)
}

#[tauri::command]
pub fn claude_follow_up(app: AppHandle, job_id: String, prompt: String) -> Result<(), String> {
    crate::claude::follow_up(&app, &job_id, &prompt)?;
    window::dismiss_bar(&app);
    Ok(())
}

#[tauri::command]
pub fn claude_cancel(app: AppHandle, job_id: String) -> Result<(), String> {
    crate::claude::cancel(&app, &job_id)
}

#[tauri::command]
pub fn claude_remove(app: AppHandle, job_id: String) -> Result<(), String> {
    crate::claude::remove(&app, &job_id)
}

#[tauri::command]
pub fn claude_open_terminal(app: AppHandle, job_id: String) -> Result<(), String> {
    crate::claude::open_in_terminal(&app, &job_id)
}

/// `decision`: "allow", "always" or "deny" (with an optional reason).
#[tauri::command]
pub fn claude_answer(
    app: AppHandle,
    approval_id: String,
    decision: String,
    reason: Option<String>,
) -> Result<(), String> {
    use crate::claude::hook::Decision;
    let decision = match decision.as_str() {
        "allow" => Decision::Allow,
        "always" => Decision::AllowAlways,
        "deny" => Decision::Deny(reason.unwrap_or_default().trim().to_string()),
        _ => return Err("Unknown answer".into()),
    };
    crate::claude::hook::answer(&app, &approval_id, decision)
}

#[tauri::command]
pub async fn claude_status(
    state: State<'_, AppState>,
) -> Result<crate::claude::binary::CliStatus, String> {
    let configured = state.snapshot()?.claude.binary;
    tauri::async_runtime::spawn_blocking(move || crate::claude::binary::status(&configured))
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn save_claude_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: crate::claude::ClaudeSettings,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| persisted.set_claude(settings))?;
    publish(&app, snapshot)
}

#[tauri::command]
pub fn save_resume_seconds(
    app: AppHandle,
    state: State<'_, AppState>,
    seconds: u32,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| {
        persisted.resume_seconds = seconds.min(24 * 60 * 60);
        Ok(())
    })?;
    publish(&app, snapshot)
}

/// What typed text like "shutdown in 30m" would do, for the bar's Power row.
#[tauri::command]
pub fn power_preview(query: String) -> Option<crate::power::Preview> {
    crate::power::preview(&query, chrono::Local::now())
}

/// Runs a power action now, or schedules it for `at` (unix seconds).
#[tauri::command]
pub fn power_run(
    app: AppHandle,
    action: crate::power::Action,
    at: Option<i64>,
) -> Result<(), String> {
    if at.is_none() {
        window::hide_bar(&app);
    }
    crate::power::run(&app, action, at)
}

#[tauri::command]
pub fn power_cancel(app: AppHandle) {
    crate::power::cancel(&app);
}

/// Opens a web address (a bookmark) in the default browser and closes the bar.
#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Only web addresses open from here".into());
    }
    // The browser extension switches to a tab already on the page, with no new tab.
    // When it isn't connected, the page opens through the local hand-off link, which the
    // extension can still catch (or which simply redirects without it).
    let found = crate::browser::focus(&url);
    if found == Some(true) {
        window::hide_bar(&app);
        return Ok(());
    }
    let port = crate::claude::hook::port();
    let target = if port > 0 && found.is_none() {
        format!(
            "http://127.0.0.1:{port}/zephyr/open?u={}",
            urlencoding::encode(&url)
        )
    } else {
        url
    };
    app.opener()
        .open_url(&target, None::<&str>)
        .map_err(|err| err.to_string())?;
    window::hide_bar(&app);
    Ok(())
}

/// The browser profiles with bookmarks, for Settings > Bookmarks.
#[tauri::command]
pub async fn bookmark_sources() -> Vec<crate::bookmarks::Source> {
    tauri::async_runtime::spawn_blocking(crate::bookmarks::sources)
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub fn save_bookmark_sources(
    app: AppHandle,
    state: State<'_, AppState>,
    disabled: Vec<String>,
) -> Result<Persisted, String> {
    crate::bookmarks::configure(&disabled);
    let snapshot = state.update(|persisted| {
        persisted.bookmarks_disabled = disabled;
        Ok(())
    })?;
    publish(&app, snapshot)
}

/// A site's icon by address (a bookmark), as a `data:` URL.
#[tauri::command]
pub async fn site_icon_url(url: String) -> Option<String> {
    crate::icons::site_icon(&url).await
}

/// Switches the palette; every window repaints from the published snapshot.
#[tauri::command]
pub fn save_theme(
    app: AppHandle,
    state: State<'_, AppState>,
    theme: String,
) -> Result<Persisted, String> {
    if !crate::state::THEMES.contains(&theme.as_str()) {
        return Err(format!("Unknown theme {theme}"));
    }
    let snapshot = state.update(|persisted| {
        persisted.theme = theme;
        persisted.theme_chosen = true;
        Ok(())
    })?;
    window::apply_theme(&app, &snapshot.theme);
    publish(&app, snapshot)
}

/// What's playing now, for a bar that just opened; later changes come as `nowplaying` events.
#[tauri::command]
pub fn now_playing() -> Option<crate::nowplaying::NowPlaying> {
    crate::nowplaying::snapshot()
}

/// Presses play/pause, next or previous on what's playing.
#[tauri::command]
pub fn now_playing_control(control: String) -> Result<(), String> {
    let control = match control.as_str() {
        "toggle" => crate::nowplaying::Control::Toggle,
        "next" => crate::nowplaying::Control::Next,
        "previous" => crate::nowplaying::Control::Previous,
        other => return Err(format!("Unknown media control {other}")),
    };
    crate::nowplaying::control(control);
    Ok(())
}

/// Switches to the app or browser tab playing the song, hiding the bar when it could.
#[tauri::command]
pub async fn now_playing_focus(app: AppHandle) -> Result<bool, String> {
    let Some(playing) = crate::nowplaying::snapshot() else {
        return Ok(false);
    };
    let focused = tauri::async_runtime::spawn_blocking(move || {
        crate::nowplaying::focus_source(&playing.source_app, &playing.title)
    })
    .await
    .map_err(|err| err.to_string())?;
    if focused {
        crate::window::hide_bar(&app);
    }
    Ok(focused)
}

#[tauri::command]
pub fn save_steam_games(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| {
        persisted.steam_games = enabled;
        Ok(())
    })?;
    crate::steam::configure(enabled);
    apps::AppIndex::rescan(apps::index());
    publish(&app, snapshot)
}

#[tauri::command]
pub fn save_show_now_playing(
    app: AppHandle,
    state: State<'_, AppState>,
    show: bool,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| {
        persisted.show_now_playing = show;
        Ok(())
    })?;
    publish(&app, snapshot)
}

/// Runs a command typed in the bar after `>`; output streams through `on_event`.
#[tauri::command]
pub fn shell_run(
    app: AppHandle,
    state: State<'_, AppState>,
    command: String,
    dir: Option<String>,
    on_event: Channel<ShellEvent>,
) -> Result<(), String> {
    let settings = state.snapshot()?.shell;
    shell::run(&settings, &command, dir.as_deref(), move |event| {
        let _ = on_event.send(event);
    })?;
    let snapshot = state.update(|persisted| {
        persisted.shell.record(&command);
        Ok(())
    })?;
    publish(&app, snapshot).map(|_| ())
}

#[tauri::command]
pub fn shell_stop(id: u64) {
    shell::stop(id);
}

#[tauri::command]
pub fn shell_open_terminal(
    app: AppHandle,
    state: State<'_, AppState>,
    command: String,
    dir: Option<String>,
) -> Result<(), String> {
    let settings = state.snapshot()?.shell;
    shell::open_in_terminal(&settings, &command, dir.as_deref())?;
    if !command.trim().is_empty() {
        let snapshot = state.update(|persisted| {
            persisted.shell.record(&command);
            Ok(())
        })?;
        publish(&app, snapshot)?;
    }
    window::hide_bar(&app);
    Ok(())
}

/// Async because finding WSL or Git Bash runs programs on Windows, which would otherwise
/// hold up the main thread while Settings opens.
#[tauri::command]
pub async fn shell_info(state: State<'_, AppState>) -> Result<ShellInfo, String> {
    let settings = state.snapshot()?.shell;
    tauri::async_runtime::spawn_blocking(move || shell::info(&settings))
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn save_shell_program(
    app: AppHandle,
    state: State<'_, AppState>,
    program: String,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| persisted.shell.set_program(&program))?;
    publish(&app, snapshot)
}

#[tauri::command]
pub fn save_shell_terminal(
    app: AppHandle,
    state: State<'_, AppState>,
    terminal: String,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| persisted.shell.set_terminal(&terminal))?;
    publish(&app, snapshot)
}

#[tauri::command]
pub fn clear_shell_history(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| {
        persisted.shell.history.clear();
        Ok(())
    })?;
    publish(&app, snapshot)
}

/// One entry for the running bug capture's log; does nothing when not recording.
#[tauri::command]
pub fn capture_event(kind: String, data: serde_json::Value) {
    crate::capture::record(&kind, data);
}

#[tauri::command]
pub fn capture_recording() -> bool {
    crate::capture::recording()
}

/// Turns the bug capture shortcuts on or off; turning them off ends a running recording.
#[tauri::command]
pub fn save_bug_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| {
        persisted.bug_capture = enabled;
        Ok(())
    })?;
    if !enabled && crate::capture::recording() {
        crate::capture::toggle(&app);
    }
    shortcut::register(&app, &snapshot.summon_shortcut, snapshot.extra_shortcuts())?;
    publish(&app, snapshot)
}

#[tauri::command]
pub fn reveal_captures(app: AppHandle) -> Result<(), String> {
    let folder = crate::capture::folder()?;
    app.opener()
        .open_path(folder.to_string_lossy(), None::<&str>)
        .map_err(|err| err.to_string())
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

/// A web destination's site icon as a `data:` URL, or `None` to keep its letter tile.
#[tauri::command]
pub async fn site_icon(
    state: State<'_, AppState>,
    destination_id: String,
) -> Result<Option<String>, String> {
    let template = state
        .snapshot()?
        .destinations
        .into_iter()
        .find(|destination| {
            destination.id == destination_id
                && destination.kind == crate::destination::DestinationKind::Web
        })
        .map(|destination| destination.url_template);
    Ok(match template {
        Some(template) => crate::icons::site_icon(&template).await,
        None => None,
    })
}

/// An app's icon as a PNG `data:` URL at `size` pixels, or `None` to keep the generic glyph.
#[tauri::command]
pub async fn app_icon(app_id: String, size: u32) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || crate::icons::app_icon(&app_id, size))
        .await
        .ok()
        .flatten()
}

/// Shows the system folder picker; `None` when cancelled.
#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Choose a folder")
            .blocking_pick_folder()
    })
    .await
    .map_err(|err| err.to_string())?;
    Ok(picked
        .and_then(|path| path.into_path().ok())
        .map(|path| path.to_string_lossy().into_owned()))
}

/// Remembers the view the bar should come back to (`claude`, `ai`, `notes`, …; empty for the
/// search bar), so it survives hiding and restarts.
#[tauri::command]
pub fn set_active_mode(state: State<'_, AppState>, mode: String) -> Result<(), String> {
    state.update(|persisted| {
        persisted.active_mode = mode.chars().take(32).collect();
        Ok(())
    })?;
    Ok(())
}

#[tauri::command]
pub fn save_open_behavior(
    app: AppHandle,
    state: State<'_, AppState>,
    return_to_mode: bool,
    route_cmd: String,
    route_alt: String,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| {
        persisted.return_to_mode = return_to_mode;
        persisted.route_cmd = route_cmd.trim().to_string();
        persisted.route_alt = route_alt.trim().to_string();
        Ok(())
    })?;
    publish(&app, snapshot)
}

// Sync (step 1: accounts, device keys, approvals)

#[tauri::command]
pub async fn sync_status() -> Result<crate::sync::Status, String> {
    crate::sync::status().await
}

/// Opens Google sign-in in the browser and finishes when it comes back.
#[tauri::command]
pub async fn sync_google_sign_in(app: AppHandle) -> Result<(), String> {
    crate::sync::google_sign_in(|url| {
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|err| format!("Couldn't open the browser: {err}"))
    })
    .await
}

#[tauri::command]
pub fn sync_cancel_sign_in() -> Result<(), String> {
    crate::sync::cancel_sign_in()
}

#[tauri::command]
pub fn sync_recovery_saved() -> Result<(), String> {
    crate::sync::recovery_saved()
}

#[tauri::command]
pub async fn sync_use_recovery(key: String) -> Result<(), String> {
    crate::sync::use_recovery(&key).await
}

#[tauri::command]
pub async fn sync_poll() -> Result<(), String> {
    crate::sync::poll().await
}

#[tauri::command]
pub async fn sync_pending() -> Result<Vec<crate::sync::Pending>, String> {
    crate::sync::pending().await
}

#[tauri::command]
pub async fn sync_approve(device_id: String) -> Result<(), String> {
    crate::sync::approve(&device_id).await
}

#[tauri::command]
pub async fn sync_revoke(device_id: String) -> Result<(), String> {
    crate::sync::revoke(&device_id).await
}

#[tauri::command]
pub fn sync_recovery_key() -> Result<String, String> {
    crate::sync::recovery_key()
}

#[tauri::command]
pub async fn sync_sign_out() -> Result<(), String> {
    crate::sync::sign_out().await
}

/// Saves a hotkey and alias for one command, app, destination or bookmark (both empty removes
/// it), then registers the hotkeys again.
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
    let (recording, snapshotting) = if current.bug_capture {
        (crate::capture::SHORTCUT, crate::capture::SNAPSHOT_SHORTCUT)
    } else {
        ("", "")
    };
    let taken = [
        (current.summon_shortcut.as_str(), "the bar"),
        (clipboard, "clipboard history"),
        (current.notes_shortcut.as_str(), "notes"),
        (recording, "a bug recording"),
        (snapshotting, "a bug snapshot"),
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
// Async for the same reason as open_settings: it may build the settings webview.
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
/// app or bookmark opens without the bar, and a destination opens the bar armed with it.
pub fn run_binding(app: &AppHandle, target: &str) {
    use tauri::Manager;
    let (kind, value) = target.split_once(':').unwrap_or((target, ""));
    let result = match (kind, value) {
        ("view", "notes") => window::open_notes(app, None),
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
        ("app", id) => launch(app, &app.state::<AppState>(), id).map(|_| ()),
        ("bookmark", url) => open_url(app.clone(), url.to_string()),
        ("group", id) => crate::launch_groups::launch(app, id),
        ("setting", id) => open_setting_page(app, id).map(|_| ()),
        ("dest", id) => {
            window::show_bar(app);
            app.emit_to(
                "main",
                "bar-input",
                serde_json::json!({ "query": "", "destination": id, "run": false }),
            )
            .map_err(|err| err.to_string())
        }
        _ => Err(format!("Nothing to run for {target}")),
    };
    if let Err(err) = result {
        log::error!("couldn't run a shortcut: {err}");
    }
}

/// What a drag out of the bar carries: the files, and a picture for the pointer.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DragOut {
    pub files: Vec<String>,
    pub icon: String,
}

/// Gets a file result (`path`, which must be in the file index) or a clipboard history item
/// (`clip`) ready to drag into another app, and keeps the bar up until the drag ends.
#[tauri::command]
pub fn drag_prepare(
    app: AppHandle,
    state: State<'_, AppState>,
    path: Option<String>,
    clip: Option<u64>,
) -> Result<DragOut, String> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|err| err.to_string())?
        .join("drag");
    let files = match (path, clip) {
        (Some(path), _) => {
            known_file(&state, &path)?;
            vec![path]
        }
        // Earlier drags' copies are done with: whatever they were dropped on has read them.
        (None, Some(id)) => {
            let _ = std::fs::remove_dir_all(&dir);
            clipboard::drag_files(id, &dir)?
        }
        (None, None) => return Err("Nothing to drag".into()),
    };
    let first = files.first().ok_or("Nothing to drag")?;
    let png = crate::icons::file_icon_png(first, 64).ok_or("Couldn't draw the drag picture")?;
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let icon = dir.join("pointer.png");
    std::fs::write(&icon, png).map_err(|err| err.to_string())?;
    window::begin_drag_out();
    Ok(DragOut {
        files,
        icon: icon.to_string_lossy().into_owned(),
    })
}

/// The drag ended: dropped somewhere hides the bar, cancelled keeps it.
#[tauri::command]
pub fn drag_finished(app: AppHandle, dropped: bool) {
    window::end_drag_out(&app, dropped);
}

/// What the bar's preview pane shows for a file result.
#[derive(serde::Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FilePreview {
    Image {
        url: String,
        size: u64,
        modified: i64,
    },
    Text {
        text: String,
        /// More lines follow than are shown.
        truncated: bool,
        size: u64,
        modified: i64,
    },
    Info {
        size: u64,
        modified: i64,
        folder: bool,
    },
}

/// A look at a file result without opening it: a picture, the first lines of a text file,
/// or its size and date. Only for files in the file index.
#[tauri::command]
pub async fn file_preview(state: State<'_, AppState>, path: String) -> Result<FilePreview, String> {
    known_file(&state, &path)?;
    tauri::async_runtime::spawn_blocking(move || preview_of(std::path::Path::new(&path)))
        .await
        .map_err(|err| err.to_string())?
}

fn preview_of(path: &std::path::Path) -> Result<FilePreview, String> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use std::io::Read;

    const IMAGE_LIMIT: u64 = 15 * 1024 * 1024;
    const TEXT_PEEK: u64 = 256 * 1024;
    const LINES: usize = 200;

    let meta = std::fs::metadata(path).map_err(|err| err.to_string())?;
    let size = meta.len();
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    if meta.is_dir() {
        return Ok(FilePreview::Info {
            size: 0,
            modified,
            folder: true,
        });
    }
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let image_type = match extension.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "ico" => Some("image/x-icon"),
        "svg" => Some("image/svg+xml"),
        _ => None,
    };
    if let Some(mime) = image_type
        && size <= IMAGE_LIMIT
    {
        let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
        return Ok(FilePreview::Image {
            url: format!("data:{mime};base64,{}", STANDARD.encode(bytes)),
            size,
            modified,
        });
    }

    // Anything that reads as text: no NUL bytes in its first part.
    let mut peek = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(TEXT_PEEK).read_to_end(&mut peek))
        .map_err(|err| err.to_string())?;
    if !peek.is_empty() && !peek.contains(&0) {
        let text = String::from_utf8_lossy(&peek);
        let mut lines: Vec<&str> = text.lines().take(LINES + 1).collect();
        let truncated = lines.len() > LINES || size > TEXT_PEEK;
        lines.truncate(LINES);
        return Ok(FilePreview::Text {
            text: lines.join("\n"),
            truncated,
            size,
            modified,
        });
    }
    Ok(FilePreview::Info {
        size,
        modified,
        folder: false,
    })
}

/// macOS: opens Quick Look on a file result (Space-style preview), from the file index only.
#[tauri::command]
pub fn quick_look(state: State<'_, AppState>, path: String) -> Result<(), String> {
    known_file(&state, &path)?;
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("qlmanage")
            .args(["-p", &path])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|err| err.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Quick Look is only on macOS".into())
    }
}
