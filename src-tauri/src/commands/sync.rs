use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use super::blocking;

#[tauri::command]
pub async fn sync_status() -> Result<crate::sync::Status, String> {
    crate::sync::status().await
}

/// Opens Google sign-in in the browser and finishes when it comes back.
#[tauri::command]
pub async fn sync_google_sign_in(app: AppHandle) -> Result<(), String> {
    crate::sync::google_sign_in(|url| {
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|err| format!("Couldn't open the browser: {err}"))
    })
    .await
}

#[tauri::command]
pub fn sync_cancel_sign_in() -> Result<(), String> {
    crate::sync::cancel_sign_in()
}

#[tauri::command]
pub fn sync_recovery_saved() -> Result<(), String> {
    crate::sync::recovery_saved()
}

#[tauri::command]
pub async fn sync_use_recovery(key: String) -> Result<(), String> {
    crate::sync::use_recovery(&key).await
}

#[tauri::command]
pub async fn sync_poll() -> Result<(), String> {
    crate::sync::poll().await
}

#[tauri::command]
pub async fn sync_pending() -> Result<Vec<crate::sync::Pending>, String> {
    crate::sync::pending().await
}

#[tauri::command]
pub async fn sync_approve(device_id: String) -> Result<(), String> {
    crate::sync::approve(&device_id).await
}

#[tauri::command]
pub async fn sync_revoke(device_id: String) -> Result<(), String> {
    crate::sync::revoke(&device_id).await
}

// Async: it reads the keychain, which can wait on a system prompt.
#[tauri::command]
pub async fn sync_recovery_key() -> Result<String, String> {
    blocking(crate::sync::recovery_key).await
}

#[tauri::command]
pub async fn sync_sign_out() -> Result<(), String> {
    crate::sync::sign_out().await
}
