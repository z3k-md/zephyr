//! The speech engine: a `parakeet-rs` Parakeet TDT session.
//!
//! ONNX Runtime is loaded the first time [`Engine::load`] runs, from the library
//! [`model::download`] placed beside the model. One ONNX Runtime environment is created
//! for the process; a second one fails, so later loads reuse the first. The encoder
//! runs on the CPU execution provider.

use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use parakeet_rs::ort::environment::Environment;
use parakeet_rs::{ExecutionConfig, ExecutionProvider, ParakeetTDT, TimestampMode, Transcriber};

use crate::voice::model;

/// A loaded Parakeet TDT model. `Send`, and meant to be used from one worker thread.
pub struct Engine {
    model: ParakeetTDT,
    token: u64,
}

/// What the process is actually running, for Settings. Cleared when the [`Engine`] drops.
pub(crate) struct LoadReport {
    pub load_ms: u128,
}

struct Record {
    token: u64,
    load_ms: u128,
}

fn record() -> &'static Mutex<Option<Record>> {
    static RECORD: Mutex<Option<Record>> = Mutex::new(None);
    &RECORD
}

fn load_error_slot() -> &'static Mutex<Option<String>> {
    static LOAD_ERROR: Mutex<Option<String>> = Mutex::new(None);
    &LOAD_ERROR
}

fn next_token() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

pub(crate) fn load_report() -> Option<LoadReport> {
    record()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .as_ref()
        .map(|record| LoadReport {
            load_ms: record.load_ms,
        })
}

pub(crate) fn load_error() -> Option<String> {
    load_error_slot()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone()
}

fn remember_error(err: &str) {
    *load_error_slot()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = Some(err.to_string());
}

impl Engine {
    /// Loads the model in `model_dir` (see [`model::model_dir`]).
    ///
    /// Fails if the model files or the speech runtime are missing. A failure to load
    /// the runtime is `Err("Couldn't load the speech runtime: ...")`.
    pub fn load(model_dir: &Path) -> Result<Engine, String> {
        if !model::installed(model_dir) {
            let err = format!("speech model is not installed in {}", model_dir.display());
            remember_error(&err);
            return Err(err);
        }
        let started = Instant::now();
        let env = match environment(model_dir) {
            Ok(env) => env,
            Err(err) => {
                remember_error(&err);
                return Err(err);
            }
        };
        let model = match load_cpu(model_dir, &env) {
            Ok(model) => model,
            Err(err) => {
                remember_error(&err);
                return Err(err);
            }
        };
        let load_ms = started.elapsed().as_millis();
        let token = next_token();
        *record().lock().unwrap_or_else(|err| err.into_inner()) = Some(Record { token, load_ms });
        *load_error_slot()
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = None;
        log::info!("voice model loaded in {load_ms} ms");
        Ok(Engine { model, token })
    }

    /// Transcribes 16 kHz mono PCM in `-1.0..1.0`. The returned text is trimmed.
    /// An empty slice is an error; silence still goes through the model.
    pub fn transcribe(&mut self, samples_16k_mono: &[f32]) -> Result<String, String> {
        if samples_16k_mono.is_empty() {
            return Err("no audio to transcribe".to_string());
        }
        let started = Instant::now();
        let samples = samples_16k_mono.len();
        let result = self
            .model
            .transcribe_samples(
                samples_16k_mono.to_vec(),
                16_000,
                1,
                Some(TimestampMode::Tokens),
            )
            .map_err(|err| err.to_string())?;
        log::debug!(
            "voice transcribed {samples} samples in {} ms",
            started.elapsed().as_millis()
        );
        Ok(result.text.trim().to_string())
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let mut slot = record().lock().unwrap_or_else(|err| err.into_inner());
        if slot
            .as_ref()
            .is_some_and(|record| record.token == self.token)
        {
            *slot = None;
        }
    }
}

fn load_cpu(model_dir: &Path, env: &Environment) -> Result<ParakeetTDT, String> {
    let config = session_config(env);
    ParakeetTDT::from_pretrained(model_dir, Some(config)).map_err(|err| err.to_string())
}

fn session_config(env: &Environment) -> ExecutionConfig {
    ExecutionConfig::new()
        .with_environment(env.clone())
        .with_execution_provider(ExecutionProvider::Cpu)
        .with_intra_threads(threads())
}

fn threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

/// One ONNX Runtime environment for the process. Creating a second one fails, so the
/// library is loaded from `model_dir` on the first call and reused after that.
fn environment(model_dir: &Path) -> Result<Environment, String> {
    static SLOT: Mutex<Option<Environment>> = Mutex::new(None);
    let mut slot = SLOT.lock().unwrap_or_else(|err| err.into_inner());
    if let Some(env) = slot.as_ref() {
        return Ok(env.clone());
    }
    let library = model::resolve_runtime_library(model_dir)
        .unwrap_or_else(|| model::runtime_library(model_dir));
    if !library.is_file() {
        return Err(format!(
            "Couldn't load the speech runtime: {} is missing",
            library.display()
        ));
    }
    let env = parakeet_rs::ort::init_from(&library)
        .map_err(|err| format!("Couldn't load the speech runtime: {err}"))?
        .with_name("zephyr")
        .with_telemetry(false)
        .build()
        .map_err(|err| format!("Couldn't load the speech runtime: {err}"))?;
    *slot = Some(env.clone());
    Ok(env)
}

#[cfg(test)]
mod tests {
    #[test]
    fn engine_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<super::Engine>();
    }
}
