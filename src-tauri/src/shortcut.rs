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

/// Replaces whatever summon shortcut is registered with `shortcut`.
pub fn register(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    let parsed = parse(shortcut)?;
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();
    shortcuts
        .register(parsed)
        .map_err(|err| format!("Couldn't register {shortcut}: {err}"))?;
    Ok(())
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
