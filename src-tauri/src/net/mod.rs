//! The connection monitor: once a second it pings the router and 1.1.1.1 (and every ten
//! seconds times a DNS lookup), keeping the last 15 minutes. From that the bar shows how the
//! line is doing (a card, the Network view, a footer chip when it's bad), and the throughput
//! meters read the interface the internet actually goes through. Settings › Network turns it
//! off.

pub mod dns;
pub mod ip;
pub mod metrics;
mod platform;
pub mod ports;
pub mod speed;

use std::collections::VecDeque;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;

use metrics::{Bucket, Grade, HopStats, Ping, Sample};

/// How much the monitor keeps, a sample a second.
pub const WINDOW: Duration = Duration::from_secs(15 * 60);
const TICK: Duration = Duration::from_secs(1);
const TIMEOUT: Duration = Duration::from_secs(1);
/// A lookup can fairly take longer than a ping.
const DNS_TIMEOUT: Duration = Duration::from_secs(3);
/// The route and DNS servers are read again this often, and DNS timed this often.
const REREAD: Duration = Duration::from_secs(10);
/// A tick after longer than this (the computer asleep) is a gap, not loss.
const GAP_AFTER: Duration = Duration::from_secs(3);
const INTERNET: Ipv4Addr = Ipv4Addr::new(1, 1, 1, 1);
const SECOND_OPINION: Ipv4Addr = Ipv4Addr::new(8, 8, 8, 8);
/// The chart: 180 stretches of five seconds.
const BUCKETS: usize = 180;
const BUCKET_SECONDS: f64 = 5.0;

static ENABLED: AtomicBool = AtomicBool::new(true);

struct Monitor {
    clock: Instant,
    samples: VecDeque<Sample>,
    route: platform::Route,
    dns: Option<Ipv4Addr>,
    alert: Option<Alert>,
    last_bad: Option<f64>,
}

fn monitor() -> &'static Mutex<Option<Monitor>> {
    static MONITOR: OnceLock<Mutex<Option<Monitor>>> = OnceLock::new();
    MONITOR.get_or_init(Default::default)
}

/// Turns the monitor on or off (Settings › Network); off forgets what it measured.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::SeqCst);
    if !enabled
        && let Ok(mut guard) = monitor().lock()
        && let Some(state) = guard.as_mut()
    {
        state.samples.clear();
        state.alert = None;
    }
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

/// The interface the internet goes through, as the throughput counters name it.
pub fn primary_interface() -> Option<String> {
    monitor().lock().ok()?.as_ref()?.route.interface.clone()
}

/// Marks the newest sample (a speed test ran).
pub fn mark(flag: u8) {
    if let Ok(mut guard) = monitor().lock()
        && let Some(sample) = guard.as_mut().and_then(|state| state.samples.back_mut())
    {
        sample.flags |= flag;
    }
}

pub fn start() {
    std::thread::Builder::new()
        .name("net-monitor".into())
        .spawn(run)
        .ok();
}

fn run() {
    let clock = Instant::now();
    if let Ok(mut guard) = monitor().lock() {
        *guard = Some(Monitor {
            clock,
            samples: VecDeque::new(),
            route: platform::Route::default(),
            dns: None,
            alert: None,
            last_bad: None,
        });
    }
    let (Some(mut router), Some(mut internet)) = (platform::Pinger::new(), platform::Pinger::new())
    else {
        log::warn!("network monitor: ICMP isn't available");
        return;
    };
    let mut route = platform::Route::default();
    let mut dns: Option<Ipv4Addr> = None;
    let mut reread: Option<Instant> = None;
    let mut last_tick: Option<Instant> = None;
    // Internet probes lost in a row, until when ICMP is swapped for a TCP connect (some
    // networks drop ICMP past the router), and until when 8.8.8.8 stands in for 1.1.1.1.
    let mut lost_in_a_row = 0u32;
    let mut tcp_until: Option<Instant> = None;
    let mut second_until: Option<Instant> = None;
    let dns_result: std::sync::Arc<Mutex<Option<Ping>>> = Default::default();
    loop {
        let started = Instant::now();
        if !enabled() {
            last_tick = None;
            std::thread::sleep(TICK);
            continue;
        }
        let mut flags = 0u8;
        if last_tick.is_some_and(|last| last.elapsed() > GAP_AFTER) {
            flags |= metrics::GAP;
        }
        last_tick = Some(started);

        let read_now = reread.is_none_or(|at| at.elapsed() >= REREAD);
        if read_now {
            reread = Some(started);
            let next = platform::route();
            if next.gateway != route.gateway && route.online {
                flags |= metrics::NET_CHANGE;
                lost_in_a_row = 0;
                tcp_until = None;
                second_until = None;
                ip::forget_public();
            }
            route = next;
            dns = platform::dns_servers().into_iter().next();
            if let Ok(mut guard) = monitor().lock()
                && let Some(state) = guard.as_mut()
            {
                state.route = route.clone();
                state.dns = dns;
            }
        }

        let (stats_down, stats_up) = {
            let totals = crate::stats::totals();
            (totals.net_down, totals.net_up)
        };
        let mut sample = Sample {
            at: started.duration_since(clock).as_secs_f64(),
            router: Ping::Skipped,
            internet: Ping::Skipped,
            dns: Ping::Skipped,
            down: stats_down,
            up: stats_up,
            flags,
        };

        if !route.online {
            sample.flags |= metrics::OFFLINE;
        } else {
            let now = Instant::now();
            // While ICMP seems blocked past the router, a TCP connect times the internet
            // instead, with ICMP tried again once a minute.
            let retry_icmp = tcp_until.is_some_and(|until| now >= until);
            let use_tcp = tcp_until.is_some() && !retry_icmp;
            let target = if second_until.is_some_and(|until| now < until) {
                SECOND_OPINION
            } else {
                INTERNET
            };
            let gateway = route.gateway;
            // Some routers take a second to resolve, so DNS is timed on its own thread and
            // lands in whichever tick follows it.
            if read_now && let Some(server) = dns {
                let result = dns_result.clone();
                std::thread::spawn(move || {
                    let ping = match platform::dns_rtt(server, DNS_TIMEOUT) {
                        Some(ms) => Ping::Ms(ms),
                        None => Ping::Lost,
                    };
                    if let Ok(mut slot) = result.lock() {
                        *slot = Some(ping);
                    }
                });
            }
            let (router_ping, internet_ping) = std::thread::scope(|scope| {
                let router_job = scope.spawn(|| gateway.map(|ip| router.ping(ip, TIMEOUT)));
                let internet_result = if use_tcp {
                    platform::tcp_rtt(target, TIMEOUT)
                } else {
                    internet.ping(target, TIMEOUT)
                };
                (router_job.join().ok().flatten(), internet_result)
            });
            sample.router = outcome(router_ping);
            if let Some(ping) = dns_result.lock().ok().and_then(|mut slot| slot.take()) {
                sample.dns = ping;
            }
            let mut internet_ping = internet_ping;
            let mut forgive = 0;
            if retry_icmp {
                if internet_ping.is_some() {
                    tcp_until = None;
                } else {
                    // Still blocked: back to TCP for another minute, and this tick by TCP too.
                    tcp_until = Some(now + Duration::from_secs(60));
                    internet_ping = platform::tcp_rtt(target, TIMEOUT);
                    sample.flags |= metrics::FALLBACK;
                }
            } else if use_tcp {
                sample.flags |= metrics::FALLBACK;
            }
            if internet_ping.is_some() {
                lost_in_a_row = 0;
            } else if !use_tcp {
                lost_in_a_row += 1;
                // 1.1.1.1 alone may be the one not answering: ask 8.8.8.8, and if it answers,
                // ping it for five minutes and don't count 1.1.1.1's silence as loss.
                if lost_in_a_row == 3 && target == INTERNET {
                    internet_ping = internet.ping(SECOND_OPINION, TIMEOUT);
                    if internet_ping.is_some() {
                        second_until = Some(now + Duration::from_secs(300));
                        forgive = 2;
                        lost_in_a_row = 0;
                    }
                }
                // Ten in a row while the router answers: if a TCP connect gets through, the
                // network drops ICMP past the router, which isn't loss.
                if lost_in_a_row >= 10
                    && lost_in_a_row.is_multiple_of(10)
                    && sample.router.ms().is_some()
                    && let Some(ms) = platform::tcp_rtt(target, TIMEOUT)
                {
                    internet_ping = Some(ms);
                    tcp_until = Some(now + Duration::from_secs(60));
                    sample.flags |= metrics::FALLBACK;
                    forgive = lost_in_a_row as usize - 1;
                    lost_in_a_row = 0;
                }
            }
            sample.internet = match internet_ping {
                Some(ms) => Ping::Ms(ms),
                None => Ping::Lost,
            };
            if forgive > 0
                && let Ok(mut guard) = monitor().lock()
                && let Some(state) = guard.as_mut()
            {
                for earlier in state.samples.iter_mut().rev().take(forgive) {
                    if earlier.internet == Ping::Lost {
                        earlier.internet = Ping::Skipped;
                    }
                }
            }
        }

        if let Ok(mut guard) = monitor().lock()
            && let Some(state) = guard.as_mut()
        {
            state.samples.push_back(sample);
            let oldest = sample.at - WINDOW.as_secs_f64();
            while state.samples.front().is_some_and(|s| s.at < oldest) {
                state.samples.pop_front();
            }
            update_alert(state, sample.at);
        }

        if let Some(wait) = TICK.checked_sub(started.elapsed()) {
            std::thread::sleep(wait);
        }
    }
}

/// What the footer shows when the line is in trouble.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Alert {
    /// Much slower than usual: the last five seconds' median.
    Slow {
        ms: f32,
    },
    /// Probes lost in a row just now.
    Drop,
    Offline,
}

/// Slow: five seconds with the median over twice the 15-minute baseline and over 80 ms.
/// Any drop or being offline counts at once. It clears after ten seconds of nothing wrong.
fn update_alert(state: &mut Monitor, now: f64) {
    let samples: Vec<Sample> = state.samples.iter().copied().collect();
    let recent: Vec<&Sample> = samples.iter().filter(|s| now - s.at < 5.0).collect();
    let offline = samples
        .last()
        .is_some_and(|s| s.flags & metrics::OFFLINE != 0);
    let dropping = samples
        .iter()
        .rev()
        .take(3)
        .filter(|s| s.internet == Ping::Lost)
        .count()
        == 3;
    let mut recent_times: Vec<f32> = recent.iter().filter_map(|s| s.internet.ms()).collect();
    recent_times.sort_by(f32::total_cmp);
    let baseline = metrics::hop(samples.iter().map(|s| s.internet)).median;
    let slow = match (metrics::percentile(&recent_times, 0.5), baseline) {
        (Some(now_ms), Some(base)) if recent.len() >= 5 && now_ms > base * 2.0 && now_ms > 80.0 => {
            Some(now_ms)
        }
        _ => None,
    };
    let bad = if offline {
        Some(Alert::Offline)
    } else if dropping {
        Some(Alert::Drop)
    } else {
        slow.map(|ms| Alert::Slow { ms })
    };
    match bad {
        Some(alert) => {
            state.alert = Some(alert);
            state.last_bad = Some(now);
        }
        None => {
            if state.last_bad.is_some_and(|at| now - at >= 10.0) {
                state.alert = None;
            }
        }
    }
}

/// The footer's chip, when there's trouble; read with each stats reading.
pub fn alert() -> Option<Alert> {
    if !enabled() {
        return None;
    }
    monitor().lock().ok()?.as_ref()?.alert
}

/// What the card shows: the grade, the numbers, a sentence, and a sparkline of 15 minutes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub watching: bool,
    pub measured: bool,
    pub grade: Grade,
    pub median: Option<f32>,
    pub jitter: Option<f32>,
    pub loss: f32,
    pub verdict: String,
    pub offline: bool,
    /// The internet's median for each of 180 five-second stretches, oldest first.
    pub spark: Vec<Option<f32>>,
    /// Which of those stretches lost probes.
    pub spark_lost: Vec<bool>,
    pub gateway: Option<String>,
    pub dns: Option<String>,
    /// The local network's name for sentences: "Wi-Fi" or "network".
    pub link: String,
    pub alert: Option<Alert>,
}

/// The Network view: the chart, the hops, the drops and the throughput.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub summary: Summary,
    pub buckets: Vec<Bucket>,
    pub router: HopStats,
    pub internet: HopStats,
    pub dns_hop: HopStats,
    /// Drops, by seconds ago and length.
    pub drops: Vec<DropAgo>,
    /// Bytes moved in the window, and the busiest second, each way.
    pub used_down: u64,
    pub used_up: u64,
    pub peak_down: u64,
    pub peak_up: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DropAgo {
    pub ago: f64,
    pub seconds: u32,
    pub past_router: bool,
}

fn link_word(route: &platform::Route) -> String {
    match route.interface.as_deref().map(ip::describe) {
        Some((ip::Link::Wifi, _)) => "Wi-Fi".into(),
        _ => "network".into(),
    }
}

/// A copy of what the monitor holds, taken under the lock and worked on outside it.
struct Snapshot {
    samples: Vec<Sample>,
    now: f64,
    route: platform::Route,
    dns: Option<Ipv4Addr>,
    alert: Option<Alert>,
}

fn snapshot() -> Option<Snapshot> {
    let guard = monitor().lock().ok()?;
    let state = guard.as_ref()?;
    Some(Snapshot {
        samples: state.samples.iter().copied().collect(),
        now: state.clock.elapsed().as_secs_f64(),
        route: state.route.clone(),
        dns: state.dns,
        alert: state.alert,
    })
}

fn summarize(snapshot: &Snapshot) -> Summary {
    let Snapshot {
        samples,
        now,
        route,
        dns,
        alert,
    } = snapshot;
    let internet = metrics::hop(samples.iter().map(|s| s.internet));
    let link = link_word(route);
    let buckets = metrics::buckets(samples, *now, BUCKETS, BUCKET_SECONDS);
    Summary {
        watching: enabled(),
        measured: internet.sent > 0,
        grade: metrics::grade(&internet),
        median: internet.median,
        jitter: internet.jitter,
        loss: internet.loss,
        verdict: metrics::verdict(samples, *now, &link),
        offline: samples
            .last()
            .is_some_and(|s| s.flags & metrics::OFFLINE != 0),
        spark: buckets.iter().map(|bucket| bucket.median).collect(),
        spark_lost: buckets.iter().map(|bucket| bucket.lost > 0).collect(),
        gateway: route.gateway.map(|ip| ip.to_string()),
        dns: dns.map(|ip| ip.to_string()),
        link,
        alert: if enabled() { *alert } else { None },
    }
}

pub fn summary() -> Option<Summary> {
    Some(summarize(&snapshot()?))
}

pub fn history() -> Option<History> {
    let snapshot = snapshot()?;
    let (samples, now) = (&snapshot.samples, snapshot.now);
    let drops = metrics::drops(samples)
        .into_iter()
        .map(|drop| DropAgo {
            ago: now - drop.start,
            seconds: drop.seconds,
            past_router: drop.past_router,
        })
        .collect();
    Some(History {
        summary: summarize(&snapshot),
        buckets: metrics::buckets(samples, now, BUCKETS, BUCKET_SECONDS),
        router: metrics::hop(samples.iter().map(|s| s.router)),
        internet: metrics::hop(samples.iter().map(|s| s.internet)),
        dns_hop: metrics::hop(samples.iter().map(|s| s.dns)),
        drops,
        used_down: samples.iter().map(|s| s.down).sum(),
        used_up: samples.iter().map(|s| s.up).sum(),
        peak_down: samples.iter().map(|s| s.down).max().unwrap_or(0),
        peak_up: samples.iter().map(|s| s.up).max().unwrap_or(0),
    })
}

/// A ping to 1.1.1.1 on its own socket, for the speed test's loaded-latency readings.
pub fn ping_internet() -> Option<f32> {
    thread_local! {
        static PINGER: std::cell::RefCell<Option<platform::Pinger>> =
            std::cell::RefCell::new(platform::Pinger::new());
    }
    PINGER.with(|pinger| pinger.borrow_mut().as_mut()?.ping(INTERNET, TIMEOUT))
}

fn outcome(ping: Option<Option<f32>>) -> Ping {
    match ping {
        None => Ping::Skipped,
        Some(Some(ms)) => Ping::Ms(ms),
        Some(None) => Ping::Lost,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run by hand: `cargo test probes_this_network -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn probes_this_network() {
        let route = platform::route();
        println!("route {route:?}");
        let mut pinger = platform::Pinger::new().expect("ICMP");
        if let Some(gateway) = route.gateway {
            println!("router {:?}", pinger.ping(gateway, TIMEOUT));
        }
        println!("1.1.1.1 {:?}", pinger.ping(INTERNET, TIMEOUT));
        println!("tcp {:?}", platform::tcp_rtt(INTERNET, TIMEOUT));
        let servers = platform::dns_servers();
        println!("dns servers {servers:?}");
        if let Some(server) = servers.first() {
            println!("dns {:?}", platform::dns_rtt(*server, DNS_TIMEOUT));
        }
    }
}
