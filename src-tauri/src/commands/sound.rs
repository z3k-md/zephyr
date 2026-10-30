use super::blocking;
use crate::audio::{self, Level, Sound, Target};

/// The Sound view's refresh: devices, master and every app, with how loud each is now.
#[tauri::command]
pub async fn sound_snapshot() -> Result<Option<Sound>, String> {
    blocking(|| Ok(audio::snapshot())).await
}

/// Sets master (`key` = `master`) or an app's level, 0–1.
#[tauri::command]
pub async fn sound_set_volume(key: String, volume: f32) -> Result<Option<Level>, String> {
    blocking(move || {
        Ok(audio::set_volume(
            &Target::from_key(&key),
            volume.clamp(0.0, 1.0),
        ))
    })
    .await
}

/// Moves master or an app by `delta`, -1–1.
#[tauri::command]
pub async fn sound_step(key: String, delta: f32) -> Result<Option<Level>, String> {
    blocking(move || Ok(audio::step(&Target::from_key(&key), delta.clamp(-1.0, 1.0)))).await
}

#[tauri::command]
pub async fn sound_set_muted(key: String, muted: bool) -> Result<Option<Level>, String> {
    blocking(move || Ok(audio::set_muted(&Target::from_key(&key), muted))).await
}

/// Makes a device the default output.
#[tauri::command]
pub async fn sound_set_device(id: String) -> Result<(), String> {
    blocking(move || audio::set_default_device(&id)).await
}
