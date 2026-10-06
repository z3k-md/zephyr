use std::sync::atomic::{AtomicU32, Ordering};

use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

/// Previous default, kept so existing installs pick up the new one.
pub const LEGACY_DEFAULT_SHORTCUT: &str = "ctrl+shift+space";

/// The key immediately left of Space: Command on a Mac keyboard, Alt on a PC keyboard.
#[cfg(target_os = "macos")]
pub const DEFAULT_SHORTCUT: &str = "command+space";

#[cfg(not(target_os = "macos"))]
pub const DEFAULT_SHORTCUT: &str = "alt+space";

pub fn canonical_shortcut(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("A shortcut is required".into());
    }
    parse(trimmed)?;
    Ok(trimmed.to_string())
}

static CLIPBOARD_ID: AtomicU32 = AtomicU32::new(0);
static NOTES_ID: AtomicU32 = AtomicU32::new(0);

/// Optional shortcuts besides the summon one.
#[derive(Default, Clone, Copy)]
pub struct Extras<'a> {
    pub clipboard: Option<&'a str>,
    pub notes: Option<&'a str>,
}

/// Replaces the registered shortcuts with the summon shortcut and the optional ones. An
/// optional shortcut that can't be registered is logged, not fatal.
pub fn register(app: &AppHandle, shortcut: &str, extras: Extras<'_>) -> Result<(), String> {
    let parsed = parse(shortcut)?;
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();
    CLIPBOARD_ID.store(0, Ordering::SeqCst);
    NOTES_ID.store(0, Ordering::SeqCst);
    shortcuts
        .register(parsed)
        .map_err(|err| format!("Couldn't register {shortcut}: {err}"))?;
    for (wanted, slot) in [(extras.clipboard, &CLIPBOARD_ID), (extras.notes, &NOTES_ID)] {
        let Some(wanted) = wanted.filter(|value| !value.is_empty()) else {
            continue;
        };
        match parse(wanted).and_then(|extra| {
            shortcuts
                .register(extra)
                .map(|()| extra)
                .map_err(|err| format!("Couldn't register {wanted}: {err}"))
        }) {
            Ok(extra) => slot.store(extra.id(), Ordering::SeqCst),
            Err(err) => log::error!("{err}"),
        }
    }
    Ok(())
}

pub fn is_notes(shortcut: &Shortcut) -> bool {
    let id = NOTES_ID.load(Ordering::SeqCst);
    id != 0 && shortcut.id() == id
}

pub fn is_clipboard(shortcut: &Shortcut) -> bool {
    let id = CLIPBOARD_ID.load(Ordering::SeqCst);
    id != 0 && shortcut.id() == id
}

fn parse(shortcut: &str) -> Result<Shortcut, String> {
    shortcut
        .parse()
        .map_err(|err| format!("Invalid shortcut: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_summon_shortcut_parses() {
        assert!(canonical_shortcut(DEFAULT_SHORTCUT).is_ok());
        assert!(canonical_shortcut("alt+space").is_ok());
        assert!(canonical_shortcut("command+space").is_ok());
        assert!(canonical_shortcut("ctrl+alt+k").is_ok());
        assert!(canonical_shortcut("not a shortcut").is_err());
    }
}
