//! The speed test, run only when asked: downloads then uploads against Cloudflare's speed
//! endpoints over several connections, pinging 1.1.1.1 four times a second throughout, so
//! the result says how fast the line is and how much slower it answers while full.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

const DOWN: &str = "https://speed.cloudflare.com/__down?bytes=";
const UP: &str = "https://speed.cloudflare.com/__up";
const STREAMS: usize = 6;
/// Each connection asks for bigger pieces as it goes, so slow lines still finish one early.
const DOWN_SIZES: [u64; 4] = [1_000_000, 5_000_000, 10_000_000, 25_000_000];
const UP_SIZES: [u64; 4] = [250_000, 1_000_000, 2_000_000, 5_000_000];
const DOWN_LIMIT: (Duration, u64) = (Duration::from_secs(8), 200_000_000);
const UP_LIMIT: (Duration, u64) = (Duration::from_secs(6), 50_000_000);
/// Throughput is read this often; the result is the 90th percentile of the readings.
const READ_EVERY: Duration = Duration::from_millis(250);
const PING_EVERY: Duration = Duration::from_millis(250);

static RUNNING: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);

/// The last test's numbers, kept in state.json.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedResult {
    /// When it ran, in ms since 1970.
    pub at: u64,
    pub down_mbps: f32,
    pub up_mbps: f32,
    /// The internet's median ping before the test, and while downloading and uploading.
    pub idle_ms: Option<f32>,
    pub down_ms: Option<f32>,
    pub up_ms: Option<f32>,
    /// How much a full line slows answers: A under 30 ms more, B under 60, C under 200, D.
    pub bloat: String,
}

/// Where a running test is, sent as "speed-test" events to the bar.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    /// `ping`, `download` or `upload`.
    phase: &'static str,
    /// The last second's throughput.
    mbps: Option<f32>,
    ping: Option<f32>,
    /// 0–1 through the phase.
    done: f32,
}

pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// Runs the test, telling the bar how it goes. Errors when one is already running, when it
/// was cancelled, or when nothing could be downloaded.
pub async fn run(app: &AppHandle) -> Result<SpeedResult, String> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("A speed test is already running".into());
    }
    CANCEL.store(false, Ordering::SeqCst);
    let result = test(app).await;
    RUNNING.store(false, Ordering::SeqCst);
    result
}

async fn test(app: &AppHandle) -> Result<SpeedResult, String> {
    let client = crate::util::http().clone();
    super::mark(super::metrics::SPEED_TEST);

    let idle = pings(app, "ping", Duration::from_secs(2)).await;
    cancelled()?;
    let (down, down_ms) = phase(app, &client, Direction::Down).await;
    cancelled()?;
    let (up, up_ms) = phase(app, &client, Direction::Up).await;
    cancelled()?;
    if down <= 0.0 {
        return Err("Couldn't reach the speed test server".into());
    }
    let idle_ms = median(idle);
    let added = [down_ms, up_ms]
        .into_iter()
        .flatten()
        .fold(None, |most: Option<f32>, ms| {
            Some(most.map_or(ms, |m| m.max(ms)))
        })
        .zip(idle_ms)
        .map(|(loaded, idle)| (loaded - idle).max(0.0));
    super::mark(super::metrics::SPEED_TEST);
    Ok(SpeedResult {
        at: crate::util::now_ms(),
        down_mbps: down,
        up_mbps: up,
        idle_ms,
        down_ms,
        up_ms,
        bloat: bloat_grade(added).into(),
    })
}

fn cancelled() -> Result<(), String> {
    if CANCEL.load(Ordering::SeqCst) {
        Err("Cancelled".into())
    } else {
        Ok(())
    }
}

fn bloat_grade(added: Option<f32>) -> &'static str {
    match added {
        None => "–",
        Some(ms) if ms < 30.0 => "A",
        Some(ms) if ms < 60.0 => "B",
        Some(ms) if ms < 200.0 => "C",
        Some(_) => "D",
    }
}

fn median(mut values: Vec<f32>) -> Option<f32> {
    values.sort_by(f32::total_cmp);
    super::metrics::percentile(&values, 0.5)
}

#[derive(Clone, Copy, PartialEq)]
enum Direction {
    Down,
    Up,
}

/// Pings 1.1.1.1 four times a second for `length` (or until told to stop), sending each
/// to the bar when it's the only thing running.
async fn pings(app: &AppHandle, phase: &'static str, length: Duration) -> Vec<f32> {
    let stop = Arc::new(AtomicBool::new(false));
    let app = app.clone();
    let found = ping_loop(stop.clone(), move |ping, elapsed| {
        let _ = app.emit_to(
            "main",
            "speed-test",
            Progress {
                phase,
                mbps: None,
                ping,
                done: (elapsed.as_secs_f32() / 2.0).min(1.0),
            },
        );
    });
    tokio::time::sleep(length).await;
    stop.store(true, Ordering::SeqCst);
    found.await.unwrap_or_default()
}

/// Pings on a thread of its own (an ICMP wait blocks) until `stop`, telling `each` every
/// result and how long it has been pinging; the replies' times, in ms.
fn ping_loop(
    stop: Arc<AtomicBool>,
    mut each: impl FnMut(Option<f32>, Duration) + Send + 'static,
) -> tauri::async_runtime::JoinHandle<Vec<f32>> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut found = Vec::new();
        let started = Instant::now();
        while !stop.load(Ordering::SeqCst) && !CANCEL.load(Ordering::SeqCst) {
            let tick = Instant::now();
            let ping = super::ping_internet();
            if let Some(ms) = ping {
                found.push(ms);
            }
            each(ping, started.elapsed());
            if let Some(wait) = PING_EVERY.checked_sub(tick.elapsed()) {
                std::thread::sleep(wait);
            }
        }
        found
    })
}

/// One direction: several connections at once until the time or byte limit, reading the
/// bytes moved every 250 ms. Returns the 90th percentile in Mb/s and the median ping.
async fn phase(
    app: &AppHandle,
    client: &reqwest::Client,
    direction: Direction,
) -> (f32, Option<f32>) {
    let (length, limit) = match direction {
        Direction::Down => DOWN_LIMIT,
        Direction::Up => UP_LIMIT,
    };
    let name = match direction {
        Direction::Down => "download",
        Direction::Up => "upload",
    };
    let moved = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let streams: Vec<_> = (0..STREAMS)
        .map(|_| {
            let (client, moved, stop) = (client.clone(), moved.clone(), stop.clone());
            tauri::async_runtime::spawn(async move {
                stream(&client, direction, &moved, &stop).await;
            })
        })
        .collect();
    let ping_stop = Arc::new(AtomicBool::new(false));
    let latest_ping = Arc::new(AtomicU64::new(u64::MAX));
    let pinger = {
        let latest = latest_ping.clone();
        ping_loop(ping_stop.clone(), move |ping, _| {
            if let Some(ms) = ping {
                latest.store(u64::from(ms.to_bits()), Ordering::SeqCst);
            }
        })
    };

    let started = Instant::now();
    let mut readings: Vec<f32> = Vec::new();
    let mut last = 0u64;
    let mut recent: std::collections::VecDeque<u64> = Default::default();
    while started.elapsed() < length
        && moved.load(Ordering::SeqCst) < limit
        && !CANCEL.load(Ordering::SeqCst)
    {
        tokio::time::sleep(READ_EVERY).await;
        let now = moved.load(Ordering::SeqCst);
        let delta = now - last;
        last = now;
        readings.push(mbps(delta, READ_EVERY));
        recent.push_back(delta);
        if recent.len() > 4 {
            recent.pop_front();
        }
        let ping = latest_ping.load(Ordering::SeqCst);
        let _ = app.emit_to(
            "main",
            "speed-test",
            Progress {
                phase: name,
                mbps: Some(mbps(recent.iter().sum(), READ_EVERY * recent.len() as u32)),
                ping: (ping != u64::MAX).then(|| f32::from_bits(ping as u32)),
                done: (started.elapsed().as_secs_f32() / length.as_secs_f32())
                    .max(now as f32 / limit as f32)
                    .min(1.0),
            },
        );
    }
    stop.store(true, Ordering::SeqCst);
    for task in streams {
        task.abort();
    }
    ping_stop.store(true, Ordering::SeqCst);
    let pings = pinger.await.unwrap_or_default();
    // The first readings are the connections opening and ramping up.
    let steady: Vec<f32> = readings.iter().skip(2).copied().collect();
    let mut sorted = if steady.is_empty() { readings } else { steady };
    sorted.sort_by(f32::total_cmp);
    (
        super::metrics::percentile(&sorted, 0.9).unwrap_or(0.0),
        median(pings),
    )
}

fn mbps(bytes: u64, over: Duration) -> f32 {
    (bytes as f64 * 8.0 / over.as_secs_f64().max(0.001) / 1_000_000.0) as f32
}

/// One connection's loop of requests, each bigger than the last up to the largest size,
/// adding to `moved` as bytes arrive (or leave).
async fn stream(
    client: &reqwest::Client,
    direction: Direction,
    moved: &Arc<AtomicU64>,
    stop: &AtomicBool,
) {
    let sizes = match direction {
        Direction::Down => DOWN_SIZES,
        Direction::Up => UP_SIZES,
    };
    let mut index = 0;
    while !stop.load(Ordering::SeqCst) && !CANCEL.load(Ordering::SeqCst) {
        let size = sizes[index.min(sizes.len() - 1)];
        index += 1;
        let ok = match direction {
            Direction::Down => download(client, size, moved).await,
            Direction::Up => upload(client, size, moved).await,
        };
        if !ok {
            // A refused or broken request: wait a moment rather than hammer.
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}

async fn download(client: &reqwest::Client, size: u64, moved: &AtomicU64) -> bool {
    let Ok(mut response) = client.get(format!("{DOWN}{size}")).send().await else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    while let Ok(Some(chunk)) = response.chunk().await {
        moved.fetch_add(chunk.len() as u64, Ordering::SeqCst);
    }
    true
}

async fn upload(client: &reqwest::Client, size: u64, moved: &Arc<AtomicU64>) -> bool {
    const PIECE: usize = 64 * 1024;
    let piece = bytes::Bytes::from(vec![0x5Au8; PIECE]);
    let pieces = size.div_ceil(PIECE as u64);
    let counter = moved.clone();
    // Counted as each piece is handed to the connection, which is as near to "sent" as the
    // client can see.
    let body = futures_util::stream::iter((0..pieces).map(move |_| {
        counter.fetch_add(PIECE as u64, Ordering::SeqCst);
        Ok::<_, std::io::Error>(piece.clone())
    }));
    client
        .post(UP)
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .header(reqwest::header::CONTENT_LENGTH, pieces * PIECE as u64)
        .body(reqwest::Body::wrap_stream(body))
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bloat_grades_by_added_latency() {
        assert_eq!(bloat_grade(Some(12.0)), "A");
        assert_eq!(bloat_grade(Some(45.0)), "B");
        assert_eq!(bloat_grade(Some(150.0)), "C");
        assert_eq!(bloat_grade(Some(400.0)), "D");
        assert_eq!(bloat_grade(None), "–");
    }

    #[test]
    fn reads_megabits() {
        assert_eq!(mbps(1_000_000, Duration::from_secs(1)), 8.0);
        assert_eq!(mbps(250_000, Duration::from_millis(250)), 8.0);
    }
}
