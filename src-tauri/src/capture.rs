//! Dev-build bug captures: Ctrl+Alt+R records the bar for up to 30 seconds as PNG frames
//! plus a timestamped log of what the bar did (keys pressed in it, the text, the results and
//! the selected row, suggestion fetches, size changes, errors). Each capture is a folder
//! under `captures/` in Zephyr's data folder; the newest 20 are kept.
//!
//! Only Zephyr's own windows are involved: frames are taken of the bar's rectangle while it
//! is showing, and keys are logged by the bar's page, so nothing typed elsewhere is seen.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};

pub const SHORTCUT: &str = "ctrl+alt+r";
pub const SNAPSHOT_SHORTCUT: &str = "ctrl+alt+s";
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

/// Saves one frame of the bar, if it is showing, as frame-NNNN.png.
fn save_frame(app: &AppHandle, dir: &Path, frame: u32) -> bool {
    let Some(window) = app.get_webview_window("main") else {
        return false;
    };
    if !window.is_visible().unwrap_or(false) {
        return false;
    }
    let (Ok(position), Ok(size)) = (window.outer_position(), window.outer_size()) else {
        return false;
    };
    let rect = (position.x, position.y, size.width, size.height);
    let Some(png) = grab(rect) else {
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
    let (dir, stop_flag) = begin("")?;
    let _ = app.emit("capture-state", true);
    log::info!("capture started: {}", dir.display());

    let app = app.clone();
    std::thread::spawn(move || {
        let began = Instant::now();
        let mut frame = 0u32;
        while !stop_flag.load(Ordering::SeqCst) && began.elapsed() < LIMIT {
            let tick = Instant::now();
            if save_frame(&app, &dir, frame + 1) {
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

/// Ctrl+Alt+S: one frame of the bar as it is now, plus the bar's state (text, results,
/// selected row) in the log. The frame is taken first, so the recording dot isn't in it.
pub fn snapshot(app: &AppHandle) {
    if recording() {
        return;
    }
    let dir = match begin("-snap") {
        Ok((dir, _)) => dir,
        Err(err) => {
            log::error!("couldn't take a snapshot: {err}");
            return;
        }
    };
    if !save_frame(app, &dir, 1) {
        record(
            "note",
            json!({ "text": "The bar wasn't showing, so there is no frame" }),
        );
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

/// macOS will take frames with ScreenCaptureKit; until then a capture is the event log only.
#[cfg(not(windows))]
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
