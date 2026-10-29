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

use crate::state::AppState;

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
                            commands::run_binding(&app, &target);
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
        .setup(|app| {
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

            if let Err(err) = shortcut::register(
                app.handle(),
                &snapshot.summon_shortcut,
                snapshot.extra_shortcuts(),
            ) {
                log::error!("{err}");
            }
            startup::mark("summon shortcut registered");
            // Off the startup path: on Windows it runs schtasks, which takes a moment.
            let autostart_app = app.handle().clone();
            let (launch_at_startup, instant) =
                (snapshot.launch_at_startup, snapshot.instant_startup);
            std::thread::spawn(move || {
                if let Err(err) =
                    commands::apply_autostart(&autostart_app, launch_at_startup, instant)
                {
                    log::error!("{err}");
                }
            });
            // The 15 minutes of totals System Resources graphs is always being kept, and the
            // connection watched unless Settings turned that off.
            net::set_enabled(snapshot.watch_connection);
            net::start();
            cputemp::start(snapshot.cpu_temp);
            stats::start(app.handle().clone());
            // The settings catalog reads every System Settings pane's plist on macOS (most of a
            // second); built now, the first search doesn't wait on it.
            std::thread::spawn(|| {
                settings::catalog();
            });

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

            app.manage(state);
            handoff::start();
            power::resume(app.handle());
            browser::start();
            nowplaying::start(app.handle());

            if let Err(err) = tray::init(app) {
                log::error!("tray setup failed: {err}");
            }

            window::prewarm_settings(app.handle());
            // First run (or every launch in dev, when switched on): setup before first use.
            let show_setup = !snapshot.onboarding_done
                || (cfg!(debug_assertions) && snapshot.onboarding_every_launch);
            if show_setup && let Err(err) = window::open_onboarding(app.handle()) {
                log::error!("couldn't open setup: {err}");
            }
            if let Some(main_window) = app.get_webview_window("main") {
                #[cfg(target_os = "macos")]
                if let Err(err) = window::make_bar_panel(&main_window) {
                    log::error!("couldn't let the bar float over full-screen apps: {err}");
                }
                #[cfg(target_os = "macos")]
                window::hide_on_space_change(app.handle());
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
                window::draw_bar_unseen(app.handle());
            }

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
                commands::file_cache_path(app.handle()),
            );
            updater::spawn_background_checks(app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::clip_list,
            commands::clip_detail,
            commands::clip_paste,
            commands::clip_copy,
            commands::clip_pin,
            commands::clip_delete,
            commands::clip_clear,
            commands::clip_can_paste,
            commands::clip_request_paste_permission,
            commands::clip_stats,
            commands::save_clipboard_settings,
            commands::page_ready,
            commands::save_typing_result,
            commands::save_resume_seconds,
            commands::save_theme,
            commands::now_playing,
            commands::now_playing_control,
            commands::now_playing_volume,
            commands::now_playing_focus,
            commands::save_bug_capture,
            commands::reveal_captures,
            commands::allow_protected_folders,
            commands::permissions_status,
            commands::system_stats,
            commands::resources_snapshot,
            commands::resources_closed,
            commands::resources_quit,
            commands::resources_relaunch,
            commands::resources_switch,
            commands::resources_reveal,
            commands::resources_detail,
            commands::resources_cores,
            commands::resources_memory,
            commands::save_show_stats,
            commands::search_apps,
            commands::weather,
            commands::weather_places,
            commands::weather_warm,
            commands::weather_at,
            commands::save_weather_place,
            commands::save_weather_unit,
            commands::network_summary,
            commands::network_history,
            commands::network_public_ip,
            commands::network_addresses,
            commands::save_watch_connection,
            commands::save_cpu_temp,
            commands::cpu_temp_status,
            commands::speed_test,
            commands::speed_test_cancel,
            commands::open_onboarding,
            commands::hide_settings,
            commands::window_chrome,
            commands::bar_move,
            commands::bar_move_end,
            commands::save_launch_groups,
            commands::record_view_open,
            commands::launch_group,
            commands::finish_onboarding,
            commands::save_onboarding_every_launch,
            commands::permission_request,
            commands::save_binding,
            commands::drag_prepare,
            commands::file_preview,
            commands::quick_look,
            commands::drag_finished,
            commands::binding_failures,
            commands::edit_binding,
            commands::take_pending_binding,
            commands::power_preview,
            commands::power_run,
            commands::power_cancel,
            commands::open_url,
            commands::site_icon_url,
            commands::app_icon,
            commands::start_bar_drag,
            commands::prepare_grid,
            commands::warm_suggestions,
            commands::capture_event,
            commands::capture_recording,
            commands::capture_toggle,
            commands::capture_snapshot,
            commands::shell_run,
            commands::shell_stop,
            commands::shell_open_terminal,
            commands::shell_info,
            commands::save_shell_program,
            commands::pick_folder,
            commands::set_active_mode,
            commands::open_note_window,
            commands::close_note_window,
            commands::keep_note_window,
            commands::save_open_behavior,
            commands::sync_status,
            commands::sync_google_sign_in,
            commands::sync_cancel_sign_in,
            commands::sync_recovery_saved,
            commands::sync_use_recovery,
            commands::sync_poll,
            commands::sync_pending,
            commands::sync_approve,
            commands::sync_revoke,
            commands::sync_recovery_key,
            commands::sync_sign_out,
            commands::notes_list,
            commands::note_get,
            commands::note_create,
            commands::note_save,
            commands::note_delete,
            commands::reveal_notes,
            commands::open_notes,
            commands::save_notes_shortcut,
            commands::pause_shortcuts,
            commands::suggest,
            commands::dispatch,
            commands::launch_app,
            commands::window_focus,
            commands::window_close,
            commands::app_quit,
            commands::open_setting,
            commands::open_file,
            commands::reveal_file,
            commands::file_index_status,
            commands::rebuild_file_index,
            commands::save_file_folders,
            commands::save_settings,
            commands::save_instant_startup,
            commands::set_bar_height,
            commands::hide_bar,
            commands::open_settings,
            commands::check_for_updates,
            commands::update_status,
            commands::update_waiting,
            commands::update_install_now,
            commands::update_later,
            commands::rehearse_update,
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
