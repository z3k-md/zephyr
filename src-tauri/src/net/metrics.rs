//! What the last 15 minutes of probes say: per-hop latency, jitter and loss, drops, how much
//! a download slows the line, a grade, and one plain sentence. All of it is worked out from
//! the samples on demand, so nothing here keeps state.

use serde::Serialize;

/// One probe's outcome. `Skipped` is a tick that didn't send to this hop (offline, a gap, the
/// DNS check that runs every ten seconds); only `Lost` counts against the line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ping {
    Skipped,
    Lost,
    Ms(f32),
}

impl Ping {
    pub fn ms(self) -> Option<f32> {
        match self {
            Ping::Ms(ms) => Some(ms),
            _ => None,
        }
    }

    fn sent(self) -> bool {
        !matches!(self, Ping::Skipped)
    }
}

/// A tick after more than a few seconds (the computer asleep): a gap, not loss.
pub const GAP: u8 = 1;
/// The network changed (another router): latency starts a new baseline here.
pub const NET_CHANGE: u8 = 2;
/// The internet hop was timed by a TCP connect because ICMP got no answer.
pub const FALLBACK: u8 = 4;
/// A speed test ran.
pub const SPEED_TEST: u8 = 8;
/// No route to the internet at all.
pub const OFFLINE: u8 = 16;

#[derive(Debug, Clone, Copy)]
pub struct Sample {
    /// Seconds on the monitor's own clock, which only moves forward.
    pub at: f64,
    pub router: Ping,
    pub internet: Ping,
    pub dns: Ping,
    /// Bytes a second on the interface the internet goes through.
    pub down: u64,
    pub up: u64,
    pub flags: u8,
}

/// Latency to one hop: the middle and the slow end, how much it wobbles, and how much is
/// lost. None where nothing came back.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HopStats {
    pub median: Option<f32>,
    pub p95: Option<f32>,
    /// The mean change between one reply and the next.
    pub jitter: Option<f32>,
    /// Share of probes lost, 0–1.
    pub loss: f32,
    pub sent: u32,
    /// The latest probe: its time, or none when it got no answer (or none was sent).
    pub last: Option<f32>,
    /// Whether the latest probe got no answer.
    pub last_lost: bool,
}

pub fn hop(pings: impl Iterator<Item = Ping>) -> HopStats {
    let mut sent = 0u32;
    let mut lost = 0u32;
    let mut times = Vec::new();
    let mut previous: Option<f32> = None;
    let mut changes = Vec::new();
    let mut latest: Option<Ping> = None;
    for ping in pings {
        if !ping.sent() {
            continue;
        }
        sent += 1;
        latest = Some(ping);
        match ping {
            Ping::Ms(ms) => {
                if let Some(before) = previous {
                    changes.push((ms - before).abs());
                }
                previous = Some(ms);
                times.push(ms);
            }
            _ => lost += 1,
        }
    }
    times.sort_by(f32::total_cmp);
    HopStats {
        median: percentile(&times, 0.5),
        p95: percentile(&times, 0.95),
        jitter: (!changes.is_empty()).then(|| changes.iter().sum::<f32>() / changes.len() as f32),
        loss: if sent == 0 {
            0.0
        } else {
            lost as f32 / sent as f32
        },
        sent,
        last: latest.and_then(Ping::ms),
        last_lost: latest.is_some_and(|ping| ping.ms().is_none()),
    }
}

/// The value `share` of the way up sorted `values` (nearest rank).
pub fn percentile(sorted: &[f32], share: f32) -> Option<f32> {
    if sorted.is_empty() {
        return None;
    }
    let rank = ((share * sorted.len() as f32).ceil() as usize).clamp(1, sorted.len());
    Some(sorted[rank - 1])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Grade {
    Good,
    Fair,
    Poor,
}

/// Good: under 0.5% loss, jitter under 10 ms and the slow end under 100 ms. Poor: 2% loss or
/// more, or jitter of 30 ms or more. Fair between.
pub fn grade(stats: &HopStats) -> Grade {
    let jitter = stats.jitter.unwrap_or(0.0);
    let p95 = stats.p95.unwrap_or(0.0);
    if stats.loss >= 0.02 || jitter >= 30.0 {
        Grade::Poor
    } else if stats.loss < 0.005 && jitter < 10.0 && p95 < 100.0 {
        Grade::Good
    } else {
        Grade::Fair
    }
}

/// Three or more internet probes lost in a row.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Drop {
    pub start: f64,
    pub seconds: u32,
    /// The router kept answering, so the trouble was past it.
    pub past_router: bool,
}

pub fn drops(samples: &[Sample]) -> Vec<Drop> {
    let mut found = Vec::new();
    let mut run: Vec<&Sample> = Vec::new();
    let mut close = |run: &mut Vec<&Sample>| {
        if run.len() >= 3 {
            let router_answered = run.iter().filter(|s| s.router.ms().is_some()).count();
            found.push(Drop {
                start: run[0].at,
                seconds: run.len() as u32,
                past_router: router_answered * 2 > run.len(),
            });
        }
        run.clear();
    };
    for sample in samples {
        match sample.internet {
            Ping::Lost => run.push(sample),
            Ping::Ms(_) => close(&mut run),
            Ping::Skipped => {}
        }
    }
    close(&mut run);
    found
}

/// How much slower the internet answers while a download takes over half the peak, against
/// the rest of the time, and when that last happened. Only once the line has been pushed
/// (a peak over 1 MB/s), and only when it's more than 15 ms.
pub fn under_load(samples: &[Sample]) -> Option<(f32, f64)> {
    let peak = samples.iter().map(|s| s.down).max()?;
    if peak < 1_000_000 {
        return None;
    }
    let busy = |s: &&Sample| s.down * 2 > peak;
    let mut loaded: Vec<f32> = samples
        .iter()
        .filter(busy)
        .filter_map(|s| s.internet.ms())
        .collect();
    let mut idle: Vec<f32> = samples
        .iter()
        .filter(|s| !busy(s))
        .filter_map(|s| s.internet.ms())
        .collect();
    loaded.sort_by(f32::total_cmp);
    idle.sort_by(f32::total_cmp);
    let extra = percentile(&loaded, 0.5)? - percentile(&idle, 0.5)?;
    let last = samples.iter().rev().find(|s| busy(s))?.at;
    (extra > 15.0).then_some((extra, last))
}

/// "2 min ago", "40 s ago".
fn ago(seconds: f64) -> String {
    let seconds = seconds.max(0.0).round() as u64;
    if seconds < 90 {
        format!("{seconds} s ago")
    } else {
        format!("{} min ago", (seconds + 30) / 60)
    }
}

fn round(ms: f32) -> String {
    if ms < 10.0 {
        format!("{ms:.1}").trim_end_matches(".0").to_string()
    } else {
        format!("{}", ms.round())
    }
}

/// One plain sentence about the line, from the same numbers every time: "18 ms, jitter 2 ms,
/// 0.3% loss. 1 drop past your router (12 s, 2 min ago). +52 ms while downloading, 5 min
/// ago." `now` is the monitor's clock; `link` names the local network ("Wi-Fi").
pub fn verdict(samples: &[Sample], now: f64, link: &str) -> String {
    let internet = hop(samples.iter().map(|s| s.internet));
    let router = hop(samples.iter().map(|s| s.router));
    let Some(median) = internet.median else {
        return if samples.iter().rev().take(3).all(|s| s.flags & OFFLINE != 0) {
            "Offline: no route to the internet.".into()
        } else {
            "No answer from the internet yet.".into()
        };
    };
    let mut text = format!(
        "{} ms, jitter {} ms, {}% loss.",
        round(median),
        round(internet.jitter.unwrap_or(0.0)),
        loss_text(internet.loss)
    );
    let found = drops(samples);
    if let Some(last) = found.last() {
        let place = if last.past_router {
            "past your router".to_string()
        } else {
            format!("on your {link}")
        };
        let count = found.len();
        text.push_str(&format!(
            " {count} drop{} {place} ({} s, {}).",
            if count == 1 { "" } else { "s" },
            last.seconds,
            ago(now - last.start)
        ));
    } else if grade(&internet) != Grade::Good {
        if grade(&router) != Grade::Good {
            text.push_str(&format!(" Most of it is on your {link}."));
        } else {
            text.push_str(" It's past your router.");
        }
    }
    // Lookups are cached, so a slow resolver mostly shows as pages that pause before
    // starting; still worth naming.
    let dns = hop(samples.iter().map(|s| s.dns));
    if let Some(lookup) = dns.median.filter(|ms| *ms >= 250.0) {
        text.push_str(&format!(" DNS lookups are slow ({} ms).", round(lookup)));
    }
    if let Some((extra, at)) = under_load(samples) {
        text.push_str(&format!(
            " +{} ms while downloading, {}.",
            extra.round(),
            ago(now - at)
        ));
    }
    text
}

fn loss_text(loss: f32) -> String {
    let percent = loss * 100.0;
    if percent == 0.0 {
        "0".into()
    } else if percent < 10.0 {
        format!("{percent:.1}")
    } else {
        format!("{}", percent.round())
    }
}

/// Five seconds of the chart: the internet hop's median and slow end, what was lost, the
/// throughput, and anything marked.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    pub median: Option<f32>,
    pub p95: Option<f32>,
    pub lost: u32,
    pub sent: u32,
    pub down: u64,
    pub up: u64,
    pub flags: u8,
}

/// The last `count` stretches of `seconds` each, oldest first, ending at `now`.
pub fn buckets(samples: &[Sample], now: f64, count: usize, seconds: f64) -> Vec<Bucket> {
    let start = now - count as f64 * seconds;
    let mut groups: Vec<Vec<&Sample>> = vec![Vec::new(); count];
    for sample in samples {
        let index = ((sample.at - start) / seconds).floor();
        if index >= 0.0 && (index as usize) < count {
            groups[index as usize].push(sample);
        }
    }
    groups
        .into_iter()
        .map(|group| {
            let stats = hop(group.iter().map(|s| s.internet));
            let lost = group.iter().filter(|s| s.internet == Ping::Lost).count() as u32;
            let n = group.len().max(1) as u64;
            Bucket {
                median: stats.median,
                p95: stats.p95,
                lost,
                sent: stats.sent,
                down: group.iter().map(|s| s.down).sum::<u64>() / n,
                up: group.iter().map(|s| s.up).sum::<u64>() / n,
                flags: group.iter().fold(0, |flags, s| flags | s.flags),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(at: f64, router: Ping, internet: Ping) -> Sample {
        Sample {
            at,
            router,
            internet,
            dns: Ping::Skipped,
            down: 0,
            up: 0,
            flags: 0,
        }
    }

    #[test]
    fn hop_stats_skip_unsent_and_count_losses() {
        let pings = [
            Ping::Ms(10.0),
            Ping::Skipped,
            Ping::Ms(14.0),
            Ping::Lost,
            Ping::Ms(12.0),
        ];
        let stats = hop(pings.into_iter());
        assert_eq!(stats.sent, 4);
        assert_eq!(stats.loss, 0.25);
        assert_eq!(stats.median, Some(12.0));
        assert_eq!(stats.p95, Some(14.0));
        // |14-10| and |12-14|.
        assert_eq!(stats.jitter, Some(3.0));
    }

    #[test]
    fn grades_by_loss_jitter_and_the_slow_end() {
        let good = HopStats {
            median: Some(18.0),
            p95: Some(30.0),
            jitter: Some(2.0),
            loss: 0.0,
            sent: 900,
            ..Default::default()
        };
        assert_eq!(grade(&good), Grade::Good);
        assert_eq!(grade(&HopStats { loss: 0.01, ..good }), Grade::Fair);
        assert_eq!(grade(&HopStats { loss: 0.02, ..good }), Grade::Poor);
        assert_eq!(
            grade(&HopStats {
                jitter: Some(31.0),
                ..good
            }),
            Grade::Poor
        );
        assert_eq!(
            grade(&HopStats {
                p95: Some(150.0),
                ..good
            }),
            Grade::Fair
        );
    }

    #[test]
    fn a_drop_is_three_lost_in_a_row_and_says_where() {
        let mut samples: Vec<Sample> = (0..10)
            .map(|i| sample(i as f64, Ping::Ms(1.0), Ping::Ms(20.0)))
            .collect();
        for s in &mut samples[3..6] {
            s.internet = Ping::Lost;
        }
        samples[8].internet = Ping::Lost; // one alone isn't a drop
        let found = drops(&samples);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].seconds, 3);
        assert!(found[0].past_router);
        for s in &mut samples[3..6] {
            s.router = Ping::Lost;
        }
        assert!(!drops(&samples)[0].past_router);
    }

    #[test]
    fn under_load_compares_busy_and_idle_medians() {
        let mut samples: Vec<Sample> = (0..20)
            .map(|i| sample(i as f64, Ping::Ms(1.0), Ping::Ms(20.0)))
            .collect();
        for s in &mut samples[10..15] {
            s.down = 10_000_000;
            s.internet = Ping::Ms(80.0);
        }
        let (extra, at) = under_load(&samples).unwrap();
        assert_eq!(extra, 60.0);
        assert_eq!(at, 14.0);
        // Small downloads don't count.
        for s in &mut samples[10..15] {
            s.down = 100_000;
        }
        assert_eq!(under_load(&samples), None);
    }

    #[test]
    fn the_verdict_reads_the_numbers() {
        let mut samples: Vec<Sample> = (0..120)
            .map(|i| sample(i as f64, Ping::Ms(2.0), Ping::Ms(18.0)))
            .collect();
        assert_eq!(
            verdict(&samples, 120.0, "Wi-Fi"),
            "18 ms, jitter 0 ms, 0% loss."
        );
        for s in &mut samples[0..12] {
            s.internet = Ping::Lost;
        }
        assert_eq!(
            verdict(&samples, 120.0, "Wi-Fi"),
            "18 ms, jitter 0 ms, 10% loss. 1 drop past your router (12 s, 2 min ago)."
        );
        for s in &mut samples[0..12] {
            s.router = Ping::Lost;
        }
        assert!(verdict(&samples, 120.0, "Wi-Fi").contains("on your Wi-Fi"));
    }

    #[test]
    fn buckets_split_the_window_oldest_first() {
        let samples: Vec<Sample> = (0..10)
            .map(|i| {
                sample(
                    i as f64,
                    Ping::Ms(1.0),
                    if i == 7 {
                        Ping::Lost
                    } else {
                        Ping::Ms(i as f32)
                    },
                )
            })
            .collect();
        let found = buckets(&samples, 10.0, 2, 5.0);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].median, Some(2.0));
        assert_eq!((found[1].lost, found[1].sent), (1, 5));
    }
}
