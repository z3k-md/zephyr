use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

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
/// Optional shortcuts besides the summon one. Bug captures aren't among them: Ctrl+Shift+S
/// and Ctrl+R work only inside Zephyr's windows (main.ts), so other apps keep them.
#[derive(Default, Clone, Copy)]
pub struct Extras<'a> {
    pub clipboard: Option<&'a str>,
    pub notes: Option<&'a str>,
    /// Hotkeys the user gave commands, apps and bookmarks.
    pub bindings: &'a [crate::state::Binding],
}

/// The registered binding hotkeys, by shortcut id, with what each runs.
fn bound() -> &'static Mutex<Vec<(u32, String)>> {
    static BOUND: OnceLock<Mutex<Vec<(u32, String)>>> = OnceLock::new();
    BOUND.get_or_init(Default::default)
}

/// Bindings whose hotkey couldn't be registered (another app holds it), by target.
fn failed() -> &'static Mutex<Vec<String>> {
    static FAILED: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    FAILED.get_or_init(Default::default)
}

/// What a pressed binding hotkey runs.
pub fn binding_for(shortcut: &Shortcut) -> Option<String> {
    bound()
        .lock()
        .ok()?
        .iter()
        .find(|(id, _)| *id == shortcut.id())
        .map(|(_, target)| target.clone())
}

/// The targets whose hotkey another app already holds, for Settings to point out.
pub fn failed_bindings() -> Vec<String> {
    failed()
        .lock()
        .map(|failed| failed.clone())
        .unwrap_or_default()
}

/// Whether two shortcuts are the same keys, however they were written.
pub fn same(a: &str, b: &str) -> bool {
    matches!((parse(a), parse(b)), (Ok(a), Ok(b)) if a.id() == b.id())
}

/// Lets go of every shortcut while Settings records a new one, so pressing a combo Zephyr
/// already uses (the summon key itself) is recorded instead of acted on. `register` takes
/// them back.
pub fn pause(app: &AppHandle) {
    RECORDING.store(true, Ordering::SeqCst);
    let _ = app.global_shortcut().unregister_all();
}

static RECORDING: AtomicBool = AtomicBool::new(false);

/// Whether a shortcut is being recorded (between `pause` and the next `register`).
pub fn recording() -> bool {
    RECORDING.load(Ordering::SeqCst)
}

/// Replaces the registered shortcuts with the summon shortcut and the optional ones. An
/// optional shortcut that can't be registered is logged, not fatal.
pub fn register(app: &AppHandle, shortcut: &str, extras: Extras<'_>) -> Result<(), String> {
    RECORDING.store(false, Ordering::SeqCst);
    let parsed = parse(shortcut)?;
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();
    CLIPBOARD_ID.store(0, Ordering::SeqCst);
    NOTES_ID.store(0, Ordering::SeqCst);
    // A summon key another app holds shouldn't take the other shortcuts down with it:
    // register the rest, then report it.
    let summon = shortcuts
        .register(parsed)
        .map_err(|err| format!("Couldn't register {shortcut}: {err}"));
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

    let mut registered = Vec::new();
    let mut failures = Vec::new();
    for binding in extras
        .bindings
        .iter()
        .filter(|binding| !binding.hotkey.is_empty())
    {
        match parse(&binding.hotkey).and_then(|hotkey| {
            shortcuts
                .register(hotkey)
                .map(|()| hotkey)
                .map_err(|err| format!("Couldn't register {}: {err}", binding.hotkey))
        }) {
            Ok(hotkey) => registered.push((hotkey.id(), binding.target.clone())),
            Err(err) => {
                log::error!("{err}");
                failures.push(binding.target.clone());
            }
        }
    }
    if let Ok(mut bound) = bound().lock() {
        *bound = registered;
    }
    if let Ok(mut failed) = failed().lock() {
        *failed = failures;
    }
    summon
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
        // The Settings recorder writes ⌘ as "super".
        assert!(canonical_shortcut("super+shift+v").is_ok());
        assert!(canonical_shortcut("not a shortcut").is_err());
    }
}
