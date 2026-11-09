//! Hold-to-talk. A control thread owns the microphone and the timers. A second thread owns
//! the [`Engine`]: it loads the model and is the only caller of `transcribe`, which is what
//! the engine asks for. Recording starts on key down, while that load is still running, so the
//! start of the sentence is not dropped.
//!
//! Partials and the final pass see the speech gate's trimmed buffer, not the raw hold.
//! A partial runs only when that buffer gains speech; trailing silence does not start one.
//! If the final buffer is the one a partial already transcribed, that text is reused.
//!
//! The microphone is opened only from a press and dropped on release, cancel, or the 30 s cap.
//! Audio and transcript text are never logged.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use super::capture::Capture;
use super::model;
use super::vad::{self, SpeechCursor, SpeechGate};
use super::{model_installed, model_path};
use crate::state::AppState;
use crate::voice::Engine;
use crate::{shortcut, window};

pub const SAMPLE_RATE: u32 = 16_000;
const PARTIAL_EVERY: Duration = Duration::from_millis(250);
const MIN_PARTIAL: Duration = Duration::from_millis(500);
const TAP_MAX: Duration = Duration::from_millis(300);
const HOLD_CAP: Duration = Duration::from_secs(30);
const IDLE_UNLOAD: Duration = Duration::from_secs(15 * 60);
const LEVEL_EVERY: Duration = Duration::from_millis(50);

static SESSION: OnceLock<Handle> = OnceLock::new();

pub fn press(app: &AppHandle) {
    let _ = handle(app).tx.send(Cmd::Press);
}

/// Closes the microphone on this thread, then asks the worker to finish the hold.
pub fn release() {
    let Some(session) = SESSION.get() else {
        return;
    };
    let samples = session.live.request_stop();
    let _ = session.tx.send(Cmd::Release { samples });
}

pub fn cancel() {
    let Some(session) = SESSION.get() else {
        return;
    };
    // Drop the stream before the worker runs, so the mic indicator goes out immediately.
    let _ = session.live.request_stop();
    let _ = session.tx.send(Cmd::Cancel);
}

/// Drops the loaded model and waits until it has. The microphone is closed first if a hold
/// is in progress. A session that was never started has nothing to unload.
pub fn unload() {
    let Some(session) = SESSION.get() else {
        return;
    };
    let (done, wait) = mpsc::channel();
    if session.tx.send(Cmd::Unload { done }).is_err() {
        return;
    }
    let _ = wait.recv_timeout(Duration::from_secs(30));
}

pub fn samples_for(duration: Duration) -> usize {
    (u128::from(SAMPLE_RATE) * duration.as_millis() / 1_000) as usize
}

pub fn is_tap(samples: usize) -> bool {
    samples < samples_for(TAP_MAX)
}

/// A partial pass runs only when the model is up, nothing else is running, `samples` is at
/// least half a second of trimmed model input, and the previous pass started at least 250 ms ago.
pub fn should_start_partial(
    model_ready: bool,
    pass_running: bool,
    samples: usize,
    since_last: Duration,
) -> bool {
    model_ready
        && !pass_running
        && samples >= samples_for(MIN_PARTIAL)
        && since_last >= PARTIAL_EVERY
}

pub fn partial_changed(previous: &str, next: &str) -> bool {
    let next = next.trim();
    next != previous.trim()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalOutcome {
    pub text: String,
    pub cancelled: bool,
}

/// Under ~0.3 s, or a blank transcript, is a tap: empty text, not cancelled. A cancel
/// discards whatever was said.
pub fn final_outcome(samples: usize, transcript: &str, cancelled: bool) -> FinalOutcome {
    if cancelled {
        return FinalOutcome {
            text: String::new(),
            cancelled: true,
        };
    }
    let text = transcript.trim();
    if is_tap(samples) || text.is_empty() {
        FinalOutcome {
            text: String::new(),
            cancelled: false,
        }
    } else {
        FinalOutcome {
            text: text.to_string(),
            cancelled: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Listening,
    Finishing,
}

enum Gate {
    Go,
    Ignore,
    NoModel,
}

enum Store {
    Kept,
    /// Release won the race and this capture was never published. The caller still has it.
    Closed(Capture),
    /// Release already took the buffer out of the slot.
    Taken,
}

struct Live {
    stop: AtomicBool,
    capture: Mutex<Option<Capture>>,
}

impl Live {
    fn arm(&self) {
        self.stop.store(false, Ordering::SeqCst);
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Sets the stop flag and takes the stream, so the device closes on the calling thread.
    fn request_stop(&self) -> Option<Vec<f32>> {
        self.stop.store(true, Ordering::SeqCst);
        self.capture
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .take()
            .map(Capture::into_samples)
    }

    fn store(&self, capture: Capture) -> Store {
        if self.stopped() {
            return Store::Closed(capture);
        }
        {
            let mut slot = self.capture.lock().unwrap_or_else(|err| err.into_inner());
            if self.stopped() {
                return Store::Closed(capture);
            }
            *slot = Some(capture);
        }
        if self.stopped() {
            return match self
                .capture
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .take()
            {
                Some(capture) => Store::Closed(capture),
                None => Store::Taken,
            };
        }
        Store::Kept
    }

    fn len(&self) -> usize {
        self.capture
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .as_ref()
            .map(Capture::len)
            .unwrap_or(0)
    }

    fn snapshot(&self) -> Vec<f32> {
        self.capture
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .as_ref()
            .map(Capture::snapshot)
            .unwrap_or_default()
    }

    fn with_samples<R>(&self, body: impl FnOnce(&[f32]) -> R) -> R {
        let slot = self.capture.lock().unwrap_or_else(|err| err.into_inner());
        match slot.as_ref() {
            Some(capture) => capture.with_samples(body),
            None => body(&[]),
        }
    }

    fn level(&self) -> Option<f32> {
        self.capture
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .as_ref()
            .map(Capture::level)
    }

    fn take_error(&self) -> Option<String> {
        self.capture
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .as_ref()
            .and_then(Capture::take_error)
    }
}

struct Handle {
    tx: Sender<Cmd>,
    live: Arc<Live>,
}

enum Cmd {
    Press,
    Release {
        samples: Option<Vec<f32>>,
    },
    Cancel,
    Unload {
        done: Sender<()>,
    },
    Loaded {
        generation: u64,
        result: Result<(), String>,
    },
    Transcript {
        id: u64,
        result: Result<String, String>,
    },
    Unloaded,
}

enum EngineCmd {
    Load { generation: u64, dir: PathBuf },
    Transcribe { id: u64, samples: Vec<f32> },
    Unload,
}

struct Pending {
    samples: Vec<f32>,
    cancelled: bool,
    /// Trimmed audio for this hold, filled once the mic has closed.
    gated: Option<GatedAudio>,
}

struct GatedAudio {
    input: Vec<f32>,
    fingerprint: u64,
}

struct CompletedPartial {
    len: usize,
    fingerprint: u64,
    text: String,
}

struct InflightPass {
    len: usize,
    fingerprint: u64,
    final_pass: bool,
}

struct Worker {
    app: AppHandle,
    rx: Receiver<Cmd>,
    engine_tx: Sender<EngineCmd>,
    live: Arc<Live>,
    phase: Phase,
    model_ready: bool,
    loading: bool,
    load_gen: u64,
    pass_id: u64,
    pass_running: bool,
    final_pass: bool,
    last_partial: String,
    last_partial_at: Instant,
    last_level_at: Instant,
    hold_started: Instant,
    final_started: Instant,
    last_used: Option<Instant>,
    pending: Option<Pending>,
    unload_waiter: Option<Sender<()>>,
    gate: SpeechGate,
    /// Speech committed to the partial most recently handed to the model.
    submitted_cursor: SpeechCursor,
    completed: Option<CompletedPartial>,
    inflight: Option<InflightPass>,
    /// The partial in flight is already the final buffer. Wait for it.
    reuse_inflight: bool,
    passes_run: u32,
    passes_skipped: u32,
    final_reused: bool,
}

fn handle(app: &AppHandle) -> &'static Handle {
    SESSION.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        let (engine_tx, engine_rx) = mpsc::channel();
        let to_worker = tx.clone();
        std::thread::Builder::new()
            .name("voice-model".into())
            .spawn(move || engine_thread(engine_rx, to_worker))
            .expect("voice model thread");
        let live = Arc::new(Live {
            stop: AtomicBool::new(false),
            capture: Mutex::new(None),
        });
        let worker_live = Arc::clone(&live);
        let worker_tx = engine_tx;
        let worker_app = app.clone();
        std::thread::Builder::new()
            .name("voice".into())
            .spawn(move || {
                Worker {
                    app: worker_app,
                    rx,
                    engine_tx: worker_tx,
                    live: worker_live,
                    phase: Phase::Idle,
                    model_ready: false,
                    loading: false,
                    load_gen: 0,
                    pass_id: 0,
                    pass_running: false,
                    final_pass: false,
                    last_partial: String::new(),
                    last_partial_at: Instant::now(),
                    last_level_at: Instant::now(),
                    hold_started: Instant::now(),
                    final_started: Instant::now(),
                    last_used: None,
                    pending: None,
                    unload_waiter: None,
                    gate: SpeechGate::new(),
                    submitted_cursor: SpeechCursor::default(),
                    completed: None,
                    inflight: None,
                    reuse_inflight: false,
                    passes_run: 0,
                    passes_skipped: 0,
                    final_reused: false,
                }
                .run();
            })
            .expect("voice thread");
        Handle { tx, live }
    })
}

fn engine_thread(rx: Receiver<EngineCmd>, tx: Sender<Cmd>) {
    let mut engine: Option<Engine> = None;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            EngineCmd::Load { generation, dir } => {
                let result = if engine.is_some() {
                    Ok(())
                } else {
                    match Engine::load(&dir) {
                        Ok(loaded) => {
                            engine = Some(loaded);
                            Ok(())
                        }
                        Err(err) => Err(err),
                    }
                };
                let _ = tx.send(Cmd::Loaded { generation, result });
            }
            EngineCmd::Transcribe { id, samples } => {
                let result = match engine.as_mut() {
                    Some(engine) => engine.transcribe(&samples),
                    None => Err("speech model is not loaded".to_string()),
                };
                // `samples` drops here. The text is not logged.
                let _ = tx.send(Cmd::Transcript { id, result });
            }
            EngineCmd::Unload => {
                engine = None;
                let _ = tx.send(Cmd::Unloaded);
            }
        }
    }
}

impl Worker {
    fn run(mut self) {
        loop {
            let incoming = if self.phase == Phase::Idle && !self.model_ready && !self.loading {
                match self.rx.recv() {
                    Ok(cmd) => Some(cmd),
                    Err(_) => break,
                }
            } else {
                let wait = if self.phase == Phase::Idle {
                    Duration::from_secs(30)
                } else {
                    Duration::from_millis(50)
                };
                match self.rx.recv_timeout(wait) {
                    Ok(cmd) => Some(cmd),
                    Err(RecvTimeoutError::Timeout) => {
                        self.tick();
                        None
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            };
            if let Some(cmd) = incoming {
                self.handle(cmd);
            }
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Press => self.on_press(),
            Cmd::Release { samples } => self.on_release(samples),
            Cmd::Cancel => self.on_cancel(),
            Cmd::Unload { done } => {
                if self.phase != Phase::Idle {
                    self.on_cancel();
                }
                self.request_unload(Some(done));
            }
            Cmd::Loaded { generation, result } => self.on_loaded(generation, result),
            Cmd::Transcript { id, result } => self.on_transcript(id, result),
            Cmd::Unloaded => {
                if let Some(done) = self.unload_waiter.take() {
                    let _ = done.send(());
                }
            }
        }
    }

    fn tick(&mut self) {
        match self.phase {
            Phase::Listening => self.tick_listen(),
            Phase::Finishing => self.pump_final(),
            Phase::Idle => self.maybe_unload(),
        }
    }

    fn on_press(&mut self) {
        // Windows delivers the hotkey once, but a held key can still be reported again.
        if self.phase != Phase::Idle || shortcut::recording() {
            return;
        }
        self.hold_started = Instant::now();
        match self.gate() {
            Gate::Ignore => return,
            Gate::NoModel => {
                window::show_bar(&self.app);
                self.fail_hold("The speech model isn't installed.".to_string());
                return;
            }
            Gate::Go => {}
        }
        self.live.arm();
        self.phase = Phase::Listening;
        self.last_partial_at = self.hold_started;
        self.last_level_at = self
            .hold_started
            .checked_sub(LEVEL_EVERY)
            .unwrap_or(self.hold_started);
        self.last_partial.clear();
        self.pass_running = false;
        self.final_pass = false;
        self.gate = SpeechGate::new();
        self.submitted_cursor = SpeechCursor::default();
        self.completed = None;
        self.inflight = None;
        self.reuse_inflight = false;
        self.passes_run = 0;
        self.passes_skipped = 0;
        self.final_reused = false;
        window::show_bar(&self.app);
        if self.live.stopped() {
            self.begin_finish(Vec::new(), false);
            return;
        }
        if !self.model_ready && !self.loading {
            self.start_load();
            if self.phase != Phase::Listening {
                return;
            }
        }
        // After the bar's own show event, so a remembered view is replaced by search. A
        // second listening event during the hold only says the model finished loading.
        self.emit_state("listening");
        log::info!(
            "voice listening, model {}",
            if self.model_ready { "ready" } else { "loading" }
        );
        let capture = match Capture::open() {
            Ok(capture) => capture,
            Err(message) => {
                self.fail_hold(message);
                return;
            }
        };
        match self.live.store(capture) {
            Store::Kept => {}
            Store::Closed(capture) => self.begin_finish(capture.into_samples(), false),
            Store::Taken => {}
        }
    }

    fn on_release(&mut self, samples: Option<Vec<f32>>) {
        if self.phase != Phase::Listening {
            return;
        }
        let samples = samples.unwrap_or_else(|| self.live.request_stop().unwrap_or_default());
        log::info!("voice key released, {} samples", samples.len());
        self.begin_finish(samples, false);
    }

    fn on_cancel(&mut self) {
        if self.phase == Phase::Idle {
            return;
        }
        let _ = self.live.request_stop();
        self.pass_id = self.pass_id.wrapping_add(1);
        self.pass_running = false;
        self.final_pass = false;
        self.inflight = None;
        self.reuse_inflight = false;
        self.pending = None;
        log::info!(
            "voice hold cancelled after {} ms",
            self.hold_started.elapsed().as_millis()
        );
        self.emit_state("finishing");
        self.phase = Phase::Idle;
        self.last_used = Some(Instant::now());
        self.emit_final("", true);
        self.emit_state("idle");
    }

    fn tick_listen(&mut self) {
        if let Some(err) = self.live.take_error() {
            self.fail_hold(err);
            return;
        }
        self.observe_live();
        if self.last_level_at.elapsed() >= LEVEL_EVERY {
            self.last_level_at = Instant::now();
            if let Some(level) = self.live.level() {
                self.emit_level(level);
            }
        }
        if self.hold_started.elapsed() >= HOLD_CAP {
            log::info!("voice hold capped at 30 s");
            let samples = self.live.request_stop().unwrap_or_default();
            self.begin_finish(samples, false);
            return;
        }
        self.maybe_partial();
    }

    fn observe_live(&mut self) {
        let live = Arc::clone(&self.live);
        live.with_samples(|audio| self.gate.observe(audio));
    }

    fn maybe_partial(&mut self) {
        let input_len = self.gate.model_input_len(self.live.len());
        if !should_start_partial(
            self.model_ready,
            self.pass_running,
            input_len,
            self.last_partial_at.elapsed(),
        ) {
            return;
        }
        // Trailing silence only moves the cursor once, when the segment closes with
        // its pad full. That pass sees what the final would, so release can reuse it.
        if self.gate.cursor() == self.submitted_cursor {
            self.passes_skipped = self.passes_skipped.saturating_add(1);
            self.last_partial_at = Instant::now();
            return;
        }
        let audio = self.live.snapshot();
        self.gate.observe(&audio);
        let input = self.gate.model_input(&audio);
        if input.len() < samples_for(MIN_PARTIAL) {
            return;
        }
        if self.gate.cursor() == self.submitted_cursor {
            self.passes_skipped = self.passes_skipped.saturating_add(1);
            self.last_partial_at = Instant::now();
            return;
        }
        self.submitted_cursor = self.gate.cursor();
        self.last_partial_at = Instant::now();
        self.start_transcribe(input, false);
    }

    fn begin_finish(&mut self, mut samples: Vec<f32>, cancelled: bool) {
        if self.phase == Phase::Idle {
            return;
        }
        if let Some(taken) = self.live.request_stop()
            && samples.is_empty()
        {
            samples = taken;
        }
        if !cancelled {
            self.gate.observe(&samples);
        }
        self.final_reused = false;
        self.phase = Phase::Finishing;
        self.final_started = Instant::now();
        self.emit_state("finishing");
        self.pending = Some(Pending {
            samples,
            cancelled,
            gated: None,
        });
        self.pump_final();
    }

    fn pump_final(&mut self) {
        let Some((cancelled, count)) = self
            .pending
            .as_ref()
            .map(|pending| (pending.cancelled, pending.samples.len()))
        else {
            return;
        };
        if cancelled {
            self.finish(final_outcome(0, "", true));
            return;
        }
        if is_tap(count) {
            log::info!("voice tap, {count} samples");
            self.finish(final_outcome(count, "", false));
            return;
        }
        self.ensure_gated();
        let Some((empty, fingerprint, len)) = self.pending.as_ref().and_then(|pending| {
            pending
                .gated
                .as_ref()
                .map(|gated| (gated.input.is_empty(), gated.fingerprint, gated.input.len()))
        }) else {
            return;
        };
        if empty {
            // No speech at all: same result as a tap, and the model is not called.
            log::info!("voice final skipped, no speech, {count} samples");
            self.finish(final_outcome(count, "", false));
            return;
        }
        if !self.model_ready {
            if !self.loading {
                self.start_load();
            }
            return;
        }
        // A pass already running has to finish before a new one starts. If it is
        // already this exact buffer, its text becomes the final.
        if self.pass_running {
            if self.inflight.as_ref().is_some_and(|pass| {
                !pass.final_pass && pass.fingerprint == fingerprint && pass.len == len
            }) {
                self.reuse_inflight = true;
            }
            return;
        }
        if let Some(text) = self.completed.as_ref().and_then(|done| {
            (done.fingerprint == fingerprint && done.len == len).then(|| done.text.clone())
        }) {
            self.final_reused = true;
            log::info!(
                "voice final reused partial, {} ms, {count} samples",
                self.final_started.elapsed().as_millis()
            );
            self.finish(final_outcome(count, &text, false));
            return;
        }
        let input = self
            .pending
            .as_ref()
            .and_then(|pending| pending.gated.as_ref().map(|gated| gated.input.clone()))
            .unwrap_or_default();
        self.start_transcribe(input, true);
    }

    fn ensure_gated(&mut self) {
        let needs = self.pending.as_ref().is_some_and(|pending| {
            pending.gated.is_none() && !pending.cancelled && !pending.samples.is_empty()
        });
        if !needs {
            return;
        }
        let samples = self
            .pending
            .as_mut()
            .map(|pending| std::mem::take(&mut pending.samples))
            .unwrap_or_default();
        self.gate.observe(&samples);
        let input = self.gate.model_input(&samples);
        let fingerprint = vad::fingerprint(&input);
        if let Some(pending) = self.pending.as_mut() {
            pending.samples = samples;
            pending.gated = Some(GatedAudio { input, fingerprint });
        }
    }

    fn start_transcribe(&mut self, samples: Vec<f32>, final_pass: bool) {
        self.pass_id = self.pass_id.wrapping_add(1);
        self.pass_running = true;
        self.final_pass = final_pass;
        let id = self.pass_id;
        let count = samples.len();
        self.inflight = Some(InflightPass {
            len: count,
            fingerprint: vad::fingerprint(&samples),
            final_pass,
        });
        log::debug!(
            "voice queued {} pass, {count} samples",
            if final_pass { "final" } else { "partial" }
        );
        if self
            .engine_tx
            .send(EngineCmd::Transcribe { id, samples })
            .is_err()
        {
            self.pass_running = false;
            self.inflight = None;
            self.fail_hold("Speech model stopped.".to_string());
            return;
        }
        self.passes_run = self.passes_run.saturating_add(1);
    }

    fn on_transcript(&mut self, id: u64, result: Result<String, String>) {
        if id != self.pass_id {
            return;
        }
        let final_pass = self.final_pass;
        let reuse = self.reuse_inflight;
        self.pass_running = false;
        self.final_pass = false;
        self.reuse_inflight = false;
        match result {
            Ok(text) => {
                let trimmed = text.trim().to_string();
                if let Some(pass) = self.inflight.take()
                    && !pass.final_pass
                {
                    self.completed = Some(CompletedPartial {
                        len: pass.len,
                        fingerprint: pass.fingerprint,
                        text: trimmed.clone(),
                    });
                }
                let count = self
                    .pending
                    .as_ref()
                    .map(|pending| pending.samples.len())
                    .unwrap_or(0);
                if final_pass {
                    log::info!(
                        "voice final pass {} ms, {count} samples",
                        self.final_started.elapsed().as_millis()
                    );
                    self.finish(final_outcome(count, &trimmed, false));
                } else if reuse {
                    self.final_reused = true;
                    log::info!(
                        "voice final reused partial, {} ms, {count} samples",
                        self.final_started.elapsed().as_millis()
                    );
                    self.finish(final_outcome(count, &trimmed, false));
                } else if self.phase == Phase::Listening
                    && partial_changed(&self.last_partial, &trimmed)
                {
                    self.last_partial = trimmed;
                    self.emit_partial(&self.last_partial);
                } else if self.phase == Phase::Finishing {
                    self.pump_final();
                }
            }
            Err(err) => {
                self.inflight = None;
                log::error!("voice transcribe failed: {err}");
                if final_pass || reuse || self.phase == Phase::Finishing {
                    self.fail_hold(err);
                }
            }
        }
    }

    fn on_loaded(&mut self, generation: u64, result: Result<(), String>) {
        if generation != self.load_gen {
            return;
        }
        self.loading = false;
        match result {
            Ok(()) => {
                self.model_ready = true;
                log::info!("voice model ready");
                if self.phase == Phase::Listening {
                    self.emit_state("listening");
                }
                self.pump_final();
            }
            Err(err) => {
                log::error!("voice model failed to load: {err}");
                self.model_ready = false;
                if self.phase != Phase::Idle {
                    self.fail_hold(err);
                }
            }
        }
    }

    fn finish(&mut self, outcome: FinalOutcome) {
        // A pass still inside the model belongs to this hold. Forget it so the
        // transcript cannot be applied after the microphone has already closed.
        if self.pass_running {
            self.pass_id = self.pass_id.wrapping_add(1);
            self.pass_running = false;
            self.final_pass = false;
            self.inflight = None;
            self.reuse_inflight = false;
        }
        let count = self
            .pending
            .as_ref()
            .map(|pending| pending.samples.len())
            .unwrap_or(0);
        if !outcome.cancelled {
            let gated_len = self
                .pending
                .as_ref()
                .and_then(|pending| pending.gated.as_ref().map(|gated| gated.input.len()));
            let model_samples = gated_len.unwrap_or_else(|| self.gate.model_input_len(count));
            log::info!(
                "voice gate: raw {count} samples, model input {model_samples} samples, {} speech segments, passes run {} skipped {}, final reused {}",
                self.gate.segment_count(),
                self.passes_run,
                self.passes_skipped,
                self.final_reused
            );
        }
        log::info!(
            "voice hold ended after {} ms, {count} samples, cancelled {}",
            self.hold_started.elapsed().as_millis(),
            outcome.cancelled
        );
        self.pending = None;
        self.pass_running = false;
        self.final_pass = false;
        self.phase = Phase::Idle;
        self.last_used = Some(Instant::now());
        self.emit_final(&outcome.text, outcome.cancelled);
        self.emit_state("idle");
    }

    fn fail_hold(&mut self, message: String) {
        let _ = self.live.request_stop();
        self.pass_id = self.pass_id.wrapping_add(1);
        self.pass_running = false;
        self.final_pass = false;
        self.inflight = None;
        self.reuse_inflight = false;
        self.pending = None;
        log::error!("voice hold failed: {message}");
        self.emit_error(&message);
        log::info!(
            "voice hold ended after {} ms, cancelled true",
            self.hold_started.elapsed().as_millis()
        );
        self.phase = Phase::Idle;
        self.last_used = Some(Instant::now());
        self.emit_final("", true);
        self.emit_state("idle");
    }

    fn start_load(&mut self) {
        let dir = match model_path() {
            Ok(dir) if model::installed(&dir) => dir,
            Ok(_) => {
                self.fail_hold("The speech model isn't installed.".to_string());
                return;
            }
            Err(err) => {
                self.fail_hold(err);
                return;
            }
        };
        self.load_gen = self.load_gen.wrapping_add(1);
        self.loading = true;
        let generation = self.load_gen;
        if self
            .engine_tx
            .send(EngineCmd::Load { generation, dir })
            .is_err()
        {
            self.loading = false;
            self.fail_hold("Speech model stopped.".to_string());
        }
    }

    fn maybe_unload(&mut self) {
        let Some(last) = self.last_used else {
            return;
        };
        if self.model_ready && last.elapsed() >= IDLE_UNLOAD {
            log::info!(
                "voice model unloaded after {} minutes idle",
                last.elapsed().as_secs() / 60
            );
            self.request_unload(None);
        }
    }

    fn request_unload(&mut self, waiter: Option<Sender<()>>) {
        self.load_gen = self.load_gen.wrapping_add(1);
        self.loading = false;
        self.model_ready = false;
        if self.engine_tx.send(EngineCmd::Unload).is_err() {
            if let Some(waiter) = waiter {
                let _ = waiter.send(());
            }
            return;
        }
        if let Some(waiter) = waiter {
            self.unload_waiter = Some(waiter);
        }
    }

    fn gate(&self) -> Gate {
        let Ok(snapshot) = self.app.state::<AppState>().snapshot() else {
            return Gate::Ignore;
        };
        if !snapshot.voice.enabled {
            return Gate::Ignore;
        }
        if model_installed() {
            Gate::Go
        } else {
            Gate::NoModel
        }
    }

    fn emit_state(&self, state: &'static str) {
        emit(
            &self.app,
            "voice-state",
            StateEvent {
                state,
                model_ready: self.model_ready,
            },
        );
    }

    fn emit_level(&self, level: f32) {
        emit(&self.app, "voice-level", LevelEvent { level });
    }

    fn emit_partial(&self, text: &str) {
        emit(
            &self.app,
            "voice-partial",
            TextEvent {
                text: text.to_string(),
            },
        );
    }

    fn emit_final(&self, text: &str, cancelled: bool) {
        emit(
            &self.app,
            "voice-final",
            FinalEvent {
                text: text.to_string(),
                cancelled,
            },
        );
    }

    fn emit_error(&self, message: &str) {
        emit(
            &self.app,
            "voice-error",
            ErrorEvent {
                message: message.to_string(),
            },
        );
    }
}

fn emit(app: &AppHandle, event: &str, payload: impl serde::Serialize + Clone) {
    if let Err(err) = app.emit_to("main", event, payload) {
        log::error!("couldn't send {event}: {err}");
    }
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct StateEvent {
    state: &'static str,
    model_ready: bool,
}

#[derive(Clone, serde::Serialize)]
struct LevelEvent {
    level: f32,
}

#[derive(Clone, serde::Serialize)]
struct TextEvent {
    text: String,
}

#[derive(Clone, serde::Serialize)]
struct FinalEvent {
    text: String,
    cancelled: bool,
}

#[derive(Clone, serde::Serialize)]
struct ErrorEvent {
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partials_wait_for_audio_a_free_model_and_a_quarter_second() {
        let half = samples_for(MIN_PARTIAL);
        assert!(!should_start_partial(true, false, half - 1, PARTIAL_EVERY));
        assert!(!should_start_partial(false, false, half, PARTIAL_EVERY));
        assert!(!should_start_partial(true, true, half, PARTIAL_EVERY));
        assert!(!should_start_partial(
            true,
            false,
            half,
            PARTIAL_EVERY - Duration::from_millis(1)
        ));
        assert!(should_start_partial(true, false, half, PARTIAL_EVERY));
    }

    #[test]
    fn a_tap_or_blank_transcript_is_an_empty_final_and_cancel_discards_it() {
        assert!(is_tap(samples_for(TAP_MAX) - 1));
        assert!(!is_tap(samples_for(TAP_MAX)));
        assert_eq!(
            final_outcome(100, "open mail", false),
            FinalOutcome {
                text: String::new(),
                cancelled: false,
            }
        );
        assert_eq!(
            final_outcome(samples_for(Duration::from_secs(2)), "  ", false),
            FinalOutcome {
                text: String::new(),
                cancelled: false,
            }
        );
        assert_eq!(
            final_outcome(samples_for(Duration::from_secs(2)), "  notes  ", false),
            FinalOutcome {
                text: "notes".to_string(),
                cancelled: false,
            }
        );
        assert_eq!(
            final_outcome(samples_for(Duration::from_secs(2)), "notes", true),
            FinalOutcome {
                text: String::new(),
                cancelled: true,
            }
        );
    }

    #[test]
    fn partials_emit_only_when_the_text_changes() {
        assert!(!partial_changed("", ""));
        assert!(!partial_changed("notes", " notes "));
        assert!(partial_changed("", "notes"));
        assert!(partial_changed("note", "notes"));
        assert!(partial_changed("notes", ""));
    }

    #[test]
    fn the_hold_cap_is_thirty_seconds_and_a_partial_needs_half_a_second() {
        assert_eq!(samples_for(HOLD_CAP), SAMPLE_RATE as usize * 30);
        assert_eq!(samples_for(MIN_PARTIAL), SAMPLE_RATE as usize / 2);
        assert_eq!(samples_for(TAP_MAX), (SAMPLE_RATE as usize * 3) / 10);
        assert!(HOLD_CAP.as_secs() == 30);
        assert!(IDLE_UNLOAD.as_secs() == 15 * 60);
    }
}
