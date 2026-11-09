//! Times the speech engine on short clips. Not part of the app.
//!
//! ```text
//! cargo run --release --example voice_bench -- --model-dir DIR --clips DIR [--download]
//! ```
//!
//! `--download` fetches the pinned model into `DIR` first. Clips are 16 kHz mono WAV files
//! listed in `DIR/expected.txt` as `filename<TAB>expected text`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use zephyr_lib::voice::Engine;
use zephyr_lib::voice::model;
use zephyr_lib::voice::vad::{SpeechCursor, SpeechGate};

const STEP_SAMPLES: usize = 16_000 / 4;
/// Silence appended after each clip so the gate can show skipped trailing passes.
const TRAIL_SAMPLES: usize = 16_000 * 3 / 2;
/// Half a second of trimmed audio, the session's minimum for a partial.
const MIN_PARTIAL_SAMPLES: usize = 16_000 / 2;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("voice_bench: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        eprintln!(
            "usage: voice_bench --model-dir DIR --clips DIR [--download]\n\
             --download fetches the pinned model into DIR before timing"
        );
        return Ok(());
    }
    let model_dir = PathBuf::from(flag(&args, "--model-dir")?);
    let clips_dir = PathBuf::from(flag(&args, "--clips")?);
    if args.iter().any(|arg| arg == "--download") {
        let cancel = AtomicBool::new(false);
        let shown = AtomicU64::new(0);
        model::download(
            &model_dir,
            |got, total| {
                let previous = shown.load(Ordering::Relaxed);
                if got >= previous.saturating_add(8 * 1024 * 1024) || got == total {
                    shown.store(got, Ordering::Relaxed);
                    eprintln!("model download {got} / {total} bytes");
                }
            },
            &cancel,
        )?;
    }
    if !model::installed(&model_dir) {
        return Err(format!(
            "model is not installed in {} (pass --download)",
            model_dir.display()
        ));
    }

    let clips = load_expected(&clips_dir)?;
    if clips.is_empty() {
        return Err(format!("no clips in {}", clips_dir.display()));
    }
    eprintln!(
        "model {} ({}, {}, {} bytes, rev {})",
        model::DISPLAY_NAME,
        model::LICENSE,
        model::MODEL_ID,
        model::DOWNLOAD_BYTES,
        model::REVISION
    );

    let started = Instant::now();
    let mut engine = Engine::load(&model_dir)?;
    let load_ms = started.elapsed().as_millis();
    eprintln!("load {load_ms} ms");

    let mut reports = Vec::new();
    for clip in &clips {
        reports.push(bench_clip(&mut engine, clip)?);
    }
    print_summary(load_ms, &reports);
    Ok(())
}

fn flag(args: &[String], name: &str) -> Result<String, String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
        .ok_or_else(|| format!("missing {name}"))
}

struct Clip {
    name: String,
    expected: String,
    samples: Vec<f32>,
}

fn load_expected(dir: &Path) -> Result<Vec<Clip>, String> {
    let text = std::fs::read_to_string(dir.join("expected.txt"))
        .map_err(|err| format!("couldn't read expected.txt: {err}"))?;
    let mut clips = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, expected) = line.split_once('\t').ok_or_else(|| {
            format!(
                "expected.txt line {} is not 'filename<TAB>text'",
                number + 1
            )
        })?;
        let samples = read_wav(&dir.join(name))?;
        clips.push(Clip {
            name: name.to_string(),
            expected: expected.to_string(),
            samples,
        });
    }
    Ok(clips)
}

fn read_wav(path: &Path) -> Result<Vec<f32>, String> {
    let mut reader =
        hound::WavReader::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.sample_rate != 16_000 {
        return Err(format!(
            "{} is {} Hz, {} ch; need 16000 Hz mono",
            path.display(),
            spec.sample_rate,
            spec.channels
        ));
    }
    match spec.sample_format {
        hound::SampleFormat::Int => {
            if spec.bits_per_sample == 0 || spec.bits_per_sample > 32 {
                return Err(format!(
                    "{} has {} bits per sample",
                    path.display(),
                    spec.bits_per_sample
                ));
            }
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|sample| {
                    sample
                        .map(|value| value as f32 / scale)
                        .map_err(|err| format!("{}: {err}", path.display()))
                })
                .collect()
        }
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|sample| sample.map_err(|err| format!("{}: {err}", path.display())))
            .collect(),
    }
}

struct Pass {
    audio_ms: u128,
    elapsed_ms: u128,
    changed: bool,
}

struct ClipReport {
    name: String,
    expected: String,
    text: String,
    ungated: String,
    audio_ms: u128,
    final_ms: u128,
    ungated_ms: u128,
    matched: bool,
    matches_ungated: bool,
    changes: u32,
    intervals: u32,
    pass_ms: u128,
    pass_audio_ms: u128,
    ran: u32,
    skipped: u32,
    reused: bool,
    final_eq_partial: bool,
    segments: usize,
    model_ms: u128,
}

fn bench_clip(engine: &mut Engine, clip: &Clip) -> Result<ClipReport, String> {
    let audio_ms = (clip.samples.len() as u128) * 1000 / 16_000;
    eprintln!(
        "\n{}  {:.2}s audio  expected {:?}",
        clip.name,
        audio_ms as f64 / 1000.0,
        clip.expected
    );

    let ungated_started = Instant::now();
    let ungated = engine.transcribe(&clip.samples)?;
    let ungated_ms = ungated_started.elapsed().as_millis();
    eprintln!("  ungated {ungated_ms:>6} ms  {ungated:?}");

    // The hold keeps going after the words stop. Those extra samples are silence.
    let mut audio = clip.samples.clone();
    audio.resize(audio.len() + TRAIL_SAMPLES, 0.0);

    let mut gate = SpeechGate::new();
    let mut submitted = SpeechCursor::default();
    let mut last_input: Vec<f32> = Vec::new();
    let mut last_partial = String::new();
    let mut passes = Vec::new();
    let mut previous = String::new();
    let mut ran = 0u32;
    let mut skipped = 0u32;

    let mut length = STEP_SAMPLES.min(audio.len());
    loop {
        let prefix = &audio[..length];
        gate.observe(prefix);
        let input = gate.model_input(prefix);
        let cursor = gate.cursor();
        // Same rule as the session: a partial needs half a second of trimmed audio
        // and speech the previous pass has not already seen.
        let ready = input.len() >= MIN_PARTIAL_SAMPLES;
        if ready && cursor != submitted {
            let started = Instant::now();
            let text = engine.transcribe(&input)?;
            let elapsed_ms = started.elapsed().as_millis();
            let model_ms = (input.len() as u128) * 1000 / 16_000;
            let raw_ms = (prefix.len() as u128) * 1000 / 16_000;
            let last = last_word(&text);
            let changed = !passes.is_empty() && last != previous;
            eprintln!(
                "  {raw_ms:>5} ms raw  {model_ms:>5} ms model  {elapsed_ms:>6} ms  last {last:?}{}  {text:?}",
                if changed { " changed" } else { "" }
            );
            previous = last;
            last_partial = text.clone();
            last_input = input;
            submitted = cursor;
            ran += 1;
            passes.push(Pass {
                audio_ms: model_ms,
                elapsed_ms,
                changed,
            });
        } else if prefix.len() >= MIN_PARTIAL_SAMPLES && ready {
            skipped += 1;
        }
        if length == audio.len() {
            break;
        }
        length = (length + STEP_SAMPLES).min(audio.len());
    }

    let final_input = gate.model_input(&audio);
    let (final_ms, text, reused) = if final_input.is_empty() {
        (0, String::new(), false)
    } else if !last_input.is_empty() && final_input == last_input {
        (0, last_partial.clone(), true)
    } else {
        let started = Instant::now();
        let text = engine.transcribe(&final_input)?;
        let elapsed_ms = started.elapsed().as_millis();
        ran += 1;
        passes.push(Pass {
            audio_ms: (final_input.len() as u128) * 1000 / 16_000,
            elapsed_ms,
            changed: !passes.is_empty() && last_word(&text) != previous,
        });
        (elapsed_ms, text, false)
    };
    let matched = normalize(&text) == normalize(&clip.expected);
    let matches_ungated = normalize(&text) == normalize(&ungated);
    let final_eq_partial = text.trim() == last_partial.trim();
    let changes = passes.iter().filter(|pass| pass.changed).count() as u32;
    let intervals = passes.len().saturating_sub(1) as u32;
    let segments = gate.segment_count();
    let model_ms = (final_input.len() as u128) * 1000 / 16_000;
    eprintln!(
        "  gate segments {segments}  model {model_ms} ms  run {ran}  skipped {skipped}  reused {reused}  final==partial {final_eq_partial}  matches ungated {matches_ungated}"
    );
    eprintln!("  final match={matched}  changes {changes}/{intervals}  text {text:?}");

    Ok(ClipReport {
        name: clip.name.clone(),
        expected: clip.expected.clone(),
        text,
        ungated,
        audio_ms,
        final_ms,
        ungated_ms,
        matched,
        matches_ungated,
        changes,
        intervals,
        pass_ms: passes.iter().map(|pass| pass.elapsed_ms).sum(),
        pass_audio_ms: passes.iter().map(|pass| pass.audio_ms).sum(),
        ran,
        skipped,
        reused,
        final_eq_partial,
        segments,
        model_ms,
    })
}

fn print_summary(load_ms: u128, reports: &[ClipReport]) {
    let exact = reports.iter().filter(|report| report.matched).count();
    let ungated_match = reports
        .iter()
        .filter(|report| report.matches_ungated)
        .count();
    let reused = reports.iter().filter(|report| report.reused).count();
    let eq_partial = reports
        .iter()
        .filter(|report| report.final_eq_partial)
        .count();
    let ran: u32 = reports.iter().map(|report| report.ran).sum();
    let skipped: u32 = reports.iter().map(|report| report.skipped).sum();
    let final_ms: u128 = reports.iter().map(|report| report.final_ms).sum();
    let ungated_ms: u128 = reports.iter().map(|report| report.ungated_ms).sum();
    let audio_ms: u128 = reports.iter().map(|report| report.audio_ms).sum();
    let pass_ms: u128 = reports.iter().map(|report| report.pass_ms).sum();
    let pass_audio_ms: u128 = reports.iter().map(|report| report.pass_audio_ms).sum();
    let changes: u32 = reports.iter().map(|report| report.changes).sum();
    let intervals: u32 = reports.iter().map(|report| report.intervals).sum();
    let clips_n = reports.len().max(1) as u128;
    let pass_n = (intervals as u128 + reports.len() as u128).max(1);
    eprintln!("\n-- summary --");
    eprintln!(
        "{:<10} {:>8} {:>8} {:>10} {:>10} {:>10} {:>10} {:>8}",
        "device", "load_ms", "exact", "final_avg", "final_rtf", "pass_avg", "pass_rtf", "word_chg"
    );
    eprintln!(
        "{:<10} {:>8} {:>5}/{} {:>10} {:>10.3} {:>10} {:>10.3} {:>5}/{}",
        "cpu",
        load_ms,
        exact,
        reports.len(),
        final_ms / clips_n,
        rtf(final_ms, audio_ms),
        pass_ms / pass_n,
        rtf(pass_ms, pass_audio_ms),
        changes,
        intervals
    );
    eprintln!(
        "gate: passes run {ran} skipped {skipped}, reused {reused}/{}, final==partial {eq_partial}/{}, gated==ungated {ungated_match}/{}, ungated avg {} ms",
        reports.len(),
        reports.len(),
        reports.len(),
        ungated_ms / clips_n
    );
    eprintln!(
        "{:<28} {:>8} {:>8} {:>8} {:>6} {:>5} {:>5} {:>6} {:>8} {:>4}  text",
        "clip",
        "audio_ms",
        "final_ms",
        "model_ms",
        "match",
        "run",
        "skip",
        "reused",
        "ungated",
        "seg"
    );
    for report in reports {
        eprintln!(
            "{:<28} {:>8} {:>8} {:>8} {:>6} {:>5} {:>5} {:>6} {:>8} {:>4}  {:?}  (expected {:?})",
            report.name,
            report.audio_ms,
            report.final_ms,
            report.model_ms,
            if report.matched { "yes" } else { "no" },
            report.ran,
            report.skipped,
            if report.reused { "yes" } else { "no" },
            if report.matches_ungated { "yes" } else { "no" },
            report.segments,
            report.text,
            report.expected
        );
        if !report.matches_ungated {
            eprintln!("    ungated {:?}", report.ungated);
        }
    }
}

fn rtf(elapsed_ms: u128, audio_ms: u128) -> f64 {
    if audio_ms == 0 {
        return 0.0;
    }
    elapsed_ms as f64 / audio_ms as f64
}

fn normalize(text: &str) -> String {
    let mut spaced = String::new();
    for ch in text.chars() {
        if ch.is_ascii_punctuation() {
            spaced.push(' ');
        } else {
            for lower in ch.to_lowercase() {
                spaced.push(lower);
            }
        }
    }
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn last_word(text: &str) -> String {
    normalize(text)
        .split_whitespace()
        .last()
        .unwrap_or("")
        .to_string()
}
