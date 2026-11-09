//! On-device speech to text.
//!
//! Audio samples and transcript text stay in the process. The only network use is
//! [`model::download`], which fetches the model files and the speech runtime. Logs record
//! durations, sample counts and timings, never samples or text.
//!
//! [`Engine`] is [`Send`] so it can live on a worker thread. Load it on that thread and call
//! [`Engine::transcribe`] from the same thread. [`model::download`] is synchronous and uses a
//! blocking HTTP client, so it also belongs on a worker (`spawn_blocking`), not on Tauri's
//! async runtime.

pub mod model;

mod capture;
pub(crate) mod download;
mod engine;
pub(crate) mod session;
pub mod vad;

pub use engine::Engine;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tauri::Manager;

static MODELS_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Remembers where the speech model lives and shares an already-installed CPU runtime.
/// Does not open the microphone or load the model.
pub fn init(app_local_data_dir: impl AsRef<Path>) {
    let root = model::models_root(app_local_data_dir);
    model::share_runtime(&root);
    let _ = MODELS_ROOT.set(root);
}

pub(crate) fn models_root() -> Result<PathBuf, String> {
    MODELS_ROOT
        .get()
        .cloned()
        .ok_or_else(|| "Voice isn't available.".to_string())
}

/// Directory of the speech model.
pub(crate) fn model_path() -> Result<PathBuf, String> {
    Ok(models_root()?.join(model::MODEL_ID))
}

/// Whether the weights and a CPU runtime are on disk. False until [`init`] has run.
pub fn model_installed() -> bool {
    model_path().is_ok_and(|dir| model::installed(&dir))
}

/// Registers shortcuts again so the voice hotkey follows the model and the setting.
pub(crate) fn reregister(app: &tauri::AppHandle) {
    let Ok(snapshot) = app.state::<crate::state::AppState>().snapshot() else {
        return;
    };
    if let Err(err) =
        crate::shortcut::register(app, &snapshot.summon_shortcut, snapshot.extra_shortcuts())
    {
        log::error!("{err}");
    }
}
