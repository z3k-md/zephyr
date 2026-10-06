use tauri::App;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri_plugin_opener::OpenerExt;

use crate::{logger, updater, window};

pub fn init(app: &App) -> Result<(), String> {
    let version = MenuItem::with_id(
        app,
        "version",
        format!("Zephyr {}", app.package_info().version),
        false,
        None::<&str>,
    )
    .map_err(|err| err.to_string())?;
    let show = MenuItem::with_id(app, "show", "Show Zephyr", true, None::<&str>)
        .map_err(|err| err.to_string())?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)
        .map_err(|err| err.to_string())?;
    let updates = MenuItem::with_id(app, "updates", "Check for Updates", true, None::<&str>)
        .map_err(|err| err.to_string())?;
    let logs = MenuItem::with_id(app, "logs", "Open Logs Folder", true, None::<&str>)
        .map_err(|err| err.to_string())?;
    let separator = PredefinedMenuItem::separator(app).map_err(|err| err.to_string())?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)
        .map_err(|err| err.to_string())?;

    let menu = Menu::with_items(
        app,
        &[
            &version, &show, &settings, &updates, &logs, &separator, &quit,
        ],
    )
    .map_err(|err| err.to_string())?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or("The app icon is missing")?;

    TrayIconBuilder::with_id("zephyr")
        .icon(icon)
        .tooltip("Zephyr")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => window::show_bar(app),
            "settings" => {
                if let Err(err) = window::open_settings(app, None) {
                    log::error!("couldn't open settings: {err}");
                }
            }
            "updates" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    match updater::check_for_updates(&app).await {
                        Ok(message) => log::info!("{message}"),
                        Err(err) => log::error!("{err}"),
                    }
                });
            }
            "logs" => {
                let path = logger::log_dir();
                if let Err(err) = app.opener().open_path(path.to_string_lossy(), None::<&str>) {
                    log::error!("couldn't open logs: {err}");
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                window::show_bar(tray.app_handle());
            }
        })
        .build(app)
        .map_err(|err| err.to_string())?;

    Ok(())
}
