use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

use super::{blocking, publish, save, save_now};
use crate::files;
use crate::shortcut;
use crate::state::{AppState, Persisted};
use crate::window;

/// Opens first-run setup again (Settings › General › Run setup again).
#[tauri::command]
pub async fn open_onboarding(app: AppHandle) -> Result<(), String> {
    window::open_onboarding(&app)
}

/// Finishing or skipping setup: it won't show again (except with the dev switch).
#[tauri::command]
pub async fn finish_onboarding(app: AppHandle) -> Result<Persisted, String> {
    let snapshot = save(app.clone(), |persisted| {
        persisted.onboarding_done = true;
        Ok(())
    })
    .await?;
    if let Some(window) = app.get_webview_window("onboarding") {
        let _ = window.hide();
        window::regular_while_open(&app);
        let _ = window.close();
    }
    Ok(snapshot)
}

/// Dev builds: show setup on every launch.
#[tauri::command]
pub async fn save_onboarding_every_launch(
    app: AppHandle,
    every: bool,
) -> Result<Persisted, String> {
    save(app, move |persisted| {
        persisted.onboarding_every_launch = every;
        Ok(())
    })
    .await
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
            let snapshot = save(app.clone(), |persisted| {
                persisted.protected_folders = true;
                Ok(())
            })
            .await?;
            files::index().configure(snapshot.file_config(), super::files::file_cache_path(&app));
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

// Async: turning startup on or off runs schtasks on Windows, which takes a moment.
#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    summon_shortcut: String,
    launch_at_startup: bool,
) -> Result<Persisted, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let shortcut = shortcut::canonical_shortcut(&summon_shortcut)?;
        let previous = state.snapshot()?;
        let shortcut_changed = shortcut != previous.summon_shortcut;
        previous.check_summon_shortcut(&shortcut)?;
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
        if let Err(err) = apply_autostart(&app, launch_at_startup, previous.instant_startup) {
            restore_shortcut();
            return Err(err);
        }

        match state.update(|persisted| {
            persisted.set_general(shortcut.clone(), launch_at_startup);
            Ok(())
        }) {
            Ok(snapshot) => publish(&app, snapshot),
            Err(err) => {
                restore_shortcut();
                let _ = apply_autostart(&app, previous.launch_at_startup, previous.instant_startup);
                Err(err)
            }
        }
    })
    .await
}

/// Windows: start from a logon task the moment you sign in, instead of the startup list
/// Windows holds back for a few seconds. Opt-in, since Defender may flag it.
#[tauri::command]
pub async fn save_instant_startup(app: AppHandle, enabled: bool) -> Result<Persisted, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let current = state.snapshot()?;
        apply_autostart(&app, current.launch_at_startup, enabled)?;
        save_now(&app, |persisted| {
            persisted.instant_startup = enabled;
            Ok(())
        })
    })
    .await
}

/// Starts the app at sign-in, or stops doing so. Normally that's the system's startup list
/// (the Run key on Windows). `instant` (Windows only, opt-in) uses a Task Scheduler logon
/// task instead, which starts right at sign-in, skipping Windows' delay for the startup
/// list; Defender's behavior scan can mistake that for malware persistence, so it's never
/// the default. Only one of the two is ever set up. Dev builds leave this to the installed
/// app, so the login item never points at a debug build.
pub fn apply_autostart(app: &AppHandle, enabled: bool, instant: bool) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Ok(());
    }
    #[cfg(windows)]
    {
        crate::login_task::set(enabled && instant)?;
        autolaunch(app, enabled && !instant)
    }
    #[cfg(not(windows))]
    {
        let _ = instant;
        autolaunch(app, enabled)
    }
}

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

#[tauri::command]
pub async fn save_resume_seconds(app: AppHandle, seconds: u32) -> Result<Persisted, String> {
    save(app, move |persisted| {
        persisted.resume_seconds = seconds.min(24 * 60 * 60);
        Ok(())
    })
    .await
}

/// Switches the palette; every window repaints from the published snapshot.
#[tauri::command]
pub async fn save_theme(app: AppHandle, theme: String) -> Result<Persisted, String> {
    if !crate::state::THEMES.contains(&theme.as_str()) {
        return Err(format!("Unknown theme {theme}"));
    }
    save(app, |persisted| {
        persisted.theme = theme;
        persisted.theme_chosen = true;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn save_open_behavior(app: AppHandle, return_to_mode: bool) -> Result<Persisted, String> {
    save(app, move |persisted| {
        persisted.return_to_mode = return_to_mode;
        Ok(())
    })
    .await
}
