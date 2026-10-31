//! Updates from GitHub Releases, checked at launch, whenever the bar is summoned and the last
//! check is over three minutes old, and from "Check for updates". A small card in the
//! screen's corner shows the download, then counts down a few seconds before installing:
//! Install now skips the wait, and Later puts it off, with a button in the bar to install it
//! whenever suits. The installer isn't held in memory meanwhile: installing a put-off update
//! downloads it again, and checks its signature again.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::timer::Debounce;
use crate::util::now_ms;

const FIRST_CHECK_DELAY: Duration = Duration::from_secs(5);
/// A summon checks again once the last check is this old, so a release just published shows
/// up within minutes of using the app, while a busy day of summons checks rarely.
const STALE_AFTER: Duration = Duration::from_secs(3 * 60);
/// Covers the check and the installer download. Without it a stalled request would hold
/// `CHECKING` forever and block every later check.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
/// How long the card counts down before installing on its own.
const COUNTDOWN: Duration = Duration::from_secs(5);

static CHECKING: AtomicBool = AtomicBool::new(false);
/// When the last check started.
static LAST_CHECK: Mutex<Option<Instant>> = Mutex::new(None);
/// Installs when the countdown runs out; Later and Install now cancel it.
static COUNTDOWN_TIMER: Debounce = Debounce::new();
/// Later pressed while downloading: keep the download, but don't count down when it's done.
static DEFERRED: AtomicBool = AtomicBool::new(false);

/// What the update card shows.
#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "phase",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Status {
    Downloading {
        version: String,
        downloaded: u64,
        total: Option<u64>,
    },
    Ready {
        version: String,
        /// When it installs on its own, in ms since the epoch.
        install_at: u64,
    },
    Installing {
        version: String,
    },
    Failed {
        version: String,
        message: String,
    },
}

/// A downloaded update waiting to install. `update` is none for the dev build's rehearsal.
struct Downloaded {
    version: String,
    update: Option<Update>,
    /// The verified installer, while the countdown runs. Later lets it go (it's tens of MB),
    /// and installing downloads it again, checking its signature again.
    bytes: Option<Vec<u8>>,
}

impl Downloaded {
    /// Put off with Later: only what's needed to fetch it again stays.
    fn put_off(&mut self) {
        self.bytes = None;
    }
}

/// Lets go of a put-off update's installer.
fn put_off_download() {
    if let Ok(mut saved) = downloaded_slot().lock()
        && let Some(saved) = saved.as_mut()
    {
        saved.put_off();
    }
}

fn status_slot() -> &'static Mutex<Option<Status>> {
    static STATUS: OnceLock<Mutex<Option<Status>>> = OnceLock::new();
    STATUS.get_or_init(Default::default)
}

fn downloaded_slot() -> &'static Mutex<Option<Downloaded>> {
    static DOWNLOADED: OnceLock<Mutex<Option<Downloaded>>> = OnceLock::new();
    DOWNLOADED.get_or_init(Default::default)
}

/// The check at launch, a few seconds in.
pub fn spawn_background_checks(app: AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK_DELAY);
        background_check(&app);
    });
}

/// Called as the bar shows: checks again when the last check is over three minutes old.
pub fn check_if_stale(app: &AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    let stale = LAST_CHECK
        .lock()
        .map(|last| last.is_none_or(|at| at.elapsed() >= STALE_AFTER))
        .unwrap_or(false);
    if stale {
        let app = app.clone();
        std::thread::spawn(move || background_check(&app));
    }
}

fn background_check(app: &AppHandle) {
    match tauri::async_runtime::block_on(check(app, false)) {
        Ok(message) => log::info!("{message}"),
        Err(err) => log::warn!("{err}"),
    }
}

/// "Check for updates", from Settings or the tray.
pub async fn check_for_updates(app: &AppHandle) -> Result<String, String> {
    check(app, true).await
}

/// A check by hand offers an update put off with Later again; one in the background leaves
/// it to the button in the bar.
async fn check(app: &AppHandle, by_hand: bool) -> Result<String, String> {
    if cfg!(debug_assertions) {
        return Ok("Update checks are skipped in dev builds.".into());
    }
    if CHECKING.swap(true, Ordering::SeqCst) {
        return Ok("Already checking for updates.".into());
    }
    if let Ok(mut last) = LAST_CHECK.lock() {
        *last = Some(Instant::now());
    }
    let result = download_latest(app, by_hand).await;
    CHECKING.store(false, Ordering::SeqCst);
    result
}

async fn download_latest(app: &AppHandle, by_hand: bool) -> Result<String, String> {
    let updater = app
        .updater_builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|err| err.to_string())?;
    let Some(update) = updater
        .check()
        .await
        .map_err(|err| format!("Update check failed: {err}"))?
    else {
        return Ok("You're on the latest version.".into());
    };
    let version = update.version.clone();

    // Downloaded before and put off with Later: a check by hand offers it again without
    // downloading; otherwise the button in the bar waits for the user.
    let have = downloaded_slot()
        .lock()
        .ok()
        .and_then(|saved| saved.as_ref().map(|saved| saved.version == version))
        .unwrap_or(false);
    if have {
        if by_hand {
            count_down(app, &version);
        }
        return Ok(format!("Update {version} is ready."));
    }

    DEFERRED.store(false, Ordering::SeqCst);
    log::info!("downloading update {version}");
    let mut progress = Progress::new(app, &version);
    let bytes = update
        .download(|chunk, total| progress.add(chunk as u64, total), || {})
        .await;
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(err) => {
            let message = format!("Couldn't download update {version}: {err}");
            publish(
                app,
                Status::Failed {
                    version,
                    message: message.clone(),
                },
            );
            return Err(message);
        }
    };
    keep(&version, Some(update), bytes);
    if DEFERRED.swap(false, Ordering::SeqCst) {
        put_off_download();
        announce_waiting(app);
        return Ok(format!(
            "Update {version} is downloaded; the button in the bar installs it."
        ));
    }
    count_down(app, &version);
    Ok(format!(
        "Update {version} is downloaded and installs in {} seconds.",
        COUNTDOWN.as_secs()
    ))
}

fn keep(version: &str, update: Option<Update>, bytes: Vec<u8>) {
    if let Ok(mut saved) = downloaded_slot().lock() {
        *saved = Some(Downloaded {
            version: version.to_string(),
            update,
            bytes: Some(bytes),
        });
    }
}

/// Reports download progress to the card, a few times a second at most.
struct Progress<'a> {
    app: &'a AppHandle,
    version: String,
    downloaded: u64,
    sent: Option<Instant>,
}

impl<'a> Progress<'a> {
    fn new(app: &'a AppHandle, version: &str) -> Self {
        publish(
            app,
            Status::Downloading {
                version: version.to_string(),
                downloaded: 0,
                total: None,
            },
        );
        Self {
            app,
            version: version.to_string(),
            downloaded: 0,
            sent: None,
        }
    }

    fn add(&mut self, chunk: u64, total: Option<u64>) {
        self.downloaded += chunk;
        let done = total.is_some_and(|total| self.downloaded >= total);
        if !done
            && self
                .sent
                .is_some_and(|sent| sent.elapsed() < Duration::from_millis(100))
        {
            return;
        }
        self.sent = Some(Instant::now());
        publish(
            self.app,
            Status::Downloading {
                version: self.version.clone(),
                downloaded: self.downloaded,
                total,
            },
        );
    }
}

/// Shows the countdown, and installs when it runs out unless Later or Install now came first.
fn count_down(app: &AppHandle, version: &str) {
    publish(
        app,
        Status::Ready {
            version: version.to_string(),
            install_at: now_ms() + COUNTDOWN.as_millis() as u64,
        },
    );
    let app = app.clone();
    COUNTDOWN_TIMER.schedule(COUNTDOWN, move || install_now(&app));
}

/// Installs the downloaded update. On Windows the installer closes the app and reopens it;
/// on macOS the new app is put in place and restarted into.
pub fn install_now(app: &AppHandle) {
    COUNTDOWN_TIMER.cancel();
    let Some(downloaded) = downloaded_slot()
        .lock()
        .ok()
        .and_then(|mut saved| saved.take())
    else {
        return;
    };
    announce_waiting(app);
    let version = downloaded.version.clone();
    publish(
        app,
        Status::Installing {
            version: version.clone(),
        },
    );
    let app = app.clone();
    std::thread::spawn(move || {
        // Lets the card draw "Installing" before the installer takes over.
        std::thread::sleep(Duration::from_millis(400));
        let Some(update) = downloaded.update else {
            log::info!("rehearsal: update {version} would install now");
            std::thread::sleep(Duration::from_millis(1600));
            hide(&app);
            return;
        };
        let bytes = match downloaded.bytes {
            Some(bytes) => bytes,
            None => match download_again(&app, &update, &version) {
                Ok(bytes) if !DEFERRED.swap(false, Ordering::SeqCst) => bytes,
                // Later again while it downloaded.
                Ok(_) => {
                    if let Ok(mut saved) = downloaded_slot().lock() {
                        *saved = Some(Downloaded {
                            version,
                            update: Some(update),
                            bytes: None,
                        });
                    }
                    announce_waiting(&app);
                    return;
                }
                Err(message) => {
                    publish(&app, Status::Failed { version, message });
                    return;
                }
            },
        };
        // On Windows the installer closes the app without an Exit event; logged now, so
        // scripts/logscan.mjs doesn't take the update for a crash.
        log::info!("Zephyr exiting to install update {version}");
        match update.install(&bytes) {
            Ok(()) => {
                // Windows never gets here: the installer has already closed the app.
                log::info!("update {version} installed, restarting");
                app.restart();
            }
            Err(err) => publish(
                &app,
                Status::Failed {
                    version,
                    message: format!("Couldn't install the update: {err}"),
                },
            ),
        }
    });
}

/// An update put off with Later, fetched again to install. The download checks the
/// signature, as the first one did.
fn download_again(app: &AppHandle, update: &Update, version: &str) -> Result<Vec<u8>, String> {
    log::info!("downloading update {version} again to install it");
    DEFERRED.store(false, Ordering::SeqCst);
    let mut progress = Progress::new(app, version);
    let bytes = tauri::async_runtime::block_on(
        update.download(|chunk, total| progress.add(chunk as u64, total), || {}),
    )
    .map_err(|err| format!("Couldn't download update {version}: {err}"))?;
    publish(
        app,
        Status::Installing {
            version: version.to_string(),
        },
    );
    Ok(bytes)
}

/// Puts the update off: the card goes away, and which update it was is kept, for the button
/// in the bar or the next check by hand. The installer itself isn't kept.
pub fn later(app: &AppHandle) {
    COUNTDOWN_TIMER.cancel();
    if matches!(current(), Some(Status::Downloading { .. })) {
        DEFERRED.store(true, Ordering::SeqCst);
    } else {
        put_off_download();
    }
    hide(app);
    announce_waiting(app);
}

/// A downloaded update put off with Later, by version: the bar shows a button for it.
pub fn waiting() -> Option<String> {
    if current().is_some() {
        return None;
    }
    downloaded_slot()
        .lock()
        .ok()?
        .as_ref()
        .map(|saved| saved.version.clone())
}

/// Tells the bar whether an update is waiting, so its button comes and goes.
fn announce_waiting(app: &AppHandle) {
    let _ = app.emit_to("main", "update-waiting", waiting());
}

/// What the card shows now, for a card that just opened.
pub fn current() -> Option<Status> {
    status_slot().lock().ok()?.clone()
}

fn hide(app: &AppHandle) {
    if let Ok(mut status) = status_slot().lock() {
        *status = None;
    }
    if let Some(window) = app.get_webview_window("update") {
        let _ = window.hide();
    }
}

fn publish(app: &AppHandle, status: Status) {
    // After Later, the rest of the download happens out of sight.
    if DEFERRED.load(Ordering::SeqCst) && matches!(status, Status::Downloading { .. }) {
        return;
    }
    if let Status::Failed { message, .. } = &status {
        log::error!("update failed: {message}");
    }
    if let Ok(mut saved) = status_slot().lock() {
        *saved = Some(status.clone());
    }
    show_card(app);
    let _ = app.emit_to("update", "update-status", &status);
}

/// Opens the update card in the corner of the main screen, without taking focus from
/// whatever is in front.
fn show_card(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let window = match handle.get_webview_window("update") {
            Some(window) => window,
            None => match build_card(&handle) {
                Ok(window) => window,
                Err(err) => {
                    log::error!("couldn't open the update card: {err}");
                    return;
                }
            },
        };
        if !window.is_visible().unwrap_or(false) {
            place_card(&window);
            let _ = window.show();
        }
    });
}

const CARD_WIDTH: f64 = 360.0;
const CARD_HEIGHT: f64 = 124.0;

fn build_card(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    let builder = WebviewWindowBuilder::new(
        app,
        "update",
        WebviewUrl::App("index.html?view=update".into()),
    )
    .title("Zephyr update")
    .inner_size(CARD_WIDTH, CARD_HEIGHT)
    .transparent(true)
    .decorations(false)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .focused(false)
    .visible(false);
    // Clicks still reach the buttons; the game or document in front keeps the keyboard.
    #[cfg(windows)]
    let builder = builder.focusable(false);
    // macOS: the theme's glass under the card, as on every other window; without it the
    // Sidebar theme's light tint left the card see-through.
    #[cfg(target_os = "macos")]
    let builder = builder.effects(crate::window::rounded_glass(12.0));
    let window = builder.build().map_err(|err| err.to_string())?;
    #[cfg(windows)]
    if let Err(err) = crate::window::make_bar_glass(&window) {
        log::debug!("the update card has no blur: {err}");
    }
    Ok(window)
}

fn place_card(window: &tauri::WebviewWindow) {
    let Ok(Some(monitor)) = window.primary_monitor() else {
        return;
    };
    let area = monitor.work_area();
    let scale = monitor.scale_factor();
    let margin = (16.0 * scale) as i32;
    let width = (CARD_WIDTH * scale) as i32;
    let height = (CARD_HEIGHT * scale) as i32;
    let x = area.position.x + area.size.width as i32 - width - margin;
    let y = area.position.y + area.size.height as i32 - height - margin;
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
}

/// Dev builds can't install a real update, so this plays the whole card through with a
/// made-up version: a download, the countdown and "Installing", which installs nothing.
#[cfg(debug_assertions)]
pub fn rehearse(app: &AppHandle) {
    if CHECKING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let version = "9.9.9".to_string();
        DEFERRED.store(false, Ordering::SeqCst);
        let total: u64 = 24 * 1024 * 1024;
        let steps = 60;
        let mut progress = Progress::new(&app, &version);
        for _ in 0..steps {
            std::thread::sleep(Duration::from_millis(60));
            progress.add(total / steps, Some(total));
        }
        progress.add(total % steps, Some(total));
        keep(&version, None, Vec::new());
        CHECKING.store(false, Ordering::SeqCst);
        if !DEFERRED.swap(false, Ordering::SeqCst) {
            count_down(&app, &version);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_put_off_update_lets_its_installer_go_but_stays_waiting() {
        let mut saved = Downloaded {
            version: "2.11.0".into(),
            update: None,
            bytes: Some(vec![0; 64]),
        };
        saved.put_off();
        assert_eq!(saved.version, "2.11.0");
        assert!(saved.bytes.is_none());
    }
}
