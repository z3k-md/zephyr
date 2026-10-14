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

/// Set while the user Ctrl+drags the bar, so only their moves are remembered, not
/// `place_bar`'s.
static DRAGGING: AtomicBool = AtomicBool::new(false);
static MOVE_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Ctrl+mousedown on the bar: the OS moves the window until the button is released.
pub fn start_bar_drag(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or("Main window is missing")?;
    DRAGGING.store(true, Ordering::SeqCst);
    // The move loop hands focus around, and the bar hides when it loses focus, so it closed
    // the moment it was dropped. Losing focus doesn't count until the move is over.
    SUPPRESS_BLUR.store(true, Ordering::SeqCst);
    log::info!("moving the bar");
    #[cfg(windows)]
    {
        // Tauri's start_dragging didn't move this window, so Windows' own move loop runs
        // instead, on the UI thread that owns the mouse capture. It returns on drop.
        let handle = app.clone();
        let result = app.run_on_main_thread(move || {
            if let Err(err) = drag_on_windows(&handle, &window) {
                log::error!("couldn't move the bar: {err}");
            }
            log::info!("bar dropped");
            force_foreground(&window);
            let _ = window.set_focus();
            settle_after_move();
        });
        result.map_err(|err| err.to_string())
    }
    #[cfg(not(windows))]
    {
        let result = window.start_dragging().map_err(|err| {
            log::error!("couldn't move the bar: {err}");
            err.to_string()
        });
        settle_after_move();
        result
    }
}

/// Lets focus loss hide the bar again a moment after a move, once the focus has settled.
fn settle_after_move() {
    let generation = SHOW_GENERATION.load(Ordering::SeqCst);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        if SHOW_GENERATION.load(Ordering::SeqCst) == generation {
            SUPPRESS_BLUR.store(false, Ordering::SeqCst);
        }
    });
}

/// Hands the mouse to Windows' window-move loop, which follows it until the button is up.
/// The bar moves freely; a grid shows over the screen meanwhile, with an outline where the
/// bar would snap, and on drop it lands there.
#[cfg(windows)]
fn drag_on_windows(app: &AppHandle, window: &tauri::WebviewWindow) -> Result<(), String> {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
    use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_SYSCOMMAND};
    // SC_MOVE plus HTCAPTION: move by dragging, as if the title bar were held.
    const SC_DRAGMOVE: usize = 0xF012;
    let hwnd = window.hwnd().map_err(|err| err.to_string())?;
    grid::begin(app, hwnd);
    unsafe {
        let _ = ReleaseCapture();
        SendMessageW(
            hwnd,
            WM_SYSCOMMAND,
            Some(WPARAM(SC_DRAGMOVE)),
            Some(LPARAM(0)),
        );
    }
    if let Some((x, y)) = grid::end() {
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
    Ok(())
}

/// The snap grid shown over the screen while the bar is dragged: its own transparent,
/// click-through window under the bar, made ahead of time (see [`prepare_grid`]) so the
/// drag never waits for it.
#[cfg(windows)]
mod grid {
    use std::sync::Mutex;

    use tauri::{AppHandle, Emitter, Manager};
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowPos, ShowWindow,
    };

    #[derive(Default)]
    struct Drag {
        app: Option<AppHandle>,
        bar: isize,
        grid: isize,
        /// The work area the grid covers: x, y, width, height.
        area: (i32, i32, i32, i32),
        /// Where the bar lands on drop, when a guide is within reach.
        snap: Option<(i32, i32)>,
    }

    static DRAG: Mutex<Option<Drag>> = Mutex::new(None);

    /// The work area of the screen `rect` is mostly on.
    pub fn work_area(rect: &RECT) -> Option<(i32, i32, i32, i32)> {
        let monitor = unsafe { MonitorFromRect(rect, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        unsafe { GetMonitorInfoW(monitor, &mut info) }
            .as_bool()
            .then(|| {
                let work = info.rcWork;
                (
                    work.left,
                    work.top,
                    work.right - work.left,
                    work.bottom - work.top,
                )
            })
    }

    fn place(grid: isize, bar: isize, area: (i32, i32, i32, i32)) {
        unsafe {
            // Right under the bar, so the bar stays on top of its own guides.
            let _ = SetWindowPos(
                HWND(grid as _),
                Some(HWND(bar as _)),
                area.0,
                area.1,
                area.2,
                area.3,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }

    pub fn begin(app: &AppHandle, bar: HWND) {
        let mut rect = RECT::default();
        if unsafe { GetWindowRect(bar, &mut rect) }.is_err() {
            return;
        }
        let area = work_area(&rect).unwrap_or_default();
        let grid = app
            .get_webview_window("grid")
            .and_then(|window| window.hwnd().ok())
            .map(|hwnd| hwnd.0 as isize)
            .unwrap_or(0);
        if grid != 0 {
            let _ = app.emit_to("grid", "grid-preview", serde_json::Value::Null);
            place(grid, bar.0 as isize, area);
        }
        if let Ok(mut drag) = DRAG.lock() {
            *drag = Some(Drag {
                app: Some(app.clone()),
                bar: bar.0 as isize,
                grid,
                area,
                snap: None,
            });
        }
    }

    /// From the move loop: where the bar is about to be. Moves the grid along when the bar
    /// changes screens, and shows where it would snap.
    pub fn moving(rect: &RECT) {
        let Ok(mut guard) = DRAG.lock() else {
            return;
        };
        let Some(drag) = guard.as_mut() else {
            return;
        };
        let Some(area) = work_area(rect) else {
            return;
        };
        if area != drag.area {
            drag.area = area;
            if drag.grid != 0 {
                place(drag.grid, drag.bar, area);
            }
        }
        let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
        let (x, y) = super::snap_position((rect.left, rect.top), area, width, height);
        drag.snap = ((x, y) != (rect.left, rect.top)).then_some((x, y));
        if let Some(app) = &drag.app {
            // Relative to the grid window, in physical pixels; the page scales them.
            let preview = drag.snap.map(|(x, y)| {
                serde_json::json!({
                    "x": x - area.0,
                    "y": y - area.1,
                    "width": width,
                    "height": height,
                })
            });
            let _ = app.emit_to(
                "grid",
                "grid-preview",
                preview.unwrap_or(serde_json::Value::Null),
            );
        }
    }

    /// The drop: hides the grid and says where the bar should land, if it snapped.
    pub fn end() -> Option<(i32, i32)> {
        let drag = DRAG.lock().ok()?.take()?;
        if drag.grid != 0 {
            unsafe {
                let _ = ShowWindow(HWND(drag.grid as _), SW_HIDE);
            }
        }
        drag.snap
    }
}

/// Makes the snap grid's window ahead of the first drag (when Ctrl is first held on the
/// bar). It's built off the main thread, hidden, transparent and click-through.
#[cfg(windows)]
pub fn prepare_grid(app: &AppHandle) -> Result<(), String> {
    if app.get_webview_window("grid").is_some() {
        return Ok(());
    }
    let window =
        WebviewWindowBuilder::new(app, "grid", WebviewUrl::App("index.html?view=grid".into()))
            .title("Zephyr grid")
            .background_color(WINDOW_BACKGROUND)
            .transparent(true)
            .decorations(false)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .focusable(false)
            .focused(false)
            .visible(false)
            .build()
            .map_err(|err| err.to_string())?;
    window
        .set_ignore_cursor_events(true)
        .map_err(|err| err.to_string())
}

#[cfg(not(windows))]
pub fn prepare_grid(_app: &AppHandle) -> Result<(), String> {
    Ok(())
}

/// Remembers where a drag left the bar, once it has stopped moving for a moment.
pub fn bar_moved(app: &AppHandle, position: PhysicalPosition<i32>) {
    if !DRAGGING.load(Ordering::SeqCst) {
        return;
    }
    let generation = MOVE_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        if MOVE_GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        let state = app.state::<crate::state::AppState>();
        let saved = state.update(|persisted| {
            persisted.bar_position = Some(crate::state::BarPosition {
                x: position.x,
                y: position.y,
            });
            Ok(())
        });
        if let Err(err) = saved {
            log::error!("couldn't remember where the bar is: {err}");
        }
    });
}

pub fn show_bar(app: &AppHandle) {
    crate::apps::AppIndex::refresh_if_stale(crate::apps::index());
    crate::files::index().refresh_if_stale();
    let Some(window) = app.get_webview_window("main") else {
        log::error!("main window is missing");
        return;
    };

    let was_visible = window.is_visible().unwrap_or(false);
    let generation = SHOW_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    SUPPRESS_BLUR.store(true, Ordering::SeqCst);

    if !was_visible {
        DRAGGING.store(false, Ordering::SeqCst);
        #[cfg(target_os = "macos")]
        previous_app::remember();
        if let Err(err) = place_bar(&window) {
            log::error!("couldn't place the bar: {err}");
        }
    }

    #[cfg(target_os = "macos")]
    show_panel(app, &window);
    #[cfg(not(target_os = "macos"))]
    {
        if let Err(err) = window.show() {
            log::error!("couldn't show the bar: {err}");
        }
        let _ = window.unminimize();
        force_foreground(&window);
        if let Err(err) = window.set_focus() {
            log::error!("couldn't focus the bar: {err}");
        }
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

/// The summon shortcut: opens the bar, or closes it when it is already open, like Esc does.
pub fn toggle_bar(app: &AppHandle) {
    let open = app
        .get_webview_window("main")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false);
    if open {
        dismiss_bar(app);
    } else {
        show_bar(app);
    }
}

/// Opens the bar straight into clipboard history.
pub fn show_clipboard(app: &AppHandle) {
    show_bar(app);
    if let Err(err) = app.emit("clipboard-shown", ()) {
        log::error!("couldn't open clipboard history: {err}");
    }
}

/// Hiding only pauses whatever the bar was doing; an answer keeps streaming for when it's
/// opened again.
pub fn hide_bar(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
        // The bar remembers when it closed so reopening soon can return to the same view.
        let _ = window.emit("bar-hidden", ());
    }
}

/// Hides the bar without acting on anything, giving focus back to whatever was in front.
pub fn dismiss_bar(app: &AppHandle) {
    hide_bar(app);
    #[cfg(target_os = "macos")]
    previous_app::restore();
}

/// Sizes the bar to its content. A width change (clipboard history is wider) keeps the bar
/// centered where it is.
pub fn set_bar_height(app: &AppHandle, height: f64, width: Option<f64>) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or("Main window is missing")?;
    let scale = window.scale_factor().map_err(|err| err.to_string())?;
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let css_width = width.unwrap_or(BAR_WIDTH).clamp(BAR_WIDTH, 1200.0);
    let width = (css_width * scale).round() as u32;
    // The limits are in CSS pixels; clamping after scaling cut a Retina bar off at half height.
    let height = (height.clamp(48.0, 720.0) * scale).round() as u32;
    let current = window.outer_size().map_err(|err| err.to_string())?;
    if current.width != width
        && let Ok(position) = window.outer_position()
    {
        let shift = (current.width as i32 - width as i32) / 2;
        let _ = window.set_position(PhysicalPosition::new(position.x + shift, position.y));
    }
    window
        .set_size(PhysicalSize::new(width, height))
        .map_err(|err| err.to_string())?;
    Ok(())
}

/// Opens or focuses the floating notes window, on note `id` when given.
/// Opens notes: `id` in its own window, or with none, brings the open notes forward (or the
/// newest note when none is open). See note_windows.
pub fn open_notes(app: &AppHandle, id: Option<&str>) -> Result<(), String> {
    crate::note_windows::show(app, id)
}

/// Settings and notes are see-through over a blurred, darkened copy of what is behind them;
/// the page adds the purple tint. Until the page paints the window stays hidden.
pub(crate) const WINDOW_BACKGROUND: tauri::window::Color = tauri::window::Color(0, 0, 0, 0);

pub(crate) fn glass() -> tauri::utils::config::WindowEffectsConfig {
    use tauri::window::{Effect, EffectState, EffectsBuilder};
    EffectsBuilder::new()
        .effect(Effect::HudWindow)
        .effect(Effect::Acrylic)
        .state(EffectState::Active)
        .build()
}

/// The bar's effect in tauri.conf.json is macOS's HUD blur, which Windows ignores, leaving
/// the tint over the bare desktop. Windows gets Acrylic instead, kept dark to match the
/// theme, with the system's rounded corners clipping it (Windows rounds at 8px, so the page
/// matches that radius there).
#[cfg(windows)]
pub fn make_bar_glass(window: &tauri::WebviewWindow) -> Result<(), String> {
    use windows::Win32::Graphics::Dwm::{
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
    };

    window
        .set_theme(Some(tauri::Theme::Dark))
        .map_err(|err| err.to_string())?;
    window.set_effects(glass()).map_err(|err| err.to_string())?;
    let hwnd = window.hwnd().map_err(|err| err.to_string())?;
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &DWMWCP_ROUND as *const _ as *const _,
            std::mem::size_of_val(&DWMWCP_ROUND) as u32,
        )
        .map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// Shows a window built hidden once its page says it has drawn, or after a moment if it
/// never does.
pub(crate) fn reveal_soon(app: &AppHandle, window: tauri::WebviewWindow) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        if !window.is_visible().unwrap_or(true) {
            let _ = window.show();
            if !crate::note_windows::take_quiet(window.label()) {
                focus_on_main_thread(&app, window);
            }
        }
    });
}

/// Called by a settings or notes page after its first paint.
pub fn page_ready(app: &AppHandle, label: &str) {
    if let Some(window) = app.get_webview_window(label)
        && !window.is_visible().unwrap_or(true)
    {
        let _ = window.show();
        // A note reopened at start comes up without taking focus.
        if !crate::note_windows::take_quiet(label) {
            focus_on_main_thread(app, window);
        }
    }
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
    .background_color(WINDOW_BACKGROUND)
    .transparent(true)
    .effects(glass())
    .visible(false)
    .inner_size(760.0, 720.0)
    .min_inner_size(560.0, 480.0)
    .center()
    .resizable(true)
    .focused(true)
    .build()
    .map_err(|err| err.to_string())?;
    reveal_soon(app, window);
    Ok(())
}

// open_settings can be called from an async command's worker thread; the Win32 foreground
// dance attaches the calling thread's input queue, so it has to run on the UI thread.
/// Brings one of Zephyr's windows to the front and gives it the keyboard.
pub(crate) fn focus_window(app: &AppHandle, window: tauri::WebviewWindow) {
    focus_on_main_thread(app, window);
}

fn focus_on_main_thread(app: &AppHandle, window: tauri::WebviewWindow) {
    let result = app.run_on_main_thread(move || {
        force_foreground(&window);
        let _ = window.set_focus();
    });
    if let Err(err) = result {
        log::error!("couldn't focus settings: {err}");
    }
}

#[cfg(target_os = "macos")]
tauri_nspanel::tauri_panel! {
    panel!(BarPanel {
        config: {
            can_become_key_window: true,
            can_become_main_window: false,
            is_floating_panel: true
        }
    })
}

/// macOS keeps a normal window off another app's full-screen Space even when it is marked
/// to join all Spaces, and showing it activates Zephyr, which pulls the user back to the
/// desktop. Spotlight and Raycast use a non-activating panel instead: it floats over
/// full-screen apps and takes keystrokes without activating the app behind it.
#[cfg(target_os = "macos")]
pub fn make_bar_panel(window: &tauri::WebviewWindow) -> Result<(), String> {
    use objc2_app_kit::{NSStatusWindowLevel, NSWindowCollectionBehavior, NSWindowStyleMask};
    use tauri_nspanel::WebviewWindowExt;

    let panel = window
        .to_panel::<BarPanel>()
        .map_err(|err| err.to_string())?;
    panel
        .add_style_mask(NSWindowStyleMask::NonactivatingPanel)
        .map_err(|err| err.to_string())?;
    // MoveToActiveSpace, not CanJoinAllSpaces: the bar opens on whichever Space is current,
    // but belongs to that one Space while up, so swiping to another desktop leaves it behind
    // instead of carrying it along (it then hides; see `hide_on_space_change`).
    panel.set_collection_behavior(
        NSWindowCollectionBehavior::MoveToActiveSpace
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    panel.set_level(NSStatusWindowLevel as i64);
    panel.set_hides_on_deactivate(false);
    Ok(())
}

/// Closes the bar the moment the user switches desktops (a three-finger swipe, Ctrl+arrow),
/// like Spotlight, so it never lingers on the old Space or shows up on the new one. Its
/// view and text are kept, as with any other close.
#[cfg(target_os = "macos")]
pub fn hide_on_space_change(app: &AppHandle) {
    use objc2_app_kit::{NSWorkspace, NSWorkspaceActiveSpaceDidChangeNotification};
    use objc2_foundation::{NSNotification, NSOperationQueue};

    let app = app.clone();
    let block = block2::RcBlock::new(move |_: std::ptr::NonNull<NSNotification>| {
        let showing = app
            .get_webview_window("main")
            .is_some_and(|window| window.is_visible().unwrap_or(false));
        if showing {
            log::debug!("the Space changed, hiding the bar");
            hide_bar(&app);
        }
    });
    // SAFETY: a main-queue observer for a workspace notification; the block only touches
    // the app handle, and the observer lives as long as Zephyr.
    let observer = unsafe {
        NSWorkspace::sharedWorkspace()
            .notificationCenter()
            .addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceActiveSpaceDidChangeNotification),
                None,
                Some(&NSOperationQueue::mainQueue()),
                &block,
            )
    };
    std::mem::forget(observer);
}

/// Shows the bar panel and gives it the keyboard without activating Zephyr, so a
/// full-screen app stays on screen behind it.
#[cfg(target_os = "macos")]
fn show_panel(app: &AppHandle, window: &tauri::WebviewWindow) {
    use tauri_nspanel::ManagerExt;

    let Ok(panel) = app.get_webview_panel(window.label()) else {
        log::error!("the bar panel is missing");
        return;
    };
    let webview = window.clone();
    let result = app.run_on_main_thread(move || {
        panel.show_and_make_key();
        if let Err(err) = webview.as_ref().set_focus() {
            log::error!("couldn't focus the bar: {err}");
        }
    });
    if let Err(err) = result {
        log::error!("couldn't show the bar: {err}");
    }
}

/// Showing the bar activates Zephyr, and macOS leaves an app with no visible window active,
/// so after Esc keystrokes went nowhere until the user clicked back into their app.
#[cfg(target_os = "macos")]
mod previous_app {
    use std::sync::Mutex;

    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

    static PREVIOUS: Mutex<Option<Retained<NSRunningApplication>>> = Mutex::new(None);

    /// Remembers the app in front before the bar takes focus.
    pub fn remember() {
        let ours = NSRunningApplication::currentApplication().processIdentifier();
        let front = NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .filter(|app| app.processIdentifier() != ours);
        if let Ok(mut previous) = PREVIOUS.lock() {
            *previous = front;
        }
    }

    /// Reactivates the remembered app. Zephyr is still active here, so macOS honors it.
    pub fn restore() {
        let previous = PREVIOUS
            .lock()
            .ok()
            .and_then(|mut previous| previous.take());
        if let Some(app) = previous
            && !app.isTerminated()
        {
            app.activateWithOptions(NSApplicationActivationOptions::empty());
        }
    }
}

/// Opens the bar where the user last dragged it, pulled fully onto that screen, or centered
/// near the top of the screen with the cursor when there is no such spot or its monitor is
/// gone.
fn place_bar(window: &tauri::WebviewWindow) -> Result<(), String> {
    let monitors = window.available_monitors().map_err(|err| err.to_string())?;
    let saved = window
        .app_handle()
        .state::<crate::state::AppState>()
        .snapshot()
        .ok()
        .and_then(|snapshot| snapshot.bar_position);
    let on_saved_screen = saved.and_then(|spot| {
        // The monitor under the bar's top-left corner, nudged inside it.
        let corner = PhysicalPosition::new(spot.x as f64 + 24.0, spot.y as f64 + 12.0);
        monitors
            .iter()
            .find(|monitor| contains(monitor, corner))
            .map(|monitor| (monitor, spot))
    });
    let (monitor, spot) = match on_saved_screen {
        Some((monitor, spot)) => (monitor, Some(spot)),
        None => {
            let cursor = window.cursor_position().map_err(|err| err.to_string())?;
            let monitor = monitors
                .iter()
                .find(|monitor| contains(monitor, cursor))
                .or_else(|| monitors.first())
                .ok_or("No monitor is available")?;
            (monitor, None)
        }
    };

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
    let height = window
        .outer_size()
        .map(|size| size.height)
        .unwrap_or(120)
        .max(48);
    let area = (
        work.position.x,
        work.position.y,
        work.size.width as i32,
        work.size.height as i32,
    );
    let (x, y) = match spot {
        Some(spot) => clamp_into((spot.x, spot.y), area, width as i32, height as i32),
        None => (
            area.0 + ((area.2 - width as i32) / 2).max(0),
            area.1 + area.3 / 6,
        ),
    };

    window
        .set_size(PhysicalSize::new(width, height))
        .map_err(|err| err.to_string())?;
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|err| err.to_string())?;
    Ok(())
}

/// Moves a `width` x `height` box at `spot` the least needed to sit inside `area`
/// (x, y, width, height). A box taller than the area keeps its top edge on screen.
fn clamp_into(spot: (i32, i32), area: (i32, i32, i32, i32), width: i32, height: i32) -> (i32, i32) {
    let (left, top, area_width, area_height) = area;
    let x = spot.0.clamp(left, (left + area_width - width).max(left));
    let y = spot.1.clamp(top, (top + area_height - height).max(top));
    (x, y)
}

/// Where a dragged bar pulls to when it comes near: its center on the screen's center or
/// thirds across, and on the half or thirds down, plus the spot it opens at by default (top
/// edge a sixth of the way down). Elsewhere it moves freely. `area` is (x, y, width,
/// height) of the screen's work area; returns the snapped top-left.
#[cfg_attr(not(windows), allow(dead_code))]
fn snap_position(
    spot: (i32, i32),
    area: (i32, i32, i32, i32),
    width: i32,
    height: i32,
) -> (i32, i32) {
    let (left, top, area_width, area_height) = area;
    // About 24 px on a 1080p-class screen, scaled with the screen.
    let reach = (area_height / 45).max(12);
    let nearest = |value: i32, targets: &[i32]| {
        targets
            .iter()
            .copied()
            .filter(|target| (value - target).abs() <= reach)
            .min_by_key(|target| (value - target).abs())
            .unwrap_or(value)
    };
    let center_x = spot.0 + width / 2;
    let across = [
        left + area_width / 3,
        left + area_width / 2,
        left + area_width * 2 / 3,
    ];
    let x = nearest(center_x, &across) - width / 2;
    let center_y = spot.1 + height / 2;
    let down = [
        top + area_height / 3,
        top + area_height / 2,
        top + area_height * 2 / 3,
    ];
    let snapped_center = nearest(center_y, &down);
    let y = if snapped_center != center_y {
        snapped_center - height / 2
    } else {
        // The default spot is by the top edge, not the center.
        nearest(spot.1, &[top + area_height / 6])
    };
    (x, y)
}

/// Makes Windows' move loop snap the bar while the user drags it (see [`snap_position`]).
#[cfg(windows)]
pub fn install_snapping(window: &tauri::WebviewWindow) {
    use windows::Win32::UI::Shell::SetWindowSubclass;
    if let Ok(hwnd) = window.hwnd() {
        unsafe {
            let _ = SetWindowSubclass(hwnd, Some(snap_while_moving), 0x5a50, 0);
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn snap_while_moving(
    hwnd: windows::Win32::Foundation::HWND,
    message: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    _id: usize,
    _data: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::Shell::DefSubclassProc;
    use windows::Win32::UI::WindowsAndMessaging::WM_MOVING;

    // The bar follows the mouse untouched (pulling it onto guides mid-drag made it jumpy);
    // the grid just shows where it would land, and the drop puts it there.
    if message == WM_MOVING && DRAGGING.load(Ordering::SeqCst) && lparam.0 != 0 {
        let rect = unsafe { &*(lparam.0 as *const RECT) };
        grid::moving(rect);
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
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

#[cfg(test)]
mod tests {
    use super::{clamp_into, snap_position};

    #[test]
    fn a_dragged_bar_snaps_near_center_and_thirds_and_moves_freely_elsewhere() {
        // A 1920x1040 work area and an 850x600 bar.
        let area = (0, 0, 1920, 1040);
        let (w, h) = (850, 600);
        // Within reach of dead center across: x snaps to 535 (center 960).
        assert_eq!(snap_position((550, 100), area, w, h).0, 535);
        // Center down at 2/3 (693): y snaps to 393.
        assert_eq!(snap_position((800, 380), area, w, h), (800, 393));
        // The default spot, top edge at a sixth (173).
        assert_eq!(snap_position((800, 180), area, w, h).1, 173);
        // Far from every line: left alone.
        assert_eq!(snap_position((700, 120), area, w, h), (700, 120));
        // A second screen to the left snaps to its own center.
        assert_eq!(
            snap_position((-1730, 100), (-2560, 0, 2560, 1400), w, h).0,
            -1280 - w / 2
        );
    }

    #[test]
    fn a_dragged_bar_is_pulled_back_on_screen() {
        let area = (0, 0, 1920, 1040);
        assert_eq!(clamp_into((300, 200), area, 680, 400), (300, 200));
        assert_eq!(clamp_into((1700, 900), area, 680, 400), (1240, 640));
        assert_eq!(clamp_into((-50, -20), area, 680, 400), (0, 0));
        // A second monitor to the left, and a bar taller than the screen.
        assert_eq!(
            clamp_into((-3000, 100), (-2560, 0, 2560, 1400), 680, 1600),
            (-2560, 0)
        );
    }
}
