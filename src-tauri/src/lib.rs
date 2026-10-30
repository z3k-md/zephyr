mod answer;
mod apps;
mod bindings;
mod bookmarks;
mod browser;
mod capture;
mod clipboard;
mod commands;
mod cputemp;
mod deeplink;
mod files;
mod gpu;
mod handoff;
mod history;
mod icons;
mod launch_groups;
mod logger;
mod login_task;
mod loopback;
mod net;
mod note_windows;
mod notes;
mod nowplaying;
mod permissions;
mod power;
mod query;
mod resources;
mod running;
mod secrets;
mod settings;
mod shell;
mod shortcut;
mod startup;
mod state;
mod stats;
mod steam;
mod stocks;
mod suggest;
mod sync;
mod temps;
mod timer;
mod tray;
mod updater;
mod util;
mod weather;
mod web;
mod window;

use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::ShortcutState;

use crate::state::{AppState, Persisted};

/// Run instead of the app when started as the CPU temperature helper (cputemp.rs): returns
/// whether it was, after the helper has finished.
pub fn run_helper_if_asked() -> bool {
    if !std::env::args().any(|arg| arg == cputemp::HELPER_ARG) {
        return false;
    }
    cputemp::helper();
    true
}

pub fn init_logging() {
    startup::begin();
    logger::init();
}

pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());
    builder
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            window::show_bar(app);
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, pressed, event| {
                    if event.state != ShortcutState::Pressed {
                        return;
                    }
                    if shortcut::is_clipboard(pressed) {
                        window::show_clipboard(app);
                    } else if shortcut::is_notes(pressed) {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Err(err) = note_windows::show(&app, None) {
                                log::error!("couldn't open notes: {err}");
                            }
                        });
                    } else if let Some(target) = shortcut::binding_for(pressed) {
                        // Off the main thread, on one that may block: a note or a launch may
                        // build a window, and a bookmark waits on the browser extension.
                        let app = app.clone();
                        tauri::async_runtime::spawn_blocking(move || {
                            commands::shortcuts::run_binding(&app, &target);
                        });
                    } else {
                        window::toggle_bar(app);
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_drag::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(setup)
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::search::record_view_open,
            commands::search::suggest,
            commands::search::warm_suggestions,
            commands::search::open_setting,
            commands::search::search_apps,
            commands::search::dispatch,
            commands::search::open_url,
            commands::search::site_icon_url,
            commands::search::app_icon,
            commands::apps::launch_app,
            commands::apps::window_focus,
            commands::apps::window_close,
            commands::apps::app_quit,
            commands::apps::save_launch_groups,
            commands::apps::launch_group,
            commands::files::open_file,
            commands::files::reveal_file,
            commands::files::file_index_status,
            commands::files::rebuild_file_index,
            commands::files::save_file_folders,
            commands::files::allow_protected_folders,
            commands::files::pick_folder,
            commands::files::drag_prepare,
            commands::files::drag_finished,
            commands::files::file_preview,
            commands::files::quick_look,
            commands::resources::system_stats,
            commands::resources::resources_snapshot,
            commands::resources::resources_closed,
            commands::resources::resources_quit,
            commands::resources::resources_relaunch,
            commands::resources::resources_switch,
            commands::resources::resources_reveal,
            commands::resources::save_show_stats,
            commands::resources::resources_detail,
            commands::resources::resources_processes,
            commands::resources::resources_cores,
            commands::resources::resources_memory,
            commands::resources::save_cpu_temp,
            commands::resources::cpu_temp_status,
            commands::bar::set_bar_height,
            commands::bar::hide_bar,
            commands::bar::open_settings,
            commands::bar::hide_settings,
            commands::bar::window_chrome,
            commands::bar::page_ready,
            commands::bar::save_typing_result,
            commands::bar::prepare_grid,
            commands::bar::start_bar_drag,
            commands::bar::bar_move,
            commands::bar::bar_move_end,
            commands::bar::set_active_mode,
            commands::settings::open_onboarding,
            commands::settings::finish_onboarding,
            commands::settings::save_onboarding_every_launch,
            commands::settings::permissions_status,
            commands::settings::permission_request,
            commands::settings::save_settings,
            commands::settings::save_instant_startup,
            commands::settings::save_resume_seconds,
            commands::settings::save_theme,
            commands::settings::save_open_behavior,
            commands::stocks::quote,
            commands::weather::weather,
            commands::weather::weather_places,
            commands::weather::weather_warm,
            commands::weather::weather_at,
            commands::weather::save_weather_place,
            commands::weather::save_weather_by_ip,
            commands::weather::save_weather_unit,
            commands::updates::check_for_updates,
            commands::updates::update_status,
            commands::updates::update_waiting,
            commands::updates::update_install_now,
            commands::updates::update_later,
            commands::updates::rehearse_update,
            commands::clipboard::clip_list,
            commands::clipboard::clip_detail,
            commands::clipboard::clip_paste,
            commands::clipboard::clip_copy,
            commands::clipboard::clip_pin,
            commands::clipboard::clip_delete,
            commands::clipboard::clip_clear,
            commands::clipboard::clip_can_paste,
            commands::clipboard::clip_request_paste_permission,
            commands::clipboard::clip_stats,
            commands::clipboard::save_clipboard_settings,
            commands::notes::notes_list,
            commands::notes::note_get,
            commands::notes::note_create,
            commands::notes::note_save,
            commands::notes::note_delete,
            commands::notes::reveal_notes,
            commands::notes::open_notes,
            commands::notes::open_note_window,
            commands::notes::keep_note_window,
            commands::notes::close_note_window,
            commands::notes::save_notes_shortcut,
            commands::shortcuts::pause_shortcuts,
            commands::shortcuts::save_binding,
            commands::shortcuts::binding_failures,
            commands::shortcuts::edit_binding,
            commands::shortcuts::take_pending_binding,
            commands::power::power_preview,
            commands::power::power_run,
            commands::power::power_cancel,
            commands::media::now_playing,
            commands::media::now_playing_control,
            commands::media::now_playing_volume,
            commands::media::now_playing_focus,
            commands::shell::shell_run,
            commands::shell::shell_stop,
            commands::shell::shell_open_terminal,
            commands::shell::shell_info,
            commands::shell::save_shell_program,
            commands::capture::capture_event,
            commands::capture::capture_recording,
            commands::capture::capture_toggle,
            commands::capture::capture_snapshot,
            commands::capture::save_bug_capture,
            commands::capture::reveal_captures,
            commands::sync::sync_status,
            commands::sync::sync_google_sign_in,
            commands::sync::sync_cancel_sign_in,
            commands::sync::sync_recovery_saved,
            commands::sync::sync_use_recovery,
            commands::sync::sync_poll,
            commands::sync::sync_pending,
            commands::sync::sync_approve,
            commands::sync::sync_revoke,
            commands::sync::sync_recovery_key,
            commands::sync::sync_sign_out,
            commands::net::network_summary,
            commands::net::network_history,
            commands::net::network_public_ip,
            commands::net::save_watch_connection,
            commands::net::speed_test,
            commands::net::speed_test_cancel,
            commands::net::network_addresses,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Zephyr")
        .run(|_app, event| match event {
            tauri::RunEvent::Ready => {
                startup::mark("event loop running, the summon shortcut works");
            }
            // Tray Quit and an update's restart both come through here.
            tauri::RunEvent::Exit => {
                clipboard::flush();
                shell::stop_all();
            }
            _ => {}
        });
}

/// Everything before the bar can open, in order: settings first, then the summon shortcut
/// (as early as possible, so it works the moment the event loop runs), the background
/// services, and last the windows.
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // A tray utility should not put an icon in the Dock or take over the menu bar.
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);

    startup::mark("setup began");
    move_from_old_identifier(app.handle());
    shell::clean_terminal_scripts();
    let config_dir = app.path().app_config_dir()?;
    let state = AppState::load(config_dir.join("state.json"));
    let snapshot = state
        .snapshot()
        .map_err(Box::<dyn std::error::Error>::from)?;

    register_shortcuts(app.handle(), &snapshot);
    apply_autostart_later(app.handle(), &snapshot);
    start_monitors(app.handle(), &snapshot);
    open_stores(app, &config_dir, &snapshot)?;

    app.manage(state);
    handoff::start();
    power::resume(app.handle());
    browser::start();
    nowplaying::start(app.handle());

    if let Err(err) = tray::init(app) {
        log::error!("tray setup failed: {err}");
    }

    open_first_windows(app.handle(), &snapshot);
    set_up_bar(app.handle());

    // Notes reopen after startup rather than holding it up.
    let notes_app = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        note_windows::restore(&notes_app);
    });
    register_links(app);

    startup::mark("setup finished");
    log::info!("Zephyr is running");

    apps::AppIndex::refresh_if_stale(apps::index());
    files::index().configure(
        snapshot.file_config(),
        commands::files::file_cache_path(app.handle()),
    );
    updater::spawn_background_checks(app.handle().clone());
    Ok(())
}

fn register_shortcuts(app: &tauri::AppHandle, snapshot: &Persisted) {
    if let Err(err) = shortcut::register(app, &snapshot.summon_shortcut, snapshot.extra_shortcuts())
    {
        log::error!("{err}");
    }
    startup::mark("summon shortcut registered");
}

/// Off the startup path: on Windows it runs schtasks, which takes a moment.
fn apply_autostart_later(app: &tauri::AppHandle, snapshot: &Persisted) {
    let app = app.clone();
    let (launch_at_startup, instant) = (snapshot.launch_at_startup, snapshot.instant_startup);
    std::thread::spawn(move || {
        if let Err(err) = commands::settings::apply_autostart(&app, launch_at_startup, instant) {
            log::error!("{err}");
        }
    });
}

fn start_monitors(app: &tauri::AppHandle, snapshot: &Persisted) {
    // The 15 minutes of totals System Resources graphs is always being kept, and the
    // connection watched unless Settings turned that off.
    net::set_enabled(snapshot.watch_connection);
    net::start();
    cputemp::start(snapshot.cpu_temp);
    stats::start(app.clone());
    // The settings catalog reads every System Settings pane's plist on macOS (most of a
    // second); built now, the first search doesn't wait on it.
    std::thread::spawn(|| {
        settings::catalog();
    });
}

/// Points each part that keeps files at its folder, and loads what it needs at once.
fn open_stores(
    app: &tauri::App,
    config_dir: &std::path::Path,
    snapshot: &Persisted,
) -> Result<(), Box<dyn std::error::Error>> {
    secrets::init(app.path().app_data_dir()?);
    sync::init(app.path().app_data_dir()?);
    notes::init(config_dir.join("notes"));
    icons::init(app.path().app_cache_dir()?.join("icons"));
    capture::init(app.path().app_data_dir()?.join("captures"));
    weather::start(app.handle(), app.path().app_data_dir()?);
    // Dev builds keep their key in a file instead of the keychain (secrets.rs), so
    // they keep their own history too; sharing one wiped it at every switch.
    let clipboard_dir = if cfg!(debug_assertions) {
        "clipboard-dev"
    } else {
        "clipboard"
    };
    clipboard::start(
        app.path().app_data_dir()?.join(clipboard_dir),
        snapshot.clipboard.clone(),
    );
    Ok(())
}

fn open_first_windows(app: &tauri::AppHandle, snapshot: &Persisted) {
    window::prewarm_settings(app);
    // First run (or every launch in dev, when switched on): setup before first use.
    let show_setup =
        !snapshot.onboarding_done || (cfg!(debug_assertions) && snapshot.onboarding_every_launch);
    if show_setup && let Err(err) = window::open_onboarding(app) {
        log::error!("couldn't open setup: {err}");
    }
}

/// Dresses the bar's window for its platform and hides it until it's summoned.
fn set_up_bar(app: &tauri::AppHandle) {
    let Some(main_window) = app.get_webview_window("main") else {
        return;
    };
    #[cfg(target_os = "macos")]
    if let Err(err) = window::make_bar_panel(&main_window) {
        log::error!("couldn't let the bar float over full-screen apps: {err}");
    }
    #[cfg(target_os = "macos")]
    window::hide_on_space_change(app);
    #[cfg(windows)]
    if let Err(err) = window::make_bar_glass(&main_window) {
        log::error!("couldn't blur behind the bar: {err}");
    }
    #[cfg(windows)]
    window::install_snapping(&main_window);
    #[cfg(windows)]
    clipboard::set_owner_window(&main_window);

    let watched = main_window.clone();
    main_window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            // Alt+F4 hides the bar like Esc does, so stats and the page hear about it.
            window::hide_bar(watched.app_handle());
        }
        WindowEvent::Focused(false) if !window::suppressing_blur() => {
            log::debug!("the bar lost focus, hiding it");
            let _ = watched.hide();
            stats::set_bar_visible(false);
            let _ = watched.emit("bar-hidden", ());
        }
        WindowEvent::Moved(position) => {
            window::bar_moved(watched.app_handle(), *position);
        }
        _ => {}
    });
    let _ = main_window.hide();
    window::draw_bar_unseen(app);
}

/// Routes `zephyr://` links to the bar, including the one that launched Zephyr.
fn register_links(app: &tauri::App) {
    use tauri_plugin_deep_link::DeepLinkExt;

    // macOS reads the scheme from the installed bundle; Windows and Linux register at runtime.
    #[cfg(any(windows, target_os = "linux"))]
    if let Err(err) = app.deep_link().register_all() {
        log::error!("couldn't register zephyr:// links: {err}");
    }
    let handle = app.handle().clone();
    app.deep_link().on_open_url(move |event| {
        for url in event.urls() {
            deeplink::handle(&handle, &url);
        }
    });
    if let Ok(Some(urls)) = app.deep_link().get_current() {
        for url in urls {
            deeplink::handle(app.handle(), &url);
        }
    }
}

/// The bundle id was `com.zephyr.app` through 2.8, and Tauri names the app's folders after it.
/// Whatever is still in the old folders moves into the new ones, entry by entry, since
/// Windows may have made a new folder (the webview's) before this runs. Keychain entries
/// are named separately and don't move.
fn move_from_old_identifier(app: &tauri::AppHandle) {
    const OLD: &str = "com.zephyr.app";
    let resolver = app.path();
    let mut folders = std::collections::BTreeSet::new();
    for folder in [
        resolver.app_config_dir(),
        resolver.app_data_dir(),
        resolver.app_local_data_dir(),
        resolver.app_cache_dir(),
    ]
    .into_iter()
    .flatten()
    {
        folders.insert(folder);
    }
    for new in folders {
        let old = new.with_file_name(OLD);
        let Ok(entries) = std::fs::read_dir(&old) else {
            continue;
        };
        if let Err(err) = std::fs::create_dir_all(&new) {
            log::error!("couldn't make {}: {err}", new.display());
            continue;
        }
        for entry in entries.flatten() {
            let target = new.join(entry.file_name());
            if target.exists() {
                continue;
            }
            if let Err(err) = std::fs::rename(entry.path(), &target) {
                log::error!("couldn't move {}: {err}", entry.path().display());
            }
        }
        // Gone once everything moved; anything left (a clash) stays where it was.
        if std::fs::remove_dir(&old).is_ok() {
            log::info!("moved {} to {}", old.display(), new.display());
        }
    }
}
