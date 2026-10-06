use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder,
};

pub const BAR_WIDTH: f64 = 680.0;

static SUPPRESS_BLUR: AtomicBool = AtomicBool::new(false);
static SHOW_GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn suppressing_blur() -> bool {
    SUPPRESS_BLUR.load(Ordering::SeqCst)
}

pub fn show_bar(app: &AppHandle) {
    crate::apps::AppIndex::refresh_if_stale(crate::apps::index());
    let Some(window) = app.get_webview_window("main") else {
        log::error!("main window is missing");
        return;
    };

    let was_visible = window.is_visible().unwrap_or(false);
    let generation = SHOW_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    SUPPRESS_BLUR.store(true, Ordering::SeqCst);

    if !was_visible && let Err(err) = place_bar(&window) {
        log::error!("couldn't place the bar: {err}");
    }

    if let Err(err) = window.show() {
        log::error!("couldn't show the bar: {err}");
    }
    let _ = window.unminimize();
    force_foreground(&window);
    if let Err(err) = window.set_focus() {
        log::error!("couldn't focus the bar: {err}");
    }

    let event = if was_visible {
        "bar-focus"
    } else {
        "bar-shown"
    };
    if let Err(err) = window.emit(event, ()) {
        log::error!("couldn't notify the bar: {err}");
    }

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(350));
        if SHOW_GENERATION.load(Ordering::SeqCst) == generation {
            SUPPRESS_BLUR.store(false, Ordering::SeqCst);
        }
    });
}

pub fn hide_bar(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

pub fn set_bar_height(app: &AppHandle, height: f64) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or("Main window is missing")?;
    let scale = window.scale_factor().map_err(|err| err.to_string())?;
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let width = (BAR_WIDTH * scale).round().clamp(1.0, 4000.0) as u32;
    let height = (height * scale).round().clamp(48.0, 720.0) as u32;
    window
        .set_size(PhysicalSize::new(width, height))
        .map_err(|err| err.to_string())?;
    Ok(())
}

/// Opens or focuses the settings window, scrolled to `section` when one is given.
pub fn open_settings(app: &AppHandle, section: Option<&str>) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("settings") {
        if let Some(section) = section {
            let _ = window.emit("settings-section", section);
        }
        let _ = window.show();
        let _ = window.unminimize();
        focus_on_main_thread(app, window);
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(
        app,
        "settings",
        WebviewUrl::App(match section {
            Some(section) => format!("index.html?view=settings&section={section}").into(),
            None => "index.html?view=settings".into(),
        }),
    )
    .title("Zephyr Settings")
    .inner_size(760.0, 720.0)
    .min_inner_size(560.0, 480.0)
    .center()
    .resizable(true)
    .focused(true)
    .build()
    .map_err(|err| err.to_string())?;
    focus_on_main_thread(app, window);
    Ok(())
}

// open_settings can be called from an async command's worker thread; the Win32 foreground
// dance attaches the calling thread's input queue, so it has to run on the UI thread.
fn focus_on_main_thread(app: &AppHandle, window: tauri::WebviewWindow) {
    let result = app.run_on_main_thread(move || {
        force_foreground(&window);
        let _ = window.set_focus();
    });
    if let Err(err) = result {
        log::error!("couldn't focus settings: {err}");
    }
}

/// A normal macOS window stays on the Space it was first shown on, and a full-screen Space
/// only admits windows marked as auxiliary to it, so the bar never appeared over a
/// full-screen app. Joining every Space as a full-screen auxiliary, above the menu bar,
/// lets it open over whatever is in front without switching Spaces.
#[cfg(target_os = "macos")]
pub fn float_over_full_screen(window: &tauri::WebviewWindow) -> Result<(), String> {
    use objc2_app_kit::{NSStatusWindowLevel, NSWindow, NSWindowCollectionBehavior};

    let ns_window = window.ns_window().map_err(|err| err.to_string())?;
    // SAFETY: tauri hands back the live NSWindow behind this webview window, and setup
    // runs on the main thread.
    let ns_window = unsafe { &*ns_window.cast::<NSWindow>() };
    ns_window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    ns_window.setLevel(NSStatusWindowLevel);
    Ok(())
}

fn place_bar(window: &tauri::WebviewWindow) -> Result<(), String> {
    let cursor = window.cursor_position().map_err(|err| err.to_string())?;
    let monitors = window.available_monitors().map_err(|err| err.to_string())?;
    let monitor = monitors
        .iter()
        .find(|monitor| contains(monitor, cursor))
        .or_else(|| monitors.first())
        .ok_or("No monitor is available")?;

    let work = monitor.work_area();
    let scale = monitor.scale_factor();
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let width = (BAR_WIDTH * scale)
        .round()
        .clamp(1.0, work.size.width as f64) as u32;
    let current_height = window.outer_size().map(|size| size.height).unwrap_or(120);
    let x = work.position.x + ((work.size.width as i32 - width as i32) / 2).max(0);
    let y = work.position.y + (work.size.height as i32 / 6);

    window
        .set_size(PhysicalSize::new(width, current_height.max(48)))
        .map_err(|err| err.to_string())?;
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|err| err.to_string())?;
    Ok(())
}

fn contains(monitor: &tauri::Monitor, cursor: tauri::PhysicalPosition<f64>) -> bool {
    let position = monitor.position();
    let size = monitor.size();
    cursor.x >= position.x as f64
        && cursor.x < position.x as f64 + size.width as f64
        && cursor.y >= position.y as f64
        && cursor.y < position.y as f64 + size.height as f64
}

fn force_foreground(window: &tauri::WebviewWindow) {
    #[cfg(windows)]
    {
        if let Err(err) = focus_windows(window) {
            log::debug!("foreground focus fell back to tauri: {err}");
        }
    }
    #[cfg(not(windows))]
    {
        let _ = window;
    }
}

#[cfg(windows)]
fn focus_windows(window: &tauri::WebviewWindow) -> Result<(), String> {
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::{KEYEVENTF_KEYUP, keybd_event};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, SW_SHOW, SetForegroundWindow, ShowWindow,
    };

    let hwnd = window.hwnd().map_err(|err| err.to_string())?;
    unsafe {
        // Windows ignores SetForegroundWindow unless this thread is attached to the
        // current foreground thread. The empty key event is the documented fallback
        // when the OS still refuses the focus change.
        keybd_event(0, 0, KEYEVENTF_KEYUP, 0);
        let foreground = GetForegroundWindow();
        let foreground_thread = GetWindowThreadProcessId(foreground, None);
        let current_thread = GetCurrentThreadId();
        let attached = foreground_thread != 0
            && foreground_thread != current_thread
            && AttachThreadInput(current_thread, foreground_thread, true).as_bool();
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        if attached {
            let _ = AttachThreadInput(current_thread, foreground_thread, false);
        }
    }
    Ok(())
}
