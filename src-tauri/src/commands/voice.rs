//! Hold-to-talk commands. Nothing here opens the microphone; that happens only while the
//! voice shortcut is held.

use tauri::{AppHandle, Manager};

use super::{blocking, publish};
use crate::state::{AppState, Persisted};
use crate::voice::download::Status;
use crate::voice::{self, download, session};

#[tauri::command]
pub fn voice_status() -> Status {
    download::status()
}

/// Starts the model download and returns. Progress arrives as `voice-download` events.
#[tauri::command]
pub fn voice_download(app: AppHandle) -> Result<(), String> {
    download::start(app)
}

#[tauri::command]
pub fn voice_cancel_download() {
    download::cancel();
}

/// Deletes the weights folder. A loaded model is dropped and the hotkey goes with it
/// until the weights are downloaded again.
#[tauri::command]
pub async fn voice_delete_model(app: AppHandle) -> Result<(), String> {
    blocking(move || {
        if download::running() {
            download::cancel_and_wait();
        }
        session::unload();
        let dir = voice::model_path()?;
        voice::model::delete(&dir)?;
        voice::reregister(&app);
        download::emit(&app);
        Ok(())
    })
    .await
}

/// Saves hold-to-talk.
#[tauri::command]
pub async fn save_voice_settings(
    app: AppHandle,
    enabled: bool,
    shortcut: String,
) -> Result<Persisted, String> {
    blocking(move || {
        let snapshot = app
            .state::<AppState>()
            .update(|persisted| persisted.set_voice(enabled, shortcut))?;
        voice::reregister(&app);
        download::emit(&app);
        publish(&app, snapshot)
    })
    .await
}

/// Stops the hold, drops the audio, and sends `voice-final` with `cancelled: true`.
#[tauri::command]
pub fn voice_cancel() {
    session::cancel();
}
