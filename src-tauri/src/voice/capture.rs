//! One microphone capture, open only while a [`Capture`] value is alive.
//!
//! Dropping it drops the `cpal` stream, which is what turns the system mic indicator off.
//! Samples stay in a memory buffer. Nothing here writes audio or logs sample values.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, SizedSample, Stream};

/// Speech model sample rate.
pub const TARGET_RATE: u32 = 16_000;
/// One meter frame: 50 ms at 16 kHz, which is 20 updates a second.
pub const LEVEL_WINDOW: usize = TARGET_RATE as usize / 20;
/// A normal speaking voice sits well under full scale. This brings it onto a 0..1 meter.
const LEVEL_GAIN: f32 = 5.0;
/// Just past the 30 s hold cap, so a late callback cannot grow the buffer without bound.
const MAX_SAMPLES: usize = TARGET_RATE as usize * 31;

const NO_MICROPHONE: &str = "No microphone was found.";

/// Averages interleaved frames down to one mono sample per frame.
pub fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels == 0 {
        return Vec::new();
    }
    if channels == 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Root-mean-square of a window, scaled into 0..1 for the bar's meter.
pub fn level_of(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|sample| sample * sample).sum();
    let rms = (sum_sq / samples.len() as f32).sqrt();
    (rms * LEVEL_GAIN).clamp(0.0, 1.0)
}

/// Linear resampler. Enough for speech, and it stays exact across chunk boundaries.
pub struct Resampler {
    /// Input samples advanced for each output sample.
    step: f64,
    /// Index of the next output sample relative to the next input chunk. Negative means it
    /// still lies on the previous chunk's last sample.
    pos: f64,
    prev: f32,
    passthrough: bool,
}

impl Resampler {
    pub fn new(from_rate: u32) -> Self {
        Self {
            step: if from_rate == 0 {
                1.0
            } else {
                f64::from(from_rate) / f64::from(TARGET_RATE)
            },
            pos: 0.0,
            prev: 0.0,
            passthrough: from_rate == TARGET_RATE,
        }
    }

    pub fn push(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() {
            return Vec::new();
        }
        if self.passthrough {
            return input.to_vec();
        }
        let mut out = Vec::new();
        let mut idx = self.pos;
        let frames = input.len() as f64;
        loop {
            if idx >= frames {
                break;
            }
            let at = idx.floor();
            let frac = (idx - at) as f32;
            let i = at as isize;
            let next = i + 1;
            // The sample just past this chunk belongs to the next callback.
            if next >= input.len() as isize {
                break;
            }
            let a = if i < 0 { self.prev } else { input[i as usize] };
            let b = if next < 0 {
                self.prev
            } else {
                input[next as usize]
            };
            out.push(a + (b - a) * frac);
            idx += self.step;
        }
        self.prev = input[input.len() - 1];
        self.pos = idx - frames;
        out
    }
}

/// What to tell the user when the device cannot be opened.
pub fn explain_open_error(err: &cpal::Error) -> String {
    match err.kind() {
        cpal::ErrorKind::PermissionDenied => permission_message().to_string(),
        cpal::ErrorKind::DeviceNotAvailable | cpal::ErrorKind::HostUnavailable => {
            NO_MICROPHONE.to_string()
        }
        _ if access_denied(&err.to_string()) => permission_message().to_string(),
        _ => format!("Couldn't open the microphone: {err}"),
    }
}

fn access_denied(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("denied")
        || lower.contains("not authorized")
        || lower.contains("permission")
        || lower.contains("privacy")
        || lower.contains("0x80070005")
}

#[cfg(windows)]
fn permission_message() -> &'static str {
    "Allow microphone access in Windows Settings > Privacy & security > Microphone"
}

#[cfg(target_os = "macos")]
fn permission_message() -> &'static str {
    "Allow microphone access in System Settings > Privacy & Security > Microphone"
}

#[cfg(not(any(windows, target_os = "macos")))]
fn permission_message() -> &'static str {
    "Allow microphone access for Zephyr in your system settings"
}

struct Shared {
    samples: Mutex<Vec<f32>>,
    level: AtomicU32,
    error: Mutex<Option<String>>,
}

/// The open microphone. Drop it to close the device.
pub struct Capture {
    stream: Option<Stream>,
    shared: Arc<Shared>,
}

impl Capture {
    /// Default input device, its default format, downmixed and resampled to 16 kHz mono.
    pub fn open() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| NO_MICROPHONE.to_string())?;
        let supported = device
            .default_input_config()
            .map_err(|err| explain_open_error(&err))?;
        let rate = supported.sample_rate();
        let channels = supported.channels() as usize;
        let format = supported.sample_format();
        if rate == 0 || channels == 0 {
            return Err(NO_MICROPHONE.to_string());
        }
        log::info!("voice capture opening: {rate} Hz, {channels} ch, {format}");
        let shared = Arc::new(Shared {
            samples: Mutex::new(Vec::new()),
            level: AtomicU32::new(0),
            error: Mutex::new(None),
        });
        let stream = build_stream(format, &device, supported.config(), channels, rate, &shared)?;
        stream.play().map_err(|err| explain_open_error(&err))?;
        Ok(Self {
            stream: Some(stream),
            shared,
        })
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.shared.level.load(Ordering::Relaxed))
    }

    pub fn len(&self) -> usize {
        lock(&self.shared.samples).len()
    }

    /// A copy of what has been recorded so far. The stream keeps going.
    pub fn snapshot(&self) -> Vec<f32> {
        lock(&self.shared.samples).clone()
    }

    /// Borrows the recorded samples. The stream keeps going.
    pub fn with_samples<R>(&self, body: impl FnOnce(&[f32]) -> R) -> R {
        body(&lock(&self.shared.samples))
    }

    pub fn take_error(&self) -> Option<String> {
        lock(&self.shared.error).take()
    }

    /// Stops the stream, then returns the buffer. The mic indicator drops with the stream.
    pub fn into_samples(mut self) -> Vec<f32> {
        let count = lock(&self.shared.samples).len();
        self.stream.take();
        log::info!("voice capture closed, {count} samples");
        std::mem::take(&mut *lock(&self.shared.samples))
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        // Stop callbacks before the buffer goes away.
        self.stream.take();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

struct Callback {
    resampler: Resampler,
    channels: usize,
    pending: Vec<f32>,
    shared: Arc<Shared>,
}

impl Callback {
    fn push_pcm(&mut self, pcm: &[f32]) {
        if pcm.is_empty() {
            return;
        }
        {
            let mut buf = lock(&self.shared.samples);
            let room = MAX_SAMPLES.saturating_sub(buf.len());
            buf.extend(pcm.iter().copied().take(room));
        }
        self.pending.extend_from_slice(pcm);
        while self.pending.len() >= LEVEL_WINDOW {
            let window: Vec<f32> = self.pending.drain(..LEVEL_WINDOW).collect();
            let level = level_of(&window);
            self.shared.level.store(level.to_bits(), Ordering::Relaxed);
        }
    }
}

fn build_stream(
    format: SampleFormat,
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    rate: u32,
    shared: &Arc<Shared>,
) -> Result<Stream, String> {
    let error_shared = Arc::clone(shared);
    let on_error = move |err: cpal::Error| {
        // Device-not-available after a route change is not a privacy failure.
        if matches!(
            err.kind(),
            cpal::ErrorKind::DeviceChanged | cpal::ErrorKind::Xrun
        ) {
            log::debug!("voice capture notice: {err}");
            return;
        }
        let message = explain_open_error(&err);
        log::error!("voice capture: {message}");
        *lock(&error_shared.error) = Some(message);
    };
    match format {
        SampleFormat::F32 => typed::<f32>(device, config, channels, rate, shared, on_error),
        SampleFormat::F64 => typed::<f64>(device, config, channels, rate, shared, on_error),
        SampleFormat::I8 => typed::<i8>(device, config, channels, rate, shared, on_error),
        SampleFormat::I16 => typed::<i16>(device, config, channels, rate, shared, on_error),
        SampleFormat::I24 => typed::<cpal::I24>(device, config, channels, rate, shared, on_error),
        SampleFormat::I32 => typed::<i32>(device, config, channels, rate, shared, on_error),
        SampleFormat::I64 => typed::<i64>(device, config, channels, rate, shared, on_error),
        SampleFormat::U8 => typed::<u8>(device, config, channels, rate, shared, on_error),
        SampleFormat::U16 => typed::<u16>(device, config, channels, rate, shared, on_error),
        SampleFormat::U24 => typed::<cpal::U24>(device, config, channels, rate, shared, on_error),
        SampleFormat::U32 => typed::<u32>(device, config, channels, rate, shared, on_error),
        SampleFormat::U64 => typed::<u64>(device, config, channels, rate, shared, on_error),
        other => Err(format!(
            "Couldn't open the microphone: unsupported format {other}"
        )),
    }
}

fn typed<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    rate: u32,
    shared: &Arc<Shared>,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<Stream, String>
where
    T: SizedSample,
    f32: cpal::FromSample<T>,
{
    let mut callback = Callback {
        resampler: Resampler::new(rate),
        channels,
        pending: Vec::with_capacity(LEVEL_WINDOW),
        shared: Arc::clone(shared),
    };
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let mono = downmix(&to_f32(data), callback.channels);
                let pcm = callback.resampler.push(&mono);
                callback.push_pcm(&pcm);
            },
            on_error,
            Some(Duration::from_secs(5)),
        )
        .map_err(|err| explain_open_error(&err))
}

fn to_f32<T>(data: &[T]) -> Vec<f32>
where
    T: Sample,
    f32: cpal::FromSample<T>,
{
    data.iter().copied().map(Sample::to_sample).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_channels_and_keeps_mono() {
        assert_eq!(downmix(&[0.2, 0.4, 0.6], 1), vec![0.2, 0.4, 0.6]);
        let stereo = downmix(&[0.0, 1.0, 0.5, 0.5], 2);
        assert_eq!(stereo, vec![0.5, 0.5]);
        // A trailing partial frame is not a sample.
        assert_eq!(downmix(&[1.0, 3.0, 9.0], 2), vec![2.0]);
        assert!(downmix(&[1.0], 0).is_empty());
    }

    #[test]
    fn level_is_scaled_rms_in_range() {
        assert_eq!(level_of(&[]), 0.0);
        assert_eq!(level_of(&[0.0, 0.0, 0.0]), 0.0);
        let quiet = level_of(&[0.1, -0.1, 0.1, -0.1]);
        assert!((quiet - 0.5).abs() < 1e-5, "{quiet}");
        assert_eq!(level_of(&[1.0, -1.0, 1.0]), 1.0);
    }

    #[test]
    fn resampler_keeps_rate_and_matches_across_chunks() {
        let mut same = Resampler::new(TARGET_RATE);
        assert_eq!(same.push(&[0.25, -0.5, 0.75]), vec![0.25, -0.5, 0.75]);

        let input: Vec<f32> = (0..12).map(|n| n as f32).collect();
        let mut whole = Resampler::new(48_000);
        let once = whole.push(&input);

        let mut chunked = Resampler::new(48_000);
        let mut parts = Vec::new();
        for chunk in input.chunks(4) {
            parts.extend(chunked.push(chunk));
        }
        assert_eq!(once, parts);
        // 48 kHz -> 16 kHz keeps every third sample of a ramp: 0, 3, 6, 9.
        assert_eq!(once, vec![0.0, 3.0, 6.0, 9.0]);

        let mut flat = Resampler::new(48_000);
        let ones = vec![1.0; 9];
        assert!(
            flat.push(&ones)
                .iter()
                .all(|sample| (*sample - 1.0).abs() < 1e-6)
        );
    }

    #[test]
    fn permission_errors_name_the_system_setting() {
        let denied = cpal::Error::with_message(cpal::ErrorKind::PermissionDenied, "nope");
        let message = explain_open_error(&denied);
        #[cfg(windows)]
        assert_eq!(
            message,
            "Allow microphone access in Windows Settings > Privacy & security > Microphone"
        );
        #[cfg(target_os = "macos")]
        assert_eq!(
            message,
            "Allow microphone access in System Settings > Privacy & Security > Microphone"
        );
        let wording = cpal::Error::with_message(cpal::ErrorKind::BackendError, "Access is denied");
        assert_eq!(explain_open_error(&wording), message);
        let missing = cpal::Error::new(cpal::ErrorKind::DeviceNotAvailable);
        assert_eq!(explain_open_error(&missing), NO_MICROPHONE);
    }
}
