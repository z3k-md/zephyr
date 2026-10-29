use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use super::record;
use crate::apps::{self, Catalog};
use crate::files;
use crate::query::{self, Decision, DispatchOutcome};
use crate::settings::{self, Target};
use crate::state::{AppState, Persisted};
use crate::suggest::{self, SuggestResponse};
use crate::util::now_secs;
use crate::window;

/// A view was opened, for the empty bar's Views to follow what's used.
#[tauri::command]
pub fn record_view_open(app: AppHandle, state: State<'_, AppState>, view: String) {
    if view.is_empty() || view.len() > 32 {
        return;
    }
    record(&app, &state, "the view", |persisted| {
        persisted.record_view_open(&view, now_secs());
    });
}

#[tauri::command]
pub async fn suggest(
    state: State<'_, AppState>,
    query: String,
    include_remote: bool,
) -> Result<SuggestResponse, String> {
    let snapshot = state.snapshot()?;
    let installed = apps::index().snapshot();
    let catalog = catalog(&installed, &snapshot);
    let mut response = suggest::gather(&query, &snapshot.history, catalog, include_remote).await;
    crate::running::annotate(&mut response, &installed, &query);
    Ok(response)
}

/// Connects to Google's suggestion service when the bar opens, so the first keystroke's
/// suggestions aren't slow.
#[tauri::command]
pub async fn warm_suggestions() {
    suggest::warm().await;
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

// Async: opening an app setting may build the settings webview, which deadlocks a sync
// command on Windows (see open_settings).
#[tauri::command]
pub async fn open_setting(app: AppHandle, setting_id: String) -> Result<DispatchOutcome, String> {
    open_setting_page(&app, &setting_id)
}

pub(super) fn open_setting_page(
    app: &AppHandle,
    setting_id: &str,
) -> Result<DispatchOutcome, String> {
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
    record(app, &app.state::<AppState>(), "the setting", |persisted| {
        persisted.record_setting_open(setting.id, now_secs())
    });
    log::info!("opened setting {}", setting.id);
    Ok(DispatchOutcome::SettingOpened {
        setting_id: setting.id.to_string(),
    })
}

/// Installed apps matching `text` as the bar ranks them, for Settings' app picker; with no
/// text, the most launched.
#[tauri::command]
pub fn search_apps(
    state: State<'_, AppState>,
    text: String,
    limit: usize,
) -> Result<Vec<(String, String)>, String> {
    let snapshot = state.snapshot()?;
    let installed = apps::index().snapshot();
    let now = now_secs();
    let text = text.trim();
    let found: Vec<&apps::App> = if text.is_empty() {
        apps::recent(&installed, &snapshot.launches, now, limit)
    } else {
        apps::ranked(&installed, &snapshot.launches, text, now, limit)
            .into_iter()
            .map(|(app, _)| app)
            .collect()
    };
    Ok(found
        .into_iter()
        .map(|app| (app.id.clone(), app.name.clone()))
        .collect())
}

#[tauri::command]
pub async fn dispatch(
    app: AppHandle,
    state: State<'_, AppState>,
    query: String,
    interpret: bool,
) -> Result<DispatchOutcome, String> {
    let snapshot = state.snapshot()?;
    let installed = apps::index().snapshot();
    let catalog = catalog(&installed, &snapshot);
    let decision = query::decide(&query, interpret, catalog);
    // Text that would have launched an app but went to the web from its row was meant for
    // the web; remember it so plain Enter stops launching the app for it.
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

    let (text, url, link) = match decision {
        Decision::Search { query } => {
            let url = crate::web::search_url(&query);
            (query, url, false)
        }
        Decision::Link { url } => (url.clone(), url, true),
        Decision::Launch { app_id } => return super::apps::launch(&app, &state, &app_id),
        Decision::OpenSetting { setting_id } => return open_setting_page(&app, &setting_id),
        Decision::OpenFile { path } => return super::files::open_file_path(&app, &state, &path),
        Decision::Empty => return Ok(DispatchOutcome::Empty),
        Decision::Rejected { message } => return Err(message),
    };
    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|err| format!("Couldn't open it: {err}"))?;
    window::hide_bar(&app);
    record(&app, &state, "history", |persisted| {
        persisted.record(&text, link, now_secs());
        if overrode_app && !link {
            persisted.record_app_override(&typed);
        }
    });
    log::info!("opened a {}", if link { "link" } else { "search" });
    Ok(DispatchOutcome::Opened)
}

/// Opens a web address (a bookmark) in the default browser and closes the bar.
#[tauri::command]
pub async fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || open_url_now(&app, url))
        .await
        .map_err(|err| err.to_string())?
}

pub(super) fn open_url_now(app: &AppHandle, url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Only web addresses open from here".into());
    }
    // The browser extension switches to a tab already on the page, with no new tab.
    // When it isn't connected, the page opens through the local hand-off link, which the
    // extension can still catch (or which simply redirects without it).
    let found = crate::browser::focus(&url);
    if found == Some(true) {
        window::hide_bar(app);
        return Ok(());
    }
    let port = crate::handoff::port();
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
    window::hide_bar(app);
    Ok(())
}

/// A site's icon by address (a bookmark), as a `data:` URL.
#[tauri::command]
pub async fn site_icon_url(url: String) -> Option<String> {
    crate::icons::site_icon(&url).await
}

/// An app's icon as a PNG `data:` URL at `size` pixels, or `None` to keep the generic glyph.
#[tauri::command]
pub async fn app_icon(app_id: String, size: u32) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || crate::icons::app_icon(&app_id, size))
        .await
        .ok()
        .flatten()
}
