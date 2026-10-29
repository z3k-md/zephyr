use tauri::AppHandle;

use crate::updater;

#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<String, String> {
    updater::check_for_updates(&app).await
}

/// What the update card shows now, for a card that just opened.
#[tauri::command]
pub fn update_status() -> Option<updater::Status> {
    updater::current()
}

/// A downloaded update put off with Later, for the bar's button.
#[tauri::command]
pub fn update_waiting() -> Option<String> {
    updater::waiting()
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
