use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

const FIRST_CHECK_DELAY: Duration = Duration::from_secs(5);
/// Zephyr lives in the tray for weeks at a time, so a launch-only check is not enough.
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// Covers the check and the installer download. Without it a stalled request would hold
/// `CHECKING` forever and block every later check.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

static CHECKING: AtomicBool = AtomicBool::new(false);

pub fn spawn_background_checks(app: AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK_DELAY);
        loop {
            match tauri::async_runtime::block_on(check_for_updates(&app)) {
                Ok(message) => log::info!("{message}"),
                Err(err) => log::warn!("{err}"),
            }
            std::thread::sleep(CHECK_INTERVAL);
        }
    });
}

pub async fn check_for_updates(app: &AppHandle) -> Result<String, String> {
    if cfg!(debug_assertions) {
        return Ok("Update checks are skipped in dev builds.".into());
    }
    if CHECKING.swap(true, Ordering::SeqCst) {
        return Ok("Already checking for updates.".into());
    }
    let result = install_latest(app).await;
    CHECKING.store(false, Ordering::SeqCst);
    result
}

async fn install_latest(app: &AppHandle) -> Result<String, String> {
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
    log::info!("installing update {version}");
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|err| format!("Couldn't install update {version}: {err}"))?;

    // Windows never gets here: the installer closes Zephyr and relaunches it when done.
    // On macOS the new bundle is already in place and only needs a relaunch.
    log::info!("update {version} installed, restarting");
    app.restart()
}
