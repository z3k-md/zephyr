//! The screen color picker. The bar hides, the screen is captured (the whole virtual
//! desktop on Windows, the main display on macOS), and a borderless window shows that
//! frame with a loupe. The window is made the first time and reused. Picking copies the
//! hex the way Zephyr's other copies do, and history files it as a color.

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder,
};

/// Enough for several 4K displays, not an unbounded allocation.
const MAX_PIXELS: u64 = 64_000_000;

struct Frame {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    /// Where the captured area starts, in physical pixels. Negative when a display sits
    /// to the left of the origin.
    x: i32,
    y: i32,
}

/// `Idle` is no picker. `Starting` is the capture, before the window is up. `Open` is the
/// window on screen. Start, choose and cancel hold [`ops`] for the whole call, so a second
/// hotkey can't capture the picker or race the first one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Session {
    Idle,
    Starting,
    Open,
}

struct Gate {
    session: Session,
    frame: Option<Frame>,
}

/// One start, choose or cancel at a time.
fn ops() -> &'static Mutex<()> {
    static OPS: OnceLock<Mutex<()>> = OnceLock::new();
    OPS.get_or_init(|| Mutex::new(()))
}

fn gate() -> &'static Mutex<Gate> {
    static GATE: OnceLock<Mutex<Gate>> = OnceLock::new();
    GATE.get_or_init(|| {
        Mutex::new(Gate {
            session: Session::Idle,
            frame: None,
        })
    })
}

fn lock_ops() -> std::sync::MutexGuard<'static, ()> {
    ops().lock().unwrap_or_else(|poison| poison.into_inner())
}

fn lock_gate() -> std::sync::MutexGuard<'static, Gate> {
    gate().lock().unwrap_or_else(|poison| poison.into_inner())
}

#[derive(Debug, PartialEq, Eq)]
enum ServeDecision {
    /// The picker window's `GET /frame`.
    Allow,
    /// Some other Zephyr window. The screenshot stays put.
    Deny,
    /// The picker asked for something that isn't the frame.
    Missing,
}

/// The captured screen is only for the picker window's `GET /frame`.
fn serve_decision(webview_label: &str, method: &str, path: &str) -> ServeDecision {
    if webview_label != "picker" {
        return ServeDecision::Deny;
    }
    if method != "GET" || path != "/frame" {
        return ServeDecision::Missing;
    }
    ServeDecision::Allow
}

/// Origins the picker page is actually loaded from. The frame names one of these in
/// `Access-Control-Allow-Origin`, so the page can read `X-Width` and `X-Height` on Windows,
/// where `http://picker.localhost` is a different origin from the app. Anything else is
/// left unnamed: `*` would let any page that reached the response read the screenshot.
fn frame_cors_origin(origin: Option<&str>) -> Option<&'static str> {
    match origin? {
        "http://tauri.localhost" => Some("http://tauri.localhost"),
        "https://tauri.localhost" => Some("https://tauri.localhost"),
        "tauri://localhost" => Some("tauri://localhost"),
        "http://localhost:1420" => Some("http://localhost:1420"),
        _ => None,
    }
}

fn apply_cors(
    builder: tauri::http::response::Builder,
    request: &tauri::http::Request<Vec<u8>>,
) -> tauri::http::response::Builder {
    let origin = request
        .headers()
        .get(tauri::http::header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    let Some(allowed) = frame_cors_origin(origin) else {
        return builder;
    };
    builder
        .header(tauri::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, allowed)
        .header(
            tauri::http::header::ACCESS_CONTROL_EXPOSE_HEADERS,
            "X-Width, X-Height",
        )
        .header(tauri::http::header::VARY, "Origin")
}

fn response(
    status: u16,
    request: Option<&tauri::http::Request<Vec<u8>>>,
) -> tauri::http::Response<Vec<u8>> {
    let mut builder = tauri::http::Response::builder().status(status);
    if let Some(request) = request {
        builder = apply_cors(builder, request);
    }
    builder
        .body(Vec::new())
        .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}

/// `picker://` (and `http://picker.localhost` on Windows): the current frame as raw RGBA,
/// with its size in headers. Only the picker window's `GET /frame` is served, and the bytes
/// are handed off once — the page loads the frame once per show. Nothing is served once the
/// picker closes.
pub fn serve(
    webview_label: &str,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    match serve_decision(
        webview_label,
        request.method().as_str(),
        request.uri().path(),
    ) {
        ServeDecision::Deny => return response(403, None),
        ServeDecision::Missing => return response(404, Some(&request)),
        ServeDecision::Allow => {}
    }
    let Some(frame) = lock_gate().frame.take() else {
        return response(404, Some(&request));
    };
    let builder = apply_cors(
        tauri::http::Response::builder()
            .header(
                tauri::http::header::CONTENT_TYPE,
                "application/octet-stream",
            )
            .header(tauri::http::header::CACHE_CONTROL, "no-store")
            .header("X-Width", frame.width.to_string())
            .header("X-Height", frame.height.to_string()),
        &request,
    );
    builder
        .body(frame.rgba)
        .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}

/// Hide the bar, capture, and show the picker. A missing Screen Recording permission on
/// macOS is reported on the bar instead. A session that is already starting or open is
/// focused and left as it is.
pub fn start(app: &AppHandle) -> Result<(), String> {
    let _ops = lock_ops();
    {
        let mut gate = lock_gate();
        if gate.session != Session::Idle {
            drop(gate);
            focus_picker(app);
            return Ok(());
        }
        gate.session = Session::Starting;
    }
    // Any return before the window is up, including a panic, drops the session.
    let mut opening = Opening { app, open: false };

    if let Err(err) = ensure_screen_recording(app) {
        tell(app, &err);
        return Err(err);
    }
    crate::window::hide_bar(app);
    // The bar is still in the frame until the compositor drops it.
    std::thread::sleep(Duration::from_millis(90));
    let frame = match capture_screen() {
        Ok(frame) => frame,
        Err(err) => {
            tell(app, &err);
            return Err(err);
        }
    };
    let (x, y, width, height) = (frame.x, frame.y, frame.width, frame.height);
    lock_gate().frame = Some(frame);
    if let Err(err) = present(app, x, y, width, height) {
        tell(app, &err);
        return Err(err);
    }
    lock_gate().session = Session::Open;
    opening.open = true;
    Ok(())
}

/// Copy `hex` and reopen the bar with it typed in, so the color card shows. A failed copy
/// still closes the picker — the page has already dropped its frame — and the error is
/// shown on the bar.
pub fn choose(app: &AppHandle, hex: &str) -> Result<(), String> {
    let _ops = lock_ops();
    let copied: Result<String, String> = (|| {
        let hex = normalize_hex(hex).ok_or_else(|| "That isn't a color".to_string())?;
        crate::clipboard::copy_text(&hex)?;
        Ok(hex)
    })();
    end_session(app);
    match copied {
        Ok(hex) => {
            crate::window::show_bar(app);
            let _ = app.emit_to(
                "main",
                "bar-input",
                serde_json::json!({ "query": hex, "notice": serde_json::Value::Null }),
            );
            Ok(())
        }
        Err(err) => {
            tell(app, &err);
            Err(err)
        }
    }
}

pub fn cancel(app: &AppHandle) {
    let _ops = lock_ops();
    end_session(app);
}

/// Clears a capture that never reached [`Session::Open`].
struct Opening<'a> {
    app: &'a AppHandle,
    open: bool,
}

impl Drop for Opening<'_> {
    fn drop(&mut self) {
        if !self.open {
            end_session(self.app);
        }
    }
}

fn end_session(app: &AppHandle) {
    {
        let mut gate = lock_gate();
        gate.session = Session::Idle;
        gate.frame = None;
    }
    if let Some(window) = app.get_webview_window("picker") {
        let _ = window.hide();
    }
}

fn focus_picker(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("picker") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn tell(app: &AppHandle, message: &str) {
    crate::window::show_bar(app);
    let _ = app.emit_to(
        "main",
        "bar-input",
        serde_json::json!({ "query": "", "notice": message }),
    );
}

/// `#rrggbb`, the form clipboard history files as a color.
fn normalize_hex(text: &str) -> Option<String> {
    let hex = text.trim().strip_prefix('#')?;
    if hex.len() == 6 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Some(format!("#{}", hex.to_ascii_lowercase()))
    } else {
        None
    }
}

fn ensure_screen_recording(app: &AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use tauri_plugin_opener::OpenerExt;
        if !objc2_core_graphics::CGPreflightScreenCaptureAccess() {
            // Shows the system prompt the first time. A later denial returns at once.
            let granted = objc2_core_graphics::CGRequestScreenCaptureAccess();
            if !granted {
                if let Some(url) = crate::permissions::pane("screen") {
                    let _ = app.opener().open_url(url, None::<&str>);
                }
                return Err("Zephyr needs Screen Recording to pick a color. Turn it on in System Settings, under Privacy & Security, then try again.".into());
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
    Ok(())
}

fn present(app: &AppHandle, x: i32, y: i32, width: u32, height: u32) -> Result<(), String> {
    let window = match app.get_webview_window("picker") {
        Some(window) => window,
        None => make_window(app)?,
    };
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|err| err.to_string())?;
    window
        .set_size(PhysicalSize::new(width, height))
        .map_err(|err| err.to_string())?;
    #[cfg(target_os = "macos")]
    cover_menu_bar(&window);
    window.show().map_err(|err| err.to_string())?;
    let _ = window.set_focus();
    let _ = window.emit("picker-shown", ());
    Ok(())
}

fn make_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    WebviewWindowBuilder::new(
        app,
        "picker",
        WebviewUrl::App("index.html?view=picker".into()),
    )
    .title("Pick a color")
    .background_color(tauri::window::Color(0, 0, 0, 255))
    .transparent(false)
    .decorations(false)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .visible(false)
    .focused(false)
    .build()
    .map_err(|err| err.to_string())
}

/// Above the menu bar, so a pixel there can be picked too. The main display only.
#[cfg(target_os = "macos")]
fn cover_menu_bar(window: &tauri::WebviewWindow) {
    use objc2_app_kit::{NSScreenSaverWindowLevel, NSWindow, NSWindowCollectionBehavior};
    let Ok(ns) = window.ns_window() else {
        return;
    };
    let pointer = ns as usize;
    let _ = window.run_on_main_thread(move || {
        // SAFETY: `pointer` is the live NSWindow, used on the main thread.
        let ns_window = unsafe { &*(pointer as *const NSWindow) };
        ns_window.setLevel(NSScreenSaverWindowLevel);
        ns_window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
    });
}

/// Per-monitor copies count only when every one of them succeeded. Otherwise the caller
/// copies the whole virtual screen, and that has to succeed on its own. The capture is
/// Windows-only; tests on every platform check the rule.
#[cfg(any(windows, test))]
fn monitors_copied(any: bool, failed: bool) -> bool {
    any && !failed
}

fn capture_screen() -> Result<Frame, String> {
    #[cfg(windows)]
    {
        capture_windows()
    }
    #[cfg(target_os = "macos")]
    {
        capture_macos()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Err("Picking a color isn't available on this system yet".into())
    }
}

#[cfg(windows)]
fn capture_windows() -> Result<Frame, String> {
    use windows::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
    };
    // SAFETY: the awareness is set for this thread only, and put back before returning.
    unsafe {
        let previous = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let result = capture_virtual();
        if !previous.0.is_null() {
            SetThreadDpiAwarenessContext(previous);
        }
        result
    }
}

#[cfg(windows)]
fn capture_virtual() -> Result<Frame, String> {
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, EnumDisplayMonitors, GetDC, GetDIBits, HDC,
        HMONITOR, ReleaseDC, SRCCOPY, SelectObject,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };

    struct Blit {
        memory: HDC,
        screen: HDC,
        origin_x: i32,
        origin_y: i32,
        /// At least one monitor copied.
        any: bool,
        /// One monitor's BitBlt failed. The partial bitmap is not the screen.
        failed: bool,
    }

    unsafe extern "system" fn blit_monitor(
        _monitor: HMONITOR,
        _hdc: HDC,
        rect: *mut RECT,
        data: LPARAM,
    ) -> windows_core::BOOL {
        if rect.is_null() || data.0 == 0 {
            return windows_core::BOOL(1);
        }
        // SAFETY: `data` is the Blit EnumDisplayMonitors is walking, alive for the call.
        let blit = unsafe { &mut *(data.0 as *mut Blit) };
        let rect = unsafe { &*rect };
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        if w > 0 && h > 0 {
            let copied = unsafe {
                BitBlt(
                    blit.memory,
                    rect.left - blit.origin_x,
                    rect.top - blit.origin_y,
                    w,
                    h,
                    Some(blit.screen),
                    rect.left,
                    rect.top,
                    SRCCOPY,
                )
            };
            if copied.is_ok() {
                blit.any = true;
            } else {
                blit.failed = true;
            }
        }
        windows_core::BOOL(1)
    }

    // SAFETY: GDI objects made here are released on every path below.
    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if w <= 0 || h <= 0 || (w as u64).saturating_mul(h as u64) > MAX_PIXELS {
            return Err("Couldn't see the screen".into());
        }
        let screen = GetDC(None);
        if screen.is_invalid() {
            return Err("Couldn't read the screen".into());
        }
        let memory = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, w, h);
        if memory.is_invalid() || bitmap.is_invalid() {
            if !memory.is_invalid() {
                let _ = DeleteDC(memory);
            }
            if !bitmap.is_invalid() {
                let _ = DeleteObject(bitmap.into());
            }
            ReleaseDC(None, screen);
            return Err("Couldn't read the screen".into());
        }
        let previous = SelectObject(memory, bitmap.into());
        if previous.is_invalid() {
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);
            return Err("Couldn't read the screen".into());
        }
        let mut blit = Blit {
            memory,
            screen,
            origin_x: x,
            origin_y: y,
            any: false,
            failed: false,
        };
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(blit_monitor),
            LPARAM(&mut blit as *mut Blit as isize),
        );
        // One failed monitor spoils the set. Copy the whole virtual screen instead, and
        // if that fails too there is no frame.
        if !monitors_copied(blit.any, blit.failed)
            && BitBlt(memory, 0, 0, w, h, Some(screen), x, y, SRCCOPY).is_err()
        {
            SelectObject(memory, previous);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);
            return Err("Couldn't read the screen".into());
        }
        SelectObject(memory, previous);
        let (width, height) = (w as u32, h as u32);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bgra = vec![0u8; (width * height * 4) as usize];
        let rows = GetDIBits(
            memory,
            bitmap,
            0,
            height,
            Some(bgra.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        if rows != h {
            return Err("Couldn't read the screen".into());
        }
        straight_rgba(&mut bgra);
        Ok(Frame {
            rgba: bgra,
            width,
            height,
            x,
            y,
        })
    }
}

/// Premultiplied BGRA from GDI to straight RGBA. A capture with no alpha (every pixel's 0)
/// is opaque, which is what BitBlt of the screen returns.
#[cfg_attr(not(windows), allow(dead_code))]
fn straight_rgba(pixels: &mut [u8]) {
    let opaque = pixels.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 0);
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let [blue, green, red, alpha] = *pixel;
        let straight = |channel: u8| match alpha {
            _ if opaque => channel,
            0 => 0,
            255 => channel,
            _ => (u16::from(channel) * 255 / u16::from(alpha)).min(255) as u8,
        };
        *pixel = [
            straight(red),
            straight(green),
            straight(blue),
            if opaque { 255 } else { alpha },
        ];
    }
}

/// The main display, through CoreGraphics. ScreenCaptureKit would be the newer call; this
/// one is what a still of the display needs, and it asks for Screen Recording.
#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn capture_macos() -> Result<Frame, String> {
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGColorSpaceCreateDeviceRGB, CGContext, CGDisplayBounds,
        CGDisplayCreateImage, CGImage, CGMainDisplayID,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    // kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big: bytes come out R, G, B, A.
    const RGBA: u32 = 1 | (4 << 12);

    let id = CGMainDisplayID();
    let Some(image) = CGDisplayCreateImage(id) else {
        return Err(
            "Couldn't read the screen. Zephyr needs Screen Recording to pick a color.".into(),
        );
    };
    let width = CGImage::width(Some(&image));
    let height = CGImage::height(Some(&image));
    if width == 0 || height == 0 || (width as u64).saturating_mul(height as u64) > MAX_PIXELS {
        return Err("Couldn't read the screen".into());
    }
    let mut rgba = vec![0u8; width * height * 4];
    let Some(space) = CGColorSpaceCreateDeviceRGB() else {
        return Err("Couldn't read the screen".into());
    };
    // SAFETY: `rgba` outlives the context, which is dropped before the buffer is moved.
    let context = unsafe {
        CGBitmapContextCreate(
            rgba.as_mut_ptr().cast(),
            width,
            height,
            8,
            width * 4,
            Some(&space),
            RGBA,
        )
    };
    let Some(context) = context else {
        return Err("Couldn't read the screen".into());
    };
    let rect = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(width as f64, height as f64),
    );
    CGContext::draw_image(Some(&context), rect, Some(&image));
    drop(context);
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let alpha = pixel[3];
        if alpha == 0 {
            pixel[3] = 255;
        } else if alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel = (u16::from(*channel) * 255 / u16::from(alpha)).min(255) as u8;
            }
            pixel[3] = 255;
        }
    }
    let bounds = CGDisplayBounds(id);
    let scale = if bounds.size.width > 0.0 {
        width as f64 / bounds.size.width
    } else {
        1.0
    };
    Ok(Frame {
        rgba,
        width: width as u32,
        height: height as u32,
        x: (bounds.origin.x * scale).round() as i32,
        y: (bounds.origin.y * scale).round() as i32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picked_color_is_six_hex_digits() {
        assert_eq!(normalize_hex("#AaBbCc").as_deref(), Some("#aabbcc"));
        assert_eq!(normalize_hex("  #ff00aa"), Some("#ff00aa".into()));
        assert_eq!(normalize_hex("#fff"), None);
        assert_eq!(normalize_hex("ff0000"), None);
        assert_eq!(normalize_hex("#ff00aagg"), None);
    }

    #[test]
    fn only_the_picker_window_may_read_the_frame() {
        assert_eq!(serve_decision("main", "GET", "/frame"), ServeDecision::Deny);
        assert_eq!(
            serve_decision("picker", "POST", "/frame"),
            ServeDecision::Missing
        );
        assert_eq!(
            serve_decision("picker", "GET", "/shot"),
            ServeDecision::Missing
        );
        assert_eq!(
            serve_decision("picker", "GET", "/frame"),
            ServeDecision::Allow
        );
        assert_eq!(
            frame_cors_origin(Some("http://tauri.localhost")),
            Some("http://tauri.localhost")
        );
        assert_eq!(
            frame_cors_origin(Some("http://localhost:1420")),
            Some("http://localhost:1420")
        );
        assert_eq!(frame_cors_origin(Some("https://evil.example")), None);
        assert_eq!(frame_cors_origin(Some("*")), None);
        assert_eq!(frame_cors_origin(None), None);
    }

    fn frame_request(origin: Option<&str>) -> tauri::http::Request<Vec<u8>> {
        let mut builder = tauri::http::Request::builder()
            .method("GET")
            .uri("http://picker.localhost/frame?t=1");
        if let Some(origin) = origin {
            builder = builder.header("Origin", origin);
        }
        builder.body(Vec::new()).unwrap()
    }

    #[test]
    fn the_frame_is_handed_off_once_and_only_to_the_picker() {
        {
            let mut gate = lock_gate();
            gate.session = Session::Idle;
            gate.frame = None;
        }
        let missing = serve("picker", frame_request(Some("http://tauri.localhost")));
        assert_eq!(missing.status(), 404);

        {
            let mut gate = lock_gate();
            gate.frame = Some(Frame {
                rgba: vec![1, 2, 3, 4],
                width: 1,
                height: 1,
                x: 0,
                y: 0,
            });
        }
        let denied = serve("main", frame_request(Some("http://tauri.localhost")));
        assert_eq!(denied.status(), 403);
        assert!(
            denied
                .headers()
                .get("access-control-allow-origin")
                .is_none()
        );

        let response = serve("picker", frame_request(Some("http://tauri.localhost")));
        assert_eq!(response.status(), 200);
        assert_eq!(response.body(), &vec![1, 2, 3, 4]);
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some("http://tauri.localhost")
        );
        assert_ne!(
            response
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some("*")
        );
        assert_eq!(
            response
                .headers()
                .get("x-width")
                .and_then(|value| value.to_str().ok()),
            Some("1")
        );

        let again = serve("picker", frame_request(Some("http://tauri.localhost")));
        assert_eq!(again.status(), 404);
        let mut gate = lock_gate();
        gate.session = Session::Idle;
        gate.frame = None;
    }

    #[test]
    fn a_failed_monitor_is_not_a_successful_capture() {
        assert!(monitors_copied(true, false));
        assert!(!monitors_copied(true, true));
        assert!(!monitors_copied(false, false));
        assert!(!monitors_copied(false, true));
    }

    #[test]
    fn gdi_pixels_become_straight_rgba() {
        let mut pixels = [0, 0, 128, 128, 0, 0, 0, 0, 255, 0, 0, 255];
        straight_rgba(&mut pixels);
        assert_eq!(pixels, [255, 0, 0, 128, 0, 0, 0, 0, 0, 0, 255, 255]);
        let mut pixels = [10, 20, 30, 0, 40, 50, 60, 0];
        straight_rgba(&mut pixels);
        assert_eq!(pixels, [30, 20, 10, 255, 60, 50, 40, 255]);
    }
}
