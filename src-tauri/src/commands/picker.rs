use tauri::AppHandle;

use super::blocking;

/// Hide the bar and open the screen color picker.
#[tauri::command]
pub async fn pick_color(app: AppHandle) -> Result<(), String> {
    blocking(move || crate::picker::start(&app)).await
}

/// The loupe's pixel: copy its hex and show the bar with that color typed in.
#[tauri::command]
pub async fn picker_choose(app: AppHandle, hex: String) -> Result<(), String> {
    blocking(move || crate::picker::choose(&app, &hex)).await
}

/// Esc or a right-click: close the picker and drop the screenshot.
#[tauri::command]
pub fn picker_cancel(app: AppHandle) {
    crate::picker::cancel(&app);
}
