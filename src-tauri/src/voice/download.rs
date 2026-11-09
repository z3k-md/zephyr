//! Speech-model download. The HTTP work runs on `spawn_blocking` because `model::download`
//! uses a blocking client. Progress is pushed to every window; samples are not involved.
//!
//! One download runs at a time.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use super::engine::{self, LoadReport};
use super::model;
use super::{model_installed, model_path, reregister};

const TICK: Duration = Duration::from_millis(100);

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCard {
    pub name: String,
    pub size_label: String,
    pub installed: bool,
    pub downloading: bool,
    pub received: u64,
    pub total: u64,
    pub size_bytes: u64,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub model: ModelCard,
    /// "Loaded in …" or "Not loaded yet".
    pub loaded_label: String,
    pub load_error: Option<String>,
    pub error: Option<String>,
    pub license: String,
}

struct State {
    running: bool,
    cancel: Option<std::sync::Arc<AtomicBool>>,
    received: u64,
    total: u64,
    error: Option<String>,
    /// Released once the current download returns.
    finished: Vec<Sender<()>>,
}

fn state() -> &'static Mutex<State> {
    static STATE: Mutex<State> = Mutex::new(State {
        running: false,
        cancel: None,
        received: 0,
        total: 0,
        error: None,
        finished: Vec::new(),
    });
    &STATE
}

fn lock() -> std::sync::MutexGuard<'static, State> {
    state().lock().unwrap_or_else(|err| err.into_inner())
}

pub fn running() -> bool {
    lock().running
}

fn card(state: &State) -> ModelCard {
    let installed = model_path().is_ok_and(|dir| model::installed(&dir));
    ModelCard {
        name: model::DISPLAY_NAME.to_string(),
        size_label: model::SIZE_LABEL.to_string(),
        installed,
        downloading: state.running,
        received: if state.running { state.received } else { 0 },
        total: if state.running {
            state.total
        } else {
            model::MODEL_BYTES
        },
        size_bytes: model::MODEL_BYTES,
    }
}

pub fn status() -> Status {
    let state = lock();
    let report = engine::load_report();
    Status {
        model: card(&state),
        loaded_label: loaded_label(report.as_ref()),
        load_error: engine::load_error(),
        error: state.error.clone(),
        license: model::LICENSE.to_string(),
    }
}

fn loaded_label(report: Option<&LoadReport>) -> String {
    let Some(report) = report else {
        return "Not loaded yet".to_string();
    };
    let time = if report.load_ms >= 1000 {
        format!("{:.1} s", report.load_ms as f64 / 1000.0)
    } else {
        format!("{} ms", report.load_ms)
    };
    format!("Loaded in {time}")
}

pub fn emit(app: &AppHandle) {
    if let Err(err) = app.emit("voice-download", status()) {
        log::error!("couldn't send voice-download: {err}");
    }
}

/// Starts a download and returns. A download already running is left alone.
pub fn start(app: AppHandle) -> Result<(), String> {
    let dir = model_path()?;
    let cancel = {
        let mut state = lock();
        if state.running {
            return Ok(());
        }
        state.running = true;
        state.error = None;
        state.received = 0;
        state.total = model::download_total(&dir);
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        state.cancel = Some(std::sync::Arc::clone(&cancel));
        cancel
    };
    emit(&app);
    // Blocking HTTP client: it panics on a thread that already owns a Tokio runtime.
    tauri::async_runtime::spawn_blocking(move || run(app, cancel));
    Ok(())
}

pub fn cancel() {
    if let Some(cancel) = &lock().cancel {
        cancel.store(true, Ordering::Relaxed);
    }
}

/// Asks the download to stop and waits for it to leave the model directory.
pub fn cancel_and_wait() {
    let waiter = {
        let mut state = lock();
        if !state.running {
            return;
        }
        if let Some(cancel) = &state.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
        let (tx, rx) = mpsc::channel();
        state.finished.push(tx);
        rx
    };
    let _ = waiter.recv_timeout(Duration::from_secs(5));
}

fn run(app: AppHandle, cancel: std::sync::Arc<AtomicBool>) {
    let dir = match model_path() {
        Ok(dir) => dir,
        Err(err) => {
            finish(&app, Err(err), false);
            return;
        }
    };
    log::info!("voice model download started");
    let mut last_emit = Instant::now();
    let app_progress = app.clone();
    let result = model::download(
        &dir,
        |received, total| {
            {
                let mut state = lock();
                state.received = received;
                state.total = total;
            }
            if last_emit.elapsed() >= TICK {
                last_emit = Instant::now();
                emit(&app_progress);
            }
        },
        &cancel,
    );
    let cancelled = cancel.load(Ordering::Relaxed)
        || result
            .as_ref()
            .is_err_and(|err| err == "download cancelled");
    match &result {
        Ok(()) => log::info!("voice model download finished"),
        Err(_) if cancelled => log::info!("voice model download cancelled"),
        Err(err) => log::error!("voice model download failed: {err}"),
    }
    finish(&app, result, cancelled);
    if model_installed() {
        reregister(&app);
    }
}

fn finish(app: &AppHandle, result: Result<(), String>, cancelled: bool) {
    let waiters = {
        let mut state = lock();
        state.running = false;
        state.error = match &result {
            Ok(()) => None,
            Err(_) if cancelled => None,
            Err(err) => Some(err.clone()),
        };
        if result.is_ok() {
            let total = model_path()
                .map(|dir| model::download_total(&dir))
                .unwrap_or(model::MODEL_BYTES);
            state.received = total;
            state.total = total;
        }
        std::mem::take(&mut state.finished)
    };
    emit(app);
    for waiter in waiters {
        let _ = waiter.send(());
    }
}
