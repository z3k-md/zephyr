mod apps;
mod commands;
mod destination;
mod history;
mod logger;
mod query;
mod settings;
mod shortcut;
mod state;
mod suggest;
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
    tauri::Builder::default()
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
                if let Err(err) = window::float_over_full_screen(&main_window) {
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

            log::info!("Zephyr is running");

            apps::AppIndex::refresh_if_stale(apps::index());
            updater::spawn_background_checks(app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::suggest,
            commands::dispatch,
            commands::launch_app,
            commands::open_setting,
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
