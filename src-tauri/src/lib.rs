mod ai;
mod answer;
mod apps;
mod claude;
mod clipboard;
mod commands;
mod deeplink;
mod destination;
mod files;
mod history;
mod logger;
mod notes;
mod query;
mod secrets;
mod settings;
mod shell;
mod shortcut;
mod state;
mod suggest;
mod template;
mod tray;
mod updater;
mod window;

use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::ShortcutState;

use crate::state::AppState;

pub fn init_logging() {
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
                            if let Err(err) = window::open_notes(&app, None) {
                                log::error!("couldn't open notes: {err}");
                            }
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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // A tray utility should not put an icon in the Dock or take over the menu bar.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

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
            if let Err(err) = commands::apply_autostart(app.handle(), snapshot.launch_at_startup) {
                log::error!("{err}");
            }

            secrets::init(app.path().app_data_dir()?);
            notes::init(config_dir.join("notes"));
            clipboard::start(
                app.path().app_data_dir()?.join("clipboard"),
                snapshot.clipboard.clone(),
            );

            app.manage(state);
            claude::init(app.handle(), app.path().app_data_dir()?.join("claude"));

            if let Err(err) = tray::init(app) {
                log::error!("tray setup failed: {err}");
            }

            if let Some(main_window) = app.get_webview_window("main") {
                #[cfg(target_os = "macos")]
                if let Err(err) = window::make_bar_panel(&main_window) {
                    log::error!("couldn't let the bar float over full-screen apps: {err}");
                }
                #[cfg(windows)]
                if let Err(err) = window::make_bar_glass(&main_window) {
                    log::error!("couldn't blur behind the bar: {err}");
                }

                let watched = main_window.clone();
                main_window.on_window_event(move |event| match event {
                    WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        let _ = watched.hide();
                    }
                    WindowEvent::Focused(false) if !window::suppressing_blur() => {
                        let _ = watched.hide();
                        let _ = watched.emit("bar-hidden", ());
                    }
                    _ => {}
                });
                let _ = main_window.hide();
            }

            register_links(app);

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
            commands::ai_ask,
            commands::resolve_url,
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
            commands::open_clipboard,
            commands::page_ready,
            commands::save_typing_result,
            commands::save_resume_seconds,
            commands::shell_run,
            commands::shell_stop,
            commands::shell_open_terminal,
            commands::shell_info,
            commands::save_shell_program,
            commands::save_shell_terminal,
            commands::clear_shell_history,
            commands::pick_folder,
            commands::claude_jobs,
            commands::claude_approvals,
            commands::claude_submit,
            commands::claude_follow_up,
            commands::claude_cancel,
            commands::claude_remove,
            commands::claude_open_terminal,
            commands::claude_answer,
            commands::claude_status,
            commands::save_claude_settings,
            commands::notes_list,
            commands::note_get,
            commands::note_create,
            commands::note_save,
            commands::note_delete,
            commands::reveal_notes,
            commands::open_notes,
            commands::save_notes_shortcut,
            commands::export_destinations,
            commands::import_destinations,
            commands::ai_cancel,
            commands::save_ai_settings,
            commands::ai_presets,
            commands::ai_set_key,
            commands::ai_has_key,
            commands::ai_models,
            commands::ai_detect_local,
            commands::suggest,
            commands::dispatch,
            commands::launch_app,
            commands::open_setting,
            commands::open_file,
            commands::reveal_file,
            commands::file_index_status,
            commands::rebuild_file_index,
            commands::save_file_folders,
            commands::save_settings,
            commands::save_destination,
            commands::remove_destination,
            commands::move_destination,
            commands::clear_history,
            commands::set_bar_height,
            commands::hide_bar,
            commands::show_bar,
            commands::open_settings,
            commands::check_for_updates,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Zephyr");
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
