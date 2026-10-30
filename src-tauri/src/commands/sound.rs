use tauri::AppHandle;

use super::{blocking, save};
use crate::audio::{self, Level, Sound, Target};
use crate::state::Persisted;

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

/// Settings › Sound: whether Zephyr takes the volume keys and shows its own overlay.
#[tauri::command]
pub async fn save_volume_keys(app: AppHandle, on: bool) -> Result<Persisted, String> {
    crate::volume_keys::set_enabled(&app, on);
    save(app, move |persisted| {
        persisted.volume_keys = on;
        Ok(())
    })
    .await
}

/// Settings › Sound: where the overlay shows, `bottom`, `top` or `bottom-right`.
#[tauri::command]
pub async fn save_volume_overlay(app: AppHandle, position: String) -> Result<Persisted, String> {
    if !["bottom", "top", "bottom-right"].contains(&position.as_str()) {
        return Err(format!("Unknown overlay position {position}"));
    }
    crate::volume_keys::set_position(&position);
    save(app, move |persisted| {
        persisted.volume_overlay = position;
        Ok(())
    })
    .await
}

/// The wheel over the volume overlay: the target a notch louder or quieter.
#[tauri::command]
pub fn osd_wheel(delta: f32) {
    crate::volume_keys::wheel(delta.clamp(-0.2, 0.2));
}

/// A click on the volume overlay opens the Sound view.
#[tauri::command]
pub fn osd_open_sound(app: AppHandle) {
    crate::volume_keys::open_sound(&app);
}
