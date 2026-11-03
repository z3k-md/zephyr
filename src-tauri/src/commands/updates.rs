use tauri::AppHandle;

use super::save;
use crate::state::Persisted;
use crate::updater;

/// Settings › About: `stable` or `nightly`. Switching checks the new channel right away.
#[tauri::command]
pub async fn save_update_channel(app: AppHandle, channel: String) -> Result<Persisted, String> {
    if !updater::CHANNELS.contains(&channel.as_str()) {
        return Err(format!("Unknown update channel {channel}"));
    }
    let saved = channel.clone();
    let snapshot = save(app.clone(), move |persisted| {
        persisted.update_channel = saved;
        Ok(())
    })
    .await?;
    updater::switch_channel(&app, &channel);
    Ok(snapshot)
}

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
