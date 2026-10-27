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
/// Bumped per drag, so a drop settling late doesn't end the next drag.
static DRAG_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Starts moving the bar. On macOS the page then drives the move (see `free_drag`) and
/// gets back where the bar is and the screen's scale.
pub fn start_bar_drag(app: &AppHandle) -> Result<Option<(i32, i32, f64)>, String> {
    let window = app
        .get_webview_window("main")
        .ok_or("Main window is missing")?;
    DRAGGING.store(true, Ordering::SeqCst);
    DRAG_GENERATION.fetch_add(1, Ordering::SeqCst);
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
            end_bar_drag();
        });
        result.map(|_| None).map_err(|err| err.to_string())
    }
    #[cfg(target_os = "macos")]
    {
        let _ = window;
        let started = free_drag::begin(app);
        // The page won't report a drop for a drag that never began.
        if started.is_none() {
            settle_after_move();
            end_bar_drag();
        }
        Ok(started)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let result = window.start_dragging().map(|_| None).map_err(|err| {
            log::error!("couldn't move the bar: {err}");
            err.to_string()
        });
        settle_after_move();
        result
    }
}

/// A drag out of the bar has started: focus moves to wherever it's dropped, so the bar
/// mustn't hide on blur until it ends.
pub fn begin_drag_out() {
    SUPPRESS_BLUR.store(true, Ordering::SeqCst);
}

/// The drag ended: a drop hides the bar (the file went somewhere), a cancel keeps it.
pub fn end_drag_out(app: &AppHandle, dropped: bool) {
    if dropped {
        SUPPRESS_BLUR.store(false, Ordering::SeqCst);
        hide_bar(app);
    } else {
        settle_after_move();
    }
}

/// The drop has landed. Once its last move is saved, later moves are the code's (re-centering
/// the wider clipboard view), not the user's, so they stop being remembered.
#[cfg(any(windows, target_os = "macos"))]
fn end_bar_drag() {
    let generation = DRAG_GENERATION.load(Ordering::SeqCst);
    std::thread::spawn(move || {
        // Longer than `bar_moved`'s wait, so the drop's own position is still saved.
        std::thread::sleep(Duration::from_millis(500));
        if DRAG_GENERATION.load(Ordering::SeqCst) == generation {
            DRAGGING.store(false, Ordering::SeqCst);
        }
    });
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

/// macOS: the bar's move, driven by the page. ⌘-drag sends the pointer's position and the
/// bar follows; the same grid as on Windows shows where it would snap, and on release it
/// lands there. (Windows uses its own move loop instead; see `drag_on_windows`.)
#[cfg(target_os = "macos")]
pub mod free_drag {
    use std::sync::Mutex;

    use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};

    struct Drag {
        /// The work area the grid covers: x, y, width, height, in physical pixels.
        area: (i32, i32, i32, i32),
        snap: Option<(i32, i32)>,
    }

    static DRAG: Mutex<Option<Drag>> = Mutex::new(None);

    fn work_area(window: &tauri::WebviewWindow) -> Option<(i32, i32, i32, i32)> {
        let monitor = window.current_monitor().ok().flatten()?;
        let area = monitor.work_area();
        Some((
            area.position.x,
            area.position.y,
            area.size.width as i32,
            area.size.height as i32,
        ))
    }

    fn place(app: &AppHandle, area: (i32, i32, i32, i32)) {
        if let Some(grid) = app.get_webview_window("grid") {
            let _ = grid.set_position(PhysicalPosition::new(area.0, area.1));
            let _ = grid.set_size(PhysicalSize::new(area.2 as u32, area.3 as u32));
            let _ = grid.show();
        }
    }

    /// Starts a move: shows the grid over the bar's screen. Returns where the bar is and
    /// the screen's scale, which the page needs to turn pointer moves into window moves.
    pub fn begin(app: &AppHandle) -> Option<(i32, i32, f64)> {
        let bar = app.get_webview_window("main")?;
        let position = bar.outer_position().ok()?;
        let scale = bar.scale_factor().unwrap_or(1.0);
        let area = work_area(&bar).unwrap_or_default();
        let _ = app.emit_to("grid", "grid-preview", serde_json::Value::Null);
        place(app, area);
        if let Ok(mut drag) = DRAG.lock() {
            *drag = Some(Drag { area, snap: None });
        }
        Some((position.x, position.y, scale))
    }

    /// The bar's new top-left: moves it, follows it to another screen, previews the snap.
    pub fn moving(app: &AppHandle, x: i32, y: i32) {
        let Some(bar) = app.get_webview_window("main") else {
            return;
        };
        let _ = bar.set_position(PhysicalPosition::new(x, y));
        let Ok(mut guard) = DRAG.lock() else {
            return;
        };
        let Some(drag) = guard.as_mut() else {
            return;
        };
        if let Some(area) = work_area(&bar)
            && area != drag.area
        {
            drag.area = area;
            place(app, area);
        }
        let Ok(size) = bar.outer_size() else {
            return;
        };
        let (width, height) = (size.width as i32, size.height as i32);
        let (sx, sy) = super::snap_position((x, y), drag.area, width, height);
        drag.snap = ((sx, sy) != (x, y)).then_some((sx, sy));
        let preview = drag.snap.map(|(sx, sy)| {
            serde_json::json!({
                "x": sx - drag.area.0,
                "y": sy - drag.area.1,
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

    /// The drop: hides the grid and puts the bar where it snapped, if it did.
    pub fn end(app: &AppHandle) {
        let Some(drag) = DRAG.lock().ok().and_then(|mut guard| guard.take()) else {
            return;
        };
        if let Some(grid) = app.get_webview_window("grid") {
            let _ = grid.hide();
        }
        if let (Some((x, y)), Some(bar)) = (drag.snap, app.get_webview_window("main")) {
            let _ = bar.set_position(PhysicalPosition::new(x, y));
        }
        super::settle_after_move();
        super::end_bar_drag();
    }
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
    // Before the bar takes the front, so the list is in the order the user left it.
    crate::running::refresh();
    crate::apps::AppIndex::refresh_if_stale(crate::apps::index());
    crate::files::index().refresh_if_stale();
    let Some(window) = app.get_webview_window("main") else {
        log::error!("main window is missing");
        return;
    };

    // Still being drawn off screen at startup counts as hidden.
    let was_visible = !unseen::finish(app, &window) && window.is_visible().unwrap_or(false);
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
    crate::stats::set_bar_visible(true);
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
        .is_some_and(|window| !unseen::drawing(&window) && window.is_visible().unwrap_or(false));
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
        crate::stats::set_bar_visible(false);
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

/// Settings and notes are see-through over a blurred, darkened copy of what is behind them;
/// the page adds the purple tint. Until the page paints the window stays hidden.
pub(crate) const WINDOW_BACKGROUND: tauri::window::Color = tauri::window::Color(0, 0, 0, 0);

pub(crate) fn glass() -> tauri::utils::config::WindowEffectsConfig {
    glass_for(None)
}

/// The glass for a frameless window with rounded corners (the update card; Windows gives
/// it Acrylic through `make_bar_glass`).
#[cfg(target_os = "macos")]
pub(crate) fn rounded_glass(radius: f64) -> tauri::utils::config::WindowEffectsConfig {
    glass_for(Some(radius))
}

/// macOS: Settings and setup open on whichever Space is current (a second desktop, a
/// full-screen app's Space) rather than pulling the user back to the one they were built
/// on, which is what happened when the app became active.
pub fn open_on_active_space(app: &AppHandle, window: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
        let Ok(pointer) = window.ns_window() else {
            return;
        };
        let pointer = pointer as usize;
        let _ = app.run_on_main_thread(move || {
            // SAFETY: the window's live NSWindow, used on the main thread.
            let ns_window = unsafe { &*(pointer as *const NSWindow) };
            ns_window.setCollectionBehavior(
                NSWindowCollectionBehavior::MoveToActiveSpace
                    | NSWindowCollectionBehavior::FullScreenAuxiliary,
            );
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, window);
}

/// macOS: while Settings or setup is open the app acts like a regular one, with a Dock
/// icon and a place in ⌘Tab, so other windows can come in front and switching back is
/// easy. Once they close it goes back to living in the menu bar.
pub fn regular_while_open(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let open = ["settings", "onboarding"].iter().any(|label| {
            app.get_webview_window(label)
                .is_some_and(|window| window.is_visible().unwrap_or(false))
        });
        let _ = app.set_activation_policy(if open {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// The native glass under a window, the same for every theme: macOS's sidebar material (the
/// System Settings sidebar) and a graphite-tinted Acrylic on Windows. Themes only tint it in
/// the page.
fn glass_for(radius: Option<f64>) -> tauri::utils::config::WindowEffectsConfig {
    use tauri::window::{Effect, EffectState, EffectsBuilder};
    let mut effects = EffectsBuilder::new()
        .effect(Effect::Sidebar)
        .effect(Effect::Acrylic)
        .state(EffectState::Active)
        .color(tauri::window::Color(43, 43, 48, 150));
    if let Some(radius) = radius {
        effects = effects.radius(radius);
    }
    effects.build()
}

/// The bar's effect in tauri.conf.json is macOS's sidebar material, which Windows ignores,
/// leaving the tint over the bare desktop. Windows gets Acrylic instead, kept dark to match the
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
/// never does. Meanwhile it draws unseen, so it never appears blank.
pub(crate) fn reveal_soon(app: &AppHandle, window: tauri::WebviewWindow) {
    take_reported(window.label());
    unseen::draw(app, &window);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        // A page that reported in was already shown; if it's hidden now, the user hid it.
        if take_reported(window.label()) {
            return;
        }
        if unseen::finish(&app, &window) || !window.is_visible().unwrap_or(true) {
            log::info!(
                "{} never reported in; shown by the 1.5 s fallback",
                window.label()
            );
            let _ = window.show();
            regular_while_open(&app);
            if !crate::note_windows::take_quiet(window.label()) {
                focus_window(&app, window);
            }
        }
    });
}

/// Windows whose page has reported in since `reveal_soon`, so its fallback leaves them be.
static REPORTED: std::sync::Mutex<Option<std::collections::HashSet<String>>> =
    std::sync::Mutex::new(None);

/// Whether `label`'s page reported in, clearing it.
fn take_reported(label: &str) -> bool {
    REPORTED
        .lock()
        .is_ok_and(|mut set| set.as_mut().is_some_and(|set| set.remove(label)))
}

/// Called by a settings or notes page after its first paint.
pub fn page_ready(app: &AppHandle, label: &str) {
    if label != "main"
        && let Ok(mut set) = REPORTED.lock()
    {
        set.get_or_insert_with(Default::default)
            .insert(label.to_string());
    }
    // Settings is built ahead of time: it stays hidden until asked for.
    if label == "settings" && !SETTINGS_WANTED.load(Ordering::SeqCst) {
        // Hidden before its page was ready: take it back off screen if it was drawing unseen,
        // since the fallback no longer will.
        if let Some(window) = app.get_webview_window(label) {
            unseen::put_away(app, &window);
        }
        return;
    }
    // The bar shows only when summoned; its page reports in once, drawn, and goes back off
    // screen until then.
    if label == "main" {
        crate::startup::mark("bar page loaded");
        if let Some(window) = app.get_webview_window(label)
            && unseen::drawing(&window)
        {
            // Its first list can predate the app scan, so once that's done it draws the list
            // again, and gets a moment longer for that to paint.
            let app = app.clone();
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                while !crate::apps::index().scanned() && started.elapsed() < Duration::from_secs(3)
                {
                    std::thread::sleep(Duration::from_millis(50));
                }
                let _ = app.emit_to("main", "bar-redraw", ());
                std::thread::sleep(Duration::from_millis(300));
                unseen::put_away(&app, &window);
            });
        }
        return;
    }
    if let Some(window) = app.get_webview_window(label)
        && (unseen::finish(app, &window) || !window.is_visible().unwrap_or(true))
    {
        let _ = window.show();
        regular_while_open(app);
        // A note reopened at start comes up without taking focus.
        if !crate::note_windows::take_quiet(label) {
            focus_window(app, window);
        }
    }
}

/// Whether the Settings window should appear once its page is ready (false while it's
/// being built in the background ahead of time).
static SETTINGS_WANTED: AtomicBool = AtomicBool::new(false);

/// Opens or focuses the settings window, scrolled to `section` when one is given. The
/// window is built ahead of time and only ever hidden, so this is just show and focus.
pub fn open_settings(app: &AppHandle, section: Option<&str>) -> Result<(), String> {
    SETTINGS_WANTED.store(true, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.emit("settings-shown", section.unwrap_or_default());
        let _ = window.unminimize();
        let _ = window.show();
        regular_while_open(app);
        focus_window(app, window);
        return Ok(());
    }
    build_settings(app, section)
}

/// Esc in Settings, or its close button: hides it, ready to show again instantly.
pub fn hide_settings(app: &AppHandle) {
    SETTINGS_WANTED.store(false, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.hide();
    }
    regular_while_open(app);
}

/// The fast path for the gear, called on the main thread: shows the pre-built Settings
/// window in place. False when it hasn't been built yet.
pub fn show_settings_now(app: &AppHandle) -> bool {
    let Some(window) = app.get_webview_window("settings") else {
        return false;
    };
    SETTINGS_WANTED.store(true, Ordering::SeqCst);
    let _ = window.unminimize();
    let _ = window.show();
    regular_while_open(app);
    force_foreground(&window);
    let _ = window.set_focus();
    let _ = window.emit("settings-shown", "");
    true
}

/// Builds the Settings window, hidden; `open_settings` or its page's first paint shows it.
fn build_settings(app: &AppHandle, section: Option<&str>) -> Result<(), String> {
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
    .focused(true);
    // The page draws its own title strip in the theme, with the sidebar running up to the
    // top: Windows drops the system's title bar (keeping the shadow and rounded corners) and
    // the page adds the window buttons; macOS keeps its traffic lights over the page.
    #[cfg(windows)]
    let window = window.decorations(false).shadow(true);
    #[cfg(target_os = "macos")]
    let window = window
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    let window = window.build().map_err(|err| err.to_string())?;
    #[cfg(windows)]
    no_window_menu(app, &window);
    open_on_active_space(app, &window);
    // Closing only hides it, so the next open is instant.
    let hidden = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = hidden.hide();
            SETTINGS_WANTED.store(false, Ordering::SeqCst);
            regular_while_open(hidden.app_handle());
        }
    });
    if SETTINGS_WANTED.load(Ordering::SeqCst) {
        reveal_soon(app, window);
    }
    Ok(())
}

/// First-run setup: a small fixed window with a few quick steps, shown once.
pub fn open_onboarding(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("onboarding") {
        let _ = window.show();
        regular_while_open(app);
        focus_window(app, window);
        return Ok(());
    }
    let window = WebviewWindowBuilder::new(
        app,
        "onboarding",
        WebviewUrl::App("index.html?view=onboarding".into()),
    )
    .title("Welcome to Zephyr")
    .background_color(WINDOW_BACKGROUND)
    .transparent(true)
    .effects(glass())
    .visible(false)
    .inner_size(560.0, 520.0)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .center()
    .focused(true)
    .build()
    .map_err(|err| err.to_string())?;
    #[cfg(windows)]
    no_window_menu(app, &window);
    // Closed with its close button rather than finished: the Dock icon goes with it.
    let handle = app.clone();
    let closing = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { .. } = event {
            let _ = closing.hide();
            regular_while_open(&handle);
        }
    });
    open_on_active_space(app, &window);
    reveal_soon(app, window);
    Ok(())
}

/// Builds Settings in the background a moment after startup, once the bar is ready, so
/// the gear never waits on a webview being made and its page loading.
pub fn prewarm_settings(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let built = app.get_webview_window("settings").is_some()
            || match build_settings(&app, None) {
                Ok(()) => true,
                Err(err) => {
                    log::error!("couldn't prepare Settings: {err}");
                    false
                }
            };
        // WebKit doesn't lay out or paint a page that has never been on screen, so the
        // first open still paid for it. Once loaded, it gets one invisible frame on screen.
        #[cfg(target_os = "macos")]
        if built {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            paint_offscreen(&app);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = built;
    });
}

/// Puts the hidden Settings window on screen fully transparent, behind everything and
/// without focus, long enough for WebKit to paint it, then takes it off again.
#[cfg(target_os = "macos")]
fn paint_offscreen(app: &AppHandle) {
    use objc2_app_kit::NSWindow;
    let Some(window) = app.get_webview_window("settings") else {
        return;
    };
    if SETTINGS_WANTED.load(Ordering::SeqCst) {
        return;
    }
    let Ok(pointer) = window.ns_window() else {
        return;
    };
    let pointer = pointer as usize;
    let _ = app.run_on_main_thread(move || {
        // SAFETY: the Settings NSWindow, alive while its WebviewWindow is, on the main thread.
        let ns_window = unsafe { &*(pointer as *const NSWindow) };
        ns_window.setAlphaValue(0.0);
        ns_window.orderBack(None);
    });
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        let _ = app.run_on_main_thread(move || {
            // SAFETY: as above.
            let ns_window = unsafe { &*(pointer as *const NSWindow) };
            if !SETTINGS_WANTED.load(Ordering::SeqCst) {
                ns_window.orderOut(None);
            }
            ns_window.setAlphaValue(1.0);
        });
    });
}

/// Brings one of the app's windows to the front and gives it the keyboard. Callers may be on
/// an async command's worker thread; the Win32 foreground dance attaches the calling thread's
/// input queue, so it runs on the UI thread.
pub(crate) fn focus_window(app: &AppHandle, window: tauri::WebviewWindow) {
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
/// to join all Spaces, and showing it activates the app, which pulls the user back to the
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
    // the app handle, and the observer lives as long as the app.
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

/// Shows the bar panel and gives it the keyboard without activating the app, so a
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

/// Showing the bar activates the app, and macOS leaves an app with no visible window active,
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

    /// Reactivates the remembered app. The app is still active here, so macOS honors it.
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
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
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

#[cfg(windows)]
static MENU_APP: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

/// Windows: Settings and setup have no use for the window menu, and Alt+Space opened it over
/// a shortcut being recorded (and Alt alone moved focus to it). Their window drops it.
#[cfg(windows)]
fn no_window_menu(app: &AppHandle, window: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{SC_KEYMENU, WM_SYSCOMMAND};

    unsafe extern "system" fn proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        if message == WM_SYSCOMMAND && (wparam.0 & 0xFFF0) as u32 == SC_KEYMENU {
            // The page never sees Alt+Space (the webview hands it to the window menu), so
            // while a shortcut is being recorded it's passed on to the page from here.
            if lparam.0 == 0x20
                && crate::shortcut::recording()
                && let Some(app) = MENU_APP.get()
            {
                use windows::Win32::UI::Input::KeyboardAndMouse::{
                    GetKeyState, VK_CONTROL, VK_LWIN, VK_RWIN, VK_SHIFT,
                };
                // SAFETY: plain key state reads.
                let down = |key: u16| unsafe { GetKeyState(i32::from(key)) } < 0;
                let _ = app.emit(
                    "alt-space",
                    serde_json::json!({
                        "ctrl": down(VK_CONTROL.0),
                        "shift": down(VK_SHIFT.0),
                        "meta": down(VK_LWIN.0) || down(VK_RWIN.0),
                    }),
                );
            }
            return LRESULT(0);
        }
        // SAFETY: the default handling for every other message.
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }

    let _ = MENU_APP.set(app.clone());
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let hwnd = hwnd.0 as isize;
    // A window is subclassed on the thread that owns it.
    let _ = app.run_on_main_thread(move || {
        // SAFETY: a live window handle and a subclass procedure that lives as long as the app.
        unsafe {
            let _ = SetWindowSubclass(HWND(hwnd as *mut _), Some(proc), 1, 0);
        }
    });
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

/// macOS: WebKit neither paints a page nor runs its animation frames while the window has
/// never been on screen, so a window built hidden showed up blank and then flickered as it
/// drew, and its page couldn't report that it had drawn. Until it has, the window sits on
/// screen fully transparent, behind everything, letting clicks through.
///
/// Windows: a WebView2 window's first show put up a stale frame and faded in from it, so
/// there the window is shown cloaked (DWM composes it but never puts it on screen) and
/// without focus until its page has drawn.
mod unseen {
    use std::collections::HashSet;
    use std::sync::Mutex;

    use tauri::AppHandle;

    static DRAWING: Mutex<Option<HashSet<String>>> = Mutex::new(None);

    pub fn drawing(window: &tauri::WebviewWindow) -> bool {
        DRAWING
            .lock()
            .is_ok_and(|set| set.as_ref().is_some_and(|set| set.contains(window.label())))
    }

    /// Puts `window` on screen, unseen, so its page draws.
    pub fn draw(app: &AppHandle, window: &tauri::WebviewWindow) {
        if !cfg!(any(target_os = "macos", windows)) {
            return;
        }
        if let Ok(mut set) = DRAWING.lock() {
            set.get_or_insert_with(HashSet::new)
                .insert(window.label().to_string());
        }
        set_unseen(app, window, true);
    }

    /// Makes the window seeable again (still where it is); true if it was being drawn unseen.
    pub fn finish(app: &AppHandle, window: &tauri::WebviewWindow) -> bool {
        let was = stop(window);
        if was {
            set_unseen(app, window, false);
        }
        was
    }

    /// Takes a window drawn unseen back off screen, then makes it seeable for next time; true
    /// if it was being drawn unseen.
    pub fn put_away(app: &AppHandle, window: &tauri::WebviewWindow) -> bool {
        let was = stop(window);
        #[cfg(target_os = "macos")]
        if was {
            use objc2_app_kit::NSWindow;
            let Ok(pointer) = window.ns_window() else {
                return was;
            };
            let pointer = pointer as usize;
            let _ = app.run_on_main_thread(move || {
                // SAFETY: the window's live NSWindow, used on the main thread. Off screen
                // first, so it never shows for a frame on the way.
                let ns_window = unsafe { &*(pointer as *const NSWindow) };
                ns_window.orderOut(None);
                ns_window.setAlphaValue(1.0);
                ns_window.setIgnoresMouseEvents(false);
            });
        }
        #[cfg(windows)]
        if was {
            use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};
            let Ok(hwnd) = window.hwnd() else {
                return was;
            };
            let hwnd = hwnd.0 as isize;
            let _ = app.run_on_main_thread(move || {
                let hwnd = windows::Win32::Foundation::HWND(hwnd as *mut _);
                // SAFETY: the window's live handle, on the thread that owns it. Hidden
                // first, so it never shows for a frame on the way.
                unsafe {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
                cloak(hwnd, false);
            });
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        let _ = app;
        was
    }

    fn stop(window: &tauri::WebviewWindow) -> bool {
        DRAWING
            .lock()
            .is_ok_and(|mut set| set.as_mut().is_some_and(|set| set.remove(window.label())))
    }

    fn set_unseen(app: &AppHandle, window: &tauri::WebviewWindow, unseen: bool) {
        #[cfg(target_os = "macos")]
        {
            use objc2_app_kit::NSWindow;
            let Ok(pointer) = window.ns_window() else {
                return;
            };
            let pointer = pointer as usize;
            let _ = app.run_on_main_thread(move || {
                // SAFETY: the window's live NSWindow, used on the main thread.
                let ns_window = unsafe { &*(pointer as *const NSWindow) };
                ns_window.setIgnoresMouseEvents(unseen);
                ns_window.setAlphaValue(if unseen { 0.0 } else { 1.0 });
                if unseen {
                    ns_window.orderBack(None);
                }
            });
        }
        #[cfg(windows)]
        {
            use windows::Win32::UI::WindowsAndMessaging::{SW_SHOWNOACTIVATE, ShowWindow};
            let Ok(hwnd) = window.hwnd() else {
                return;
            };
            let hwnd = hwnd.0 as isize;
            let _ = app.run_on_main_thread(move || {
                let hwnd = windows::Win32::Foundation::HWND(hwnd as *mut _);
                cloak(hwnd, unseen);
                if unseen {
                    // Shown behind Tauri's back, so it still counts the window hidden and its
                    // own show later goes through. SAFETY: live handle, owning thread.
                    unsafe {
                        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                    }
                }
            });
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        let _ = (app, window, unseen);
    }

    #[cfg(windows)]
    fn cloak(hwnd: windows::Win32::Foundation::HWND, on: bool) {
        use windows::Win32::Graphics::Dwm::{DWMWA_CLOAK, DwmSetWindowAttribute};
        use windows::core::BOOL;
        let value = BOOL::from(on);
        // SAFETY: a live window handle and a BOOL-sized attribute.
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_CLOAK,
                &value as *const _ as *const _,
                std::mem::size_of::<BOOL>() as u32,
            );
        }
    }
}

/// Draws the bar unseen once at startup, so the first summon shows a page that has already
/// painted. Its page reporting in puts it away again; if it never does, a fallback does.
pub fn draw_bar_unseen(app: &AppHandle) {
    if !cfg!(any(target_os = "macos", windows)) {
        return;
    }
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    unseen::draw(app, &window);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(5));
        if unseen::put_away(&app, &window) {
            log::info!("the bar never reported in; put away by the 5 s fallback");
        }
    });
}
