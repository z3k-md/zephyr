//! How long startup takes, logged at each step that matters for the summon shortcut: when
//! Tauri hands over (setup), when the shortcut works, and when the bar's page has loaded.

use std::sync::OnceLock;
use std::time::Instant;

static STARTED: OnceLock<Instant> = OnceLock::new();

/// Called first thing in `main`.
pub fn begin() {
    let _ = STARTED.set(Instant::now());
}

/// Logs `step` with the time since the process started.
pub fn mark(step: &str) {
    let elapsed = STARTED
        .get()
        .map(|at| at.elapsed().as_millis())
        .unwrap_or(0);
    log::info!("startup: {step} after {elapsed} ms");
}
