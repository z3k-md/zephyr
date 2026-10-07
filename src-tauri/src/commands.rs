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
pub fn clip_stats() -> std::collections::HashMap<&'static str, usize> {
    clipboard::stats()
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
    remember_notes_mode(&app);
    window::hide_bar(&app);
    window::open_notes(&app, id.as_deref())
}

/// Opening notes makes them the place Zephyr returns to.
pub fn remember_notes_mode(app: &AppHandle) {
    use tauri::Manager;
    let _ = app.state::<AppState>().update(|persisted| {
        persisted.active_mode = "notes".into();
        Ok(())
    });
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

/// Remembers the view Zephyr should come back to (`claude`, `ai`, `notes`, …; empty for the
/// search bar), so it survives hiding and restarts.
#[tauri::command]
pub fn set_active_mode(state: State<'_, AppState>, mode: String) -> Result<(), String> {
    state.update(|persisted| {
        persisted.active_mode = mode.chars().take(32).collect();
        Ok(())
    })?;
    Ok(())
}

/// Leaves Notes as the sticky mode: hides the notes window and brings back the bar.
#[tauri::command]
pub fn leave_notes(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    use tauri::Manager;
    state.update(|persisted| {
        persisted.active_mode.clear();
        Ok(())
    })?;
    if let Some(notes) = app.get_webview_window("notes") {
        let _ = notes.hide();
    }
    window::show_bar(&app);
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
