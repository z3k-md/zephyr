mod ai;
mod answer;
mod apps;
mod commands;
mod deeplink;
mod destination;
mod files;
mod history;
mod logger;
mod query;
mod settings;
mod shortcut;
mod state;
mod suggest;
mod template;
mod tray;
mod updater;
mod window;

use tauri::{Manager, WindowEvent};
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
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        window::show_bar(app);
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

            if let Err(err) = shortcut::register(app.handle(), &snapshot.summon_shortcut) {
                log::error!("{err}");
            }
            if let Err(err) = commands::apply_autostart(app.handle(), snapshot.launch_at_startup) {
                log::error!("{err}");
            }

            app.manage(state);

            if let Err(err) = tray::init(app) {
                log::error!("tray setup failed: {err}");
            }

            if let Some(main_window) = app.get_webview_window("main") {
                #[cfg(target_os = "macos")]
                if let Err(err) = window::make_bar_panel(&main_window) {
                    log::error!("couldn't let the bar float over full-screen apps: {err}");
                }

                let watched = main_window.clone();
                main_window.on_window_event(move |event| match event {
                    WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        let _ = watched.hide();
                    }
                    WindowEvent::Focused(false) if !window::suppressing_blur() => {
                        let _ = watched.hide();
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
