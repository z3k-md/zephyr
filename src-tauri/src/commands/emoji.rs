use tauri::AppHandle;

use super::save;
use crate::clipboard;
use crate::state::Persisted;
use crate::window;

/// Pastes `text` into the app in front. `copy_only` copies and dismisses the bar instead.
/// Without Accessibility permission it copies and says so, the same as clipboard history.
#[tauri::command]
pub fn paste_text(app: AppHandle, text: String, copy_only: bool) -> Result<String, String> {
    if copy_only {
        clipboard::copy_text(&text)?;
        window::dismiss_bar(&app);
        return Ok("copied".into());
    }
    match clipboard::paste_text(&text, || window::hide_bar(&app))? {
        clipboard::PasteOutcome::Pasted => Ok("pasted".into()),
        clipboard::PasteOutcome::NeedsPermission => Ok("needsPermission".into()),
    }
}

/// The skin tone (0–5) and the emoji pasted lately, newest first.
#[tauri::command]
pub async fn save_emoji_prefs(
    app: AppHandle,
    skin: u8,
    recents: Vec<String>,
) -> Result<Persisted, String> {
    save(app, move |persisted| persisted.set_emoji(skin, recents)).await
}
