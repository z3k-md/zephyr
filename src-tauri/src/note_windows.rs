//! Notes work like sticky notes: each open note is its own window, any number can be open
//! beside the bar, and they reopen where they were when Zephyr starts.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder,
    WindowEvent,
};

use crate::state::{AppState, NoteWindow};

const LABEL_PREFIX: &str = "note-";
const WIDTH: f64 = 380.0;
const HEIGHT: f64 = 420.0;
/// Each new note opens this far down and right of the last one, like a cascade.
const CASCADE: i32 = 28;

/// Windows reopened at start show without taking focus from what the user is doing.
fn quiet() -> &'static Mutex<HashSet<String>> {
    static QUIET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    QUIET.get_or_init(Default::default)
}

/// Per window, bumped by each move or resize so only the last one is saved.
fn moves() -> &'static Mutex<HashMap<String, u64>> {
    static MOVES: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    MOVES.get_or_init(Default::default)
}

pub fn label_for(id: &str) -> String {
    let safe: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{LABEL_PREFIX}{safe}")
}

pub fn is_note_window(label: &str) -> bool {
    label.starts_with(LABEL_PREFIX)
}

/// Whether a window that just painted should come up without taking focus.
pub fn take_quiet(label: &str) -> bool {
    quiet().lock().is_ok_and(|mut quiet| quiet.remove(label))
}

fn open_windows(app: &AppHandle) -> Vec<tauri::WebviewWindow> {
    app.webview_windows()
        .into_iter()
        .filter(|(label, _)| is_note_window(label))
        .map(|(_, window)| window)
        .collect()
}

/// The notes shortcut, `!note` and the bar's Notes row: brings the open notes forward, or
/// opens the newest note (a new one when there are none) when none is open.
pub fn show(app: &AppHandle, id: Option<&str>) -> Result<(), String> {
    if let Some(id) = id {
        return open(app, id, true);
    }
    let open_now = open_windows(app);
    if !open_now.is_empty() {
        let last = open_now.len() - 1;
        for (index, window) in open_now.into_iter().enumerate() {
            let _ = window.show();
            let _ = window.unminimize();
            if index == last {
                crate::window::focus_window(app, window);
            }
        }
        return Ok(());
    }
    let id = match crate::notes::list("").into_iter().next() {
        Some(newest) => newest.id,
        None => crate::notes::create("")?,
    };
    open(app, &id, true)
}

/// Opens `id` in its own window, or brings its window forward when it is already open.
pub fn open(app: &AppHandle, id: &str, focus: bool) -> Result<(), String> {
    let label = label_for(id);
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.show();
        let _ = window.unminimize();
        if focus {
            crate::window::focus_window(app, window);
        }
        return Ok(());
    }

    let saved = saved(app).into_iter().find(|saved| saved.id == id);
    let cascade_from = open_windows(app)
        .into_iter()
        .filter_map(|window| window.outer_position().ok())
        .max_by_key(|position| (position.y, position.x));

    let url = format!("index.html?view=notes&id={}", urlencoding::encode(id));
    // Painted dark and kept hidden until the page has drawn, so it never flashes white.
    let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title("Note")
        .background_color(crate::window::WINDOW_BACKGROUND)
        .transparent(true)
        .effects(crate::window::glass())
        .visible(false)
        .inner_size(WIDTH, HEIGHT)
        .min_inner_size(260.0, 200.0)
        .resizable(true)
        .visible_on_all_workspaces(true)
        .focused(focus);
    let builder = if saved.is_none() && cascade_from.is_none() {
        builder.center()
    } else {
        builder
    };
    let window = builder.build().map_err(|err| err.to_string())?;

    match (&saved, cascade_from) {
        (Some(saved), _) if on_a_screen(&window, saved) => {
            let _ = window.set_size(PhysicalSize::new(saved.width, saved.height));
            let _ = window.set_position(PhysicalPosition::new(saved.x, saved.y));
        }
        (_, Some(last)) => {
            let step = (CASCADE as f64 * window.scale_factor().unwrap_or(1.0)) as i32;
            let _ = window.set_position(PhysicalPosition::new(last.x + step, last.y + step));
        }
        _ => {}
    }

    if !focus && let Ok(mut quiet) = quiet().lock() {
        quiet.insert(label.clone());
    }
    watch(app, &window, id);
    remember(app, &window, id);
    crate::window::reveal_soon(app, window);
    Ok(())
}

/// Reopens the notes that were open when Zephyr last ran, without taking focus.
pub fn restore(app: &AppHandle) {
    for saved in saved(app) {
        if crate::notes::get(&saved.id).is_err() {
            forget(app, &saved.id);
            continue;
        }
        if let Err(err) = open(app, &saved.id, false) {
            log::error!("couldn't reopen a note: {err}");
        }
    }
}

/// Closes a note's window for good, once its page has saved; the note itself stays.
pub fn close(app: &AppHandle, window: &tauri::WebviewWindow) {
    if let Some(id) = id_of(app, window.label()) {
        forget(app, &id);
    }
    let _ = window.destroy();
}

/// The close button asks the page to save first; it closes the window when done, and a
/// page that never answers is closed anyway.
fn watch(app: &AppHandle, window: &tauri::WebviewWindow, id: &str) {
    let app = app.clone();
    let watched = window.clone();
    let id = id.to_string();
    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let _ = watched.emit_to(watched.label(), "note-close", ());
            let app = app.clone();
            let window = watched.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(1500));
                if app.get_webview_window(window.label()).is_some() {
                    close(&app, &window);
                }
            });
        }
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
            remember_soon(&app, &watched, &id);
        }
        _ => {}
    });
}

/// Saves where a note's window is once it has stopped moving for a moment.
fn remember_soon(app: &AppHandle, window: &tauri::WebviewWindow, id: &str) {
    let label = window.label().to_string();
    let generation = {
        let Ok(mut moves) = moves().lock() else {
            return;
        };
        let entry = moves.entry(label.clone()).or_insert(0);
        *entry += 1;
        *entry
    };
    let app = app.clone();
    let window = window.clone();
    let id = id.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        let latest = moves()
            .lock()
            .ok()
            .and_then(|moves| moves.get(&label).copied());
        if latest == Some(generation) && app.get_webview_window(&label).is_some() {
            remember(&app, &window, &id);
        }
    });
}

fn remember(app: &AppHandle, window: &tauri::WebviewWindow, id: &str) {
    let (Ok(position), Ok(size)) = (window.outer_position(), window.inner_size()) else {
        return;
    };
    // A minimized window reports a far-off spot; keep where it was before.
    if window.is_minimized().unwrap_or(false) {
        return;
    }
    let entry = NoteWindow {
        id: id.to_string(),
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    };
    let state = app.state::<AppState>();
    let _ = state.update(|persisted| {
        match persisted
            .note_windows
            .iter_mut()
            .find(|saved| saved.id == id)
        {
            Some(saved) => *saved = entry,
            None => persisted.note_windows.push(entry),
        }
        Ok(())
    });
}

fn forget(app: &AppHandle, id: &str) {
    let state = app.state::<AppState>();
    let _ = state.update(|persisted| {
        persisted.note_windows.retain(|saved| saved.id != id);
        Ok(())
    });
}

fn saved(app: &AppHandle) -> Vec<NoteWindow> {
    app.try_state::<AppState>()
        .and_then(|state| state.snapshot().ok())
        .map(|snapshot| snapshot.note_windows)
        .unwrap_or_default()
}

fn id_of(app: &AppHandle, label: &str) -> Option<String> {
    saved(app)
        .into_iter()
        .map(|saved| saved.id)
        .find(|id| label_for(id) == label)
}

/// Whether a saved spot is still on one of the screens (a monitor may have been unplugged).
fn on_a_screen(window: &tauri::WebviewWindow, saved: &NoteWindow) -> bool {
    let monitors = window.available_monitors().unwrap_or_default();
    monitors.iter().any(|monitor| {
        let (position, size) = (monitor.position(), monitor.size());
        let corner_x = saved.x + 40;
        let corner_y = saved.y + 20;
        corner_x >= position.x
            && corner_x < position.x + size.width as i32
            && corner_y >= position.y
            && corner_y < position.y + size.height as i32
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_safe_for_any_note_id() {
        assert_eq!(label_for("2026-10-13-abc"), "note-2026-10-13-abc");
        assert_eq!(label_for("a b/c"), "note-a_b_c");
        assert!(is_note_window(&label_for("x")));
        assert!(!is_note_window("notes"));
    }
}
