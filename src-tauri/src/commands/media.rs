use tauri::AppHandle;

/// What's playing now, for a bar that just opened; later changes come as `nowplaying` events.
#[tauri::command]
pub fn now_playing() -> Option<crate::nowplaying::NowPlaying> {
    crate::nowplaying::snapshot()
}

/// Presses play/pause, next or previous on what's playing.
#[tauri::command]
pub fn now_playing_control(control: String) -> Result<(), String> {
    let control = match control.as_str() {
        "toggle" => crate::nowplaying::Control::Toggle,
        "next" => crate::nowplaying::Control::Next,
        "previous" => crate::nowplaying::Control::Previous,
        other => return Err(format!("Unknown media control {other}")),
    };
    crate::nowplaying::control(control);
    Ok(())
}

/// The scroll wheel over the track: what's playing a step louder or quieter, just that app.
/// Off the main thread, since on macOS each notch runs AppleScript.
#[tauri::command]
pub async fn now_playing_volume(step: f32) -> Option<f32> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::nowplaying::change_volume(step.clamp(-0.2, 0.2))
    })
    .await
    .ok()
    .flatten()
}

/// Switches to the app or browser tab playing the song, hiding the bar when it could.
#[tauri::command]
pub async fn now_playing_focus(app: AppHandle) -> Result<bool, String> {
    let Some(playing) = crate::nowplaying::snapshot() else {
        return Ok(false);
    };
    let focused = tauri::async_runtime::spawn_blocking(move || {
        crate::nowplaying::focus_source(&playing.source_app, &playing.title)
    })
    .await
    .map_err(|err| err.to_string())?;
    if focused {
        crate::window::hide_bar(&app);
    }
    Ok(focused)
}
