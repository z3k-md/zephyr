//! Bug captures, in dev and installed builds alike (Settings › Dev can turn them off). Ctrl+R
//! in any of the app's windows records the one in front (the bar, Notes or Settings) for up to
//! 30 seconds as PNG frames, plus a timestamped log of what its page did (for the bar: keys
//! pressed in it, the text, the results and the selected row, suggestion fetches, size
//! changes, errors), and Ctrl+Shift+S takes one frame with the page's state. The keys are the
//! pages' own (main.ts), not global, so other apps keep them. Each capture is a folder
//! under `captures/` in the app's data folder; the newest 20 are kept.
//!
//! Only the app's own windows are involved: frames are taken of that window alone while it is
//! showing (on macOS, of the window itself, which needs no Screen Recording permission), and
//! keys are logged by the app's pages, so nothing typed elsewhere is seen.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};

const LIMIT: Duration = Duration::from_secs(30);
const FRAME_EVERY: Duration = Duration::from_millis(150);
const KEEP: usize = 20;

struct Session {
    dir: PathBuf,
    started: Instant,
    events: Vec<Value>,
    stop: Arc<AtomicBool>,
}

static DIR: OnceLock<PathBuf> = OnceLock::new();
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

pub fn init(dir: PathBuf) {
    let _ = DIR.set(dir);
}

/// The captures folder, made if it isn't there yet.
pub fn folder() -> Result<PathBuf, String> {
    let dir = DIR.get().ok_or("Captures aren't set up")?;
    std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    Ok(dir.clone())
}

pub fn recording() -> bool {
    SESSION
        .lock()
        .map(|session| session.is_some())
        .unwrap_or(false)
}

pub fn toggle(app: &AppHandle) {
    if recording() {
        stop(app);
    } else if let Err(err) = start(app) {
        log::error!("couldn't start a capture: {err}");
    }
}

/// Adds an entry to the running capture's log; does nothing when not recording.
pub fn record(kind: &str, data: Value) {
    if let Ok(mut guard) = SESSION.lock()
        && let Some(session) = guard.as_mut()
    {
        let t = session.started.elapsed().as_millis() as u64;
        session
            .events
            .push(json!({ "t": t, "kind": kind, "data": data }));
    }
}

/// Opens a capture folder and its log; `suffix` marks snapshots apart from recordings.
fn begin(suffix: &str) -> Result<(PathBuf, Arc<AtomicBool>), String> {
    let root = DIR.get().ok_or("Captures aren't set up")?;
    let name = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let dir = root.join(format!("{name}{suffix}"));
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let stop_flag = Arc::new(AtomicBool::new(false));
    let mut guard = SESSION.lock().map_err(|_| "Captures are unavailable")?;
    *guard = Some(Session {
        dir: dir.clone(),
        started: Instant::now(),
        events: Vec::new(),
        stop: stop_flag.clone(),
    });
    Ok((dir, stop_flag))
}

/// The window a capture is of: the app's window in front (the bar, a note, Settings), else
/// the bar.
fn target(app: &AppHandle) -> String {
    app.webview_windows()
        .into_iter()
        .find(|(_, window)| window.is_focused().unwrap_or(false))
        .map(|(label, _)| label)
        .unwrap_or_else(|| "main".into())
}

/// Saves one frame of `label`'s window, if it is showing, as frame-NNNN.png.
fn save_frame(app: &AppHandle, label: &str, dir: &Path, frame: u32) -> bool {
    let Some(window) = app.get_webview_window(label) else {
        return false;
    };
    if !window.is_visible().unwrap_or(false) {
        return false;
    }
    let (Ok(position), Ok(size)) = (window.outer_position(), window.outer_size()) else {
        return false;
    };
    let rect = (position.x, position.y, size.width, size.height);
    #[cfg(target_os = "macos")]
    let png = grab_window(app, &window);
    #[cfg(not(target_os = "macos"))]
    let png = grab(rect);
    let Some(png) = png else {
        return false;
    };
    let file = format!("frame-{frame:04}.png");
    if std::fs::write(dir.join(&file), png).is_err() {
        return false;
    }
    record(
        "frame",
        json!({ "file": file, "rect": [rect.0, rect.1, rect.2, rect.3] }),
    );
    true
}

fn start(app: &AppHandle) -> Result<(), String> {
    let label = target(app);
    let (dir, stop_flag) = begin("")?;
    record("window", json!({ "label": label }));
    let _ = app.emit("capture-state", true);
    log::info!("capture started: {}", dir.display());

    let app = app.clone();
    std::thread::spawn(move || {
        let began = Instant::now();
        let mut frame = 0u32;
        while !stop_flag.load(Ordering::SeqCst) && began.elapsed() < LIMIT {
            let tick = Instant::now();
            if save_frame(&app, &label, &dir, frame + 1) {
                frame += 1;
            }
            std::thread::sleep(FRAME_EVERY.saturating_sub(tick.elapsed()));
        }
        if !stop_flag.load(Ordering::SeqCst) {
            stop(&app);
        }
    });
    Ok(())
}

/// Ctrl+Shift+S: one frame of the app's window in front as it is now, plus its page's state
/// (the bar's text, results and selected row; the open note) in the log. The frame is taken first, so the recording dot isn't in it.
pub fn snapshot(app: &AppHandle) {
    if recording() {
        return;
    }
    let label = target(app);
    let dir = match begin("-snap") {
        Ok((dir, _)) => dir,
        Err(err) => {
            log::error!("couldn't take a snapshot: {err}");
            return;
        }
    };
    record("window", json!({ "label": label }));
    if !save_frame(app, &label, &dir, 1) {
        record(
            "note",
            json!({ "text": "No Zephyr window was showing, so there is no frame" }),
        );
    } else if let Err(err) = std::fs::read(dir.join("frame-0001.png"))
        .map_err(|err| err.to_string())
        .and_then(crate::clipboard::copy_png)
    {
        // On the clipboard too, ready to paste.
        log::error!("couldn't copy the snapshot: {err}");
    }
    // The page answers with its state when told a capture is running.
    let _ = app.emit("capture-state", true);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        stop(&app);
    });
}

fn stop(app: &AppHandle) {
    let Some(session) = SESSION.lock().ok().and_then(|mut guard| guard.take()) else {
        return;
    };
    session.stop.store(true, Ordering::SeqCst);
    let log = json!({
        "seconds": session.started.elapsed().as_secs_f64(),
        "events": session.events,
    });
    let path = session.dir.join("events.json");
    match serde_json::to_vec_pretty(&log) {
        Ok(bytes) => {
            if let Err(err) = std::fs::write(&path, bytes) {
                log::error!("couldn't save the capture log: {err}");
            }
        }
        Err(err) => log::error!("couldn't save the capture log: {err}"),
    }
    let _ = app.emit("capture-state", false);
    log::info!("capture saved: {}", session.dir.display());
    if let Some(root) = DIR.get() {
        prune(root);
    }
}

/// Keeps the newest captures; folder names sort by time.
fn prune(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    let excess = dirs.len().saturating_sub(KEEP);
    for dir in dirs.into_iter().take(excess) {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// The screen inside `rect` (x, y, width, height in physical pixels) as PNG: what is actually
/// shown there, blur and transparency included.
#[cfg(windows)]
fn grab(rect: (i32, i32, u32, u32)) -> Option<Vec<u8>> {
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap,
        CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC,
        SRCCOPY, SelectObject,
    };
    let (x, y, width, height) = rect;
    let (w, h) = (width as i32, height as i32);
    if w <= 0 || h <= 0 {
        return None;
    }
    unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, w, h);
        let previous = SelectObject(memory, bitmap.into());
        let copied = BitBlt(memory, 0, 0, w, h, Some(screen), x, y, SRCCOPY | CAPTUREBLT).is_ok();
        if copied {
            draw_pointer(memory, x, y);
        }
        SelectObject(memory, previous);
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
        if !copied || rows != h {
            return None;
        }
        for pixel in bgra.as_chunks_mut::<4>().0.iter_mut() {
            let [blue, green, red, _] = *pixel;
            *pixel = [red, green, blue, 255];
        }
        let mut png = Vec::new();
        let mut encoder = png::Encoder::new(&mut png, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&bgra).ok()?;
        writer.finish().ok()?;
        Some(png)
    }
}

/// The mouse pointer, as it looks right now, onto a frame whose top left is the screen's
/// (`x`, `y`), so a snapshot can point at things. A screen copy leaves it out.
#[cfg(windows)]
unsafe fn draw_pointer(frame: windows::Win32::Graphics::Gdi::HDC, x: i32, y: i32) {
    use windows::Win32::Graphics::Gdi::DeleteObject;
    use windows::Win32::UI::WindowsAndMessaging::{
        CURSOR_SHOWING, CURSORINFO, DI_NORMAL, DrawIconEx, GetCursorInfo, GetIconInfo, ICONINFO,
    };
    unsafe {
        let mut cursor = CURSORINFO {
            cbSize: std::mem::size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        if GetCursorInfo(&mut cursor).is_err() || cursor.flags.0 & CURSOR_SHOWING.0 == 0 {
            return;
        }
        let mut icon = ICONINFO::default();
        if GetIconInfo(cursor.hCursor.into(), &mut icon).is_err() {
            return;
        }
        let left = cursor.ptScreenPos.x - x - icon.xHotspot as i32;
        let top = cursor.ptScreenPos.y - y - icon.yHotspot as i32;
        let _ = DrawIconEx(
            frame,
            left,
            top,
            cursor.hCursor.into(),
            0,
            0,
            0,
            None,
            DI_NORMAL,
        );
        // GetIconInfo hands over copies of the cursor's bitmaps.
        if !icon.hbmMask.is_invalid() {
            let _ = DeleteObject(icon.hbmMask.into());
        }
        if !icon.hbmColor.is_invalid() {
            let _ = DeleteObject(icon.hbmColor.into());
        }
    }
}

/// macOS will take frames with ScreenCaptureKit; until then a capture is the event log only.
/// The bar's own window as PNG, blur and transparency included. Capturing only the app's
/// window needs no Screen Recording permission, so this never prompts.
#[cfg(target_os = "macos")]
#[allow(deprecated)] // CGWindowListCreateImage; ScreenCaptureKit would need the permission.
fn grab_window(app: &AppHandle, window: &tauri::WebviewWindow) -> Option<Vec<u8>> {
    use objc2::AnyThread;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSWindow};
    use objc2_core_graphics::{CGWindowImageOption, CGWindowListCreateImage, CGWindowListOption};
    use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize};

    // AppKit windows belong to the main thread, so the window number is read there.
    let pointer = window.ns_window().ok()? as usize;
    let (sender, receiver) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        // SAFETY: `pointer` is the live NSWindow behind the bar, read on the main thread.
        let ns_window = unsafe { &*(pointer as *const NSWindow) };
        let _ = sender.send(ns_window.windowNumber());
    })
    .ok()?;
    let number = receiver.recv_timeout(Duration::from_secs(1)).ok()?;
    let number = u32::try_from(number).ok().filter(|number| *number > 0)?;

    objc2::rc::autoreleasepool(|_| {
        // CGRectNull: just the window's own bounds.
        let bounds = NSRect::new(
            NSPoint::new(f64::INFINITY, f64::INFINITY),
            NSSize::new(0.0, 0.0),
        );
        let image = CGWindowListCreateImage(
            bounds,
            CGWindowListOption::OptionIncludingWindow,
            number,
            CGWindowImageOption::BoundsIgnoreFraming | CGWindowImageOption::BestResolution,
        )?;
        let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &image);
        // SAFETY: an empty properties dictionary is valid for PNG.
        let png = unsafe {
            rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
        }?;
        Some(png.to_vec())
    })
}

#[cfg(not(any(windows, target_os = "macos")))]
fn grab(_rect: (i32, i32, u32, u32)) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_newest_captures_are_kept() {
        let root = std::env::temp_dir().join(format!("zephyr-captures-{}", std::process::id()));
        for index in 0..(KEEP + 3) {
            std::fs::create_dir_all(root.join(format!("20261010-0000{index:02}"))).unwrap();
        }
        prune(&root);
        let mut left: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left.len(), KEEP);
        assert_eq!(left[0], "20261010-000003");
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn grabs_a_screen_rectangle_as_png() {
        let png = grab((0, 0, 16, 8)).expect("a frame");
        assert!(png.starts_with(b"\x89PNG"));
    }
}
