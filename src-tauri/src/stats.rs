//! Live system stats for the bar's footer and System Resources: CPU, GPU, memory, free space
//! on the system disk and network throughput. A background thread reads them once a second
//! (about a millisecond each: totals only, never the process list), so Resources always
//! opens with the last 15 minutes. While the bar shows it reads twice a second and sends each
//! reading to the footer.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use sysinfo::{Disks, Networks, System};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    /// Across all cores, 0–100.
    pub cpu: f32,
    pub memory_used: u64,
    pub memory_total: u64,
    pub disk_free: u64,
    pub disk_total: u64,
    /// Bytes per second since the last sample, on the interface the internet goes through.
    pub net_down: u64,
    pub net_up: u64,
    /// None without a GPU that reports its load.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<crate::gpu::Gpu>,
    /// The CPU and the GPU, in °C; none where they can't be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_temp: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_temp: Option<f32>,
    /// Trouble on the line (slow, dropping, offline), for the footer's chip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<crate::net::Alert>,
    /// Dev builds: the app's own use, its webview helper processes included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<AppUsage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUsage {
    /// Percent of one core, summed over the processes, as Activity Monitor counts it.
    pub cpu: f32,
    /// Resident memory (the physical footprint on macOS).
    pub memory: u64,
    pub processes: usize,
}

struct Sampler {
    system: System,
    disks: Disks,
    networks: Networks,
    gpu: Option<crate::gpu::Reader>,
    last: Instant,
    /// The latest reading, which the footer and Resources show.
    latest: Option<Stats>,
    /// The last 15 minutes of totals, with when each was read.
    history: std::collections::VecDeque<(Instant, Point)>,
    /// Each logical processor's load (0–100) a second at a time, for the CPU drill-in.
    cores: std::collections::VecDeque<(Instant, Vec<u8>)>,
    /// Dev builds: the app's own use and when it was read, which walks every process, so
    /// the footer's twice-a-second readings reuse it for a second.
    app: Option<(Instant, AppUsage)>,
    /// The CPU and GPU temperatures and when they were read: every other second is plenty,
    /// and a read walks a few dozen sensors.
    temps: Option<(Instant, Temps)>,
    /// When the GPU was last read, which a hidden tick reads only every few seconds.
    gpu_read: Option<Instant>,
    /// Windows: each process's GPU load from the last full read, for Resources' GPU column.
    gpu_by_process: std::collections::HashMap<u32, f32>,
}

/// The hottest CPU and GPU sensors, in °C.
type Temps = (Option<f32>, Option<f32>);

/// One sample of the totals, for the sparklines.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    /// Seconds ago, so a gap (the computer asleep) shows as one.
    pub age: f32,
    pub cpu: f32,
    /// Memory in use, 0–1.
    pub memory: f32,
    /// The GPU's load, 0–100; 0 without one.
    pub gpu: f32,
    pub down: u64,
    pub up: u64,
}

/// How far back the sparklines go, and how often the thread reads.
const WINDOW: Duration = Duration::from_secs(15 * 60);
const EVERY: Duration = Duration::from_secs(1);

/// The last 15 minutes of totals, oldest first.
pub fn history() -> Vec<Point> {
    let Ok(guard) = SAMPLER.lock() else {
        return Vec::new();
    };
    let Some(sampler) = guard.as_ref() else {
        return Vec::new();
    };
    sampler
        .history
        .iter()
        .map(|(at, point)| Point {
            age: at.elapsed().as_secs_f32(),
            ..*point
        })
        .filter(|point| point.age <= WINDOW.as_secs_f32())
        .collect()
}

/// The history in `count` stretches, oldest first, each the highest of every reading in it
/// (so a spike survives) at the newest one's age. A stretch with no reading (asleep) is left
/// out, so the gap shows. 120 of them draw the same sparkline as 900 points, for an eighth of
/// what crosses to the page.
pub fn history_buckets(count: usize) -> Vec<Point> {
    let points = history().into_iter().map(|point| (point.age, point));
    buckets(points, WINDOW.as_secs_f32(), count, |kept, point| Point {
        age: kept.age.min(point.age),
        cpu: kept.cpu.max(point.cpu),
        memory: kept.memory.max(point.memory),
        gpu: kept.gpu.max(point.gpu),
        down: kept.down.max(point.down),
        up: kept.up.max(point.up),
    })
    .into_iter()
    .map(Point::rounded)
    .collect()
}

/// Readings over the last `window` seconds, each with its age, in `count` equal stretches,
/// oldest first, the readings in each folded together by `merge`. Empty stretches are left
/// out, as are readings older than the window.
pub(crate) fn buckets<T: Copy>(
    readings: impl IntoIterator<Item = (f32, T)>,
    window: f32,
    count: usize,
    merge: impl Fn(T, T) -> T,
) -> Vec<T> {
    if count == 0 {
        return Vec::new();
    }
    let span = window / count as f32;
    let mut found: Vec<Option<T>> = vec![None; count];
    for (age, reading) in readings {
        let index = ((window - age) / span).floor();
        if index < 0.0 {
            continue;
        }
        let slot = &mut found[(index as usize).min(count - 1)];
        *slot = Some(match *slot {
            None => reading,
            Some(kept) => merge(kept, reading),
        });
    }
    found.into_iter().flatten().collect()
}

impl Point {
    /// Trimmed to what a sparkline can show, which roughly halves the JSON.
    fn rounded(self) -> Point {
        let tenth = |value: f32| (value * 10.0).round() / 10.0;
        Point {
            age: tenth(self.age),
            cpu: tenth(self.cpu),
            memory: (self.memory * 1000.0).round() / 1000.0,
            gpu: tenth(self.gpu),
            ..self
        }
    }
}

/// How often the thread reads while the bar shows, so the footer's meters move smoothly.
const EVERY_SHOWN: Duration = Duration::from_millis(500);
/// How often the GPU is read while the bar is hidden and Resources closed: its counters
/// cover every engine of every process, a lot to collect each second for the history alone.
const GPU_HIDDEN_EVERY: Duration = Duration::from_secs(5);
/// The shortest gap between reads: CPU use over less means little.
const AT_LEAST: Duration = Duration::from_millis(250);

static BAR_VISIBLE: AtomicBool = AtomicBool::new(false);
/// Whether System Resources is open, which reads the full set even with the bar hidden.
static RESOURCES_OPEN: AtomicBool = AtomicBool::new(false);

pub fn set_resources_open(open: bool) {
    RESOURCES_OPEN.store(open, Ordering::SeqCst);
}

static THREAD: OnceLock<std::thread::Thread> = OnceLock::new();

/// Whether the bar is showing, for work that only matters while it does (macOS now playing).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn bar_visible() -> bool {
    BAR_VISIBLE.load(Ordering::SeqCst)
}

/// Called as the bar shows and hides. Showing wakes the thread, so the footer gets a fresh
/// reading right away.
pub fn set_bar_visible(visible: bool) {
    let was = BAR_VISIBLE.swap(visible, Ordering::SeqCst);
    if visible
        && !was
        && let Some(thread) = THREAD.get()
    {
        thread.unpark();
    }
}

/// Starts the background reading for the life of the app: once a second, twice while the bar
/// shows, when each reading goes to the bar as a "stats" event.
pub fn start(app: AppHandle) {
    let thread = std::thread::spawn(move || {
        let mut last = Instant::now() - EVERY;
        loop {
            if let Some(wait) = AT_LEAST.checked_sub(last.elapsed()) {
                std::thread::sleep(wait);
            }
            last = Instant::now();
            let visible = BAR_VISIBLE.load(Ordering::SeqCst);
            let reading = tick(visible || RESOURCES_OPEN.load(Ordering::SeqCst));
            if visible && let Some(stats) = reading {
                let _ = app.emit_to("main", "stats", stats);
            }
            std::thread::park_timeout(if BAR_VISIBLE.load(Ordering::SeqCst) {
                EVERY_SHOWN
            } else {
                EVERY
            });
        }
    });
    let _ = THREAD.set(thread.thread().clone());
}

/// Logical cores, for CPU percentages.
pub fn cores() -> usize {
    std::thread::available_parallelism().map_or(1, |cores| cores.get())
}

static SAMPLER: Mutex<Option<Sampler>> = Mutex::new(None);

/// The disk the system runs from: `/` on macOS and Linux, the system drive on Windows.
fn system_mount() -> std::path::PathBuf {
    if cfg!(windows) {
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        return format!("{drive}\\").into();
    }
    "/".into()
}

/// The latest reading, with the dev build's own use read now.
pub fn sample() -> Stats {
    let mut stats = totals();
    if cfg!(debug_assertions)
        && let Ok(mut guard) = SAMPLER.lock()
        && let Some(sampler) = guard.as_mut()
    {
        stats.app = Some(recent_app_usage(sampler));
    }
    stats
}

/// The latest reading, without the dev build's own use, which walks every process.
pub fn totals() -> Stats {
    let Ok(guard) = SAMPLER.lock() else {
        return empty();
    };
    if let Some(stats) = guard.as_ref().and_then(|sampler| sampler.latest.clone()) {
        return stats;
    }
    drop(guard);
    tick(true).unwrap_or_else(empty)
}

impl Sampler {
    fn new() -> Self {
        let mut system = System::new();
        system.refresh_cpu_usage();
        Sampler {
            system,
            disks: Disks::new_with_refreshed_list(),
            networks: Networks::new_with_refreshed_list(),
            gpu: crate::gpu::Reader::new(),
            last: Instant::now(),
            latest: None,
            history: std::collections::VecDeque::new(),
            cores: std::collections::VecDeque::new(),
            app: None,
            temps: None,
            gpu_read: None,
            gpu_by_process: std::collections::HashMap::new(),
        }
    }
}

/// The dev build's own use, read at most once a second.
fn recent_app_usage(sampler: &mut Sampler) -> AppUsage {
    if let Some((at, usage)) = &sampler.app
        && at.elapsed() < EVERY
    {
        return usage.clone();
    }
    let usage = app_usage(&mut sampler.system);
    sampler.app = Some((Instant::now(), usage.clone()));
    usage
}

/// One reading. The GPU's counters (a couple of milliseconds) are read with the reader taken
/// out of the sampler, so nothing waits on the lock meanwhile. `full` is false while the bar
/// is hidden and Resources closed: then only what the history keeps is read, and the GPU only
/// every few seconds.
fn tick(full: bool) -> Option<Stats> {
    // Only Resources shows each process's GPU load.
    let per_process = RESOURCES_OPEN.load(Ordering::SeqCst);
    let (mut reader, reuse) = {
        let mut guard = SAMPLER.lock().ok()?;
        let sampler = guard.get_or_insert_with(Sampler::new);
        // While nothing shows it, the GPU is read every few seconds, the last reading
        // standing in between.
        if !full
            && sampler
                .gpu_read
                .is_some_and(|at| at.elapsed() < GPU_HIDDEN_EVERY)
        {
            (
                None,
                Some(sampler.latest.as_ref().and_then(|s| s.gpu.clone())),
            )
        } else {
            (sampler.gpu.take(), None)
        }
    };
    let gpu = match reuse {
        Some(gpu) => gpu,
        None => reader.as_mut().and_then(|reader| reader.read(per_process)),
    };
    let mut guard = SAMPLER.lock().ok()?;
    let sampler = guard.get_or_insert_with(Sampler::new);
    if let Some(mut reader) = reader {
        sampler.gpu_read = Some(Instant::now());
        if per_process {
            sampler.gpu_by_process = reader.take_by_process();
        }
        if sampler.gpu.is_none() {
            sampler.gpu = Some(reader);
        }
    }
    Some(record(sampler, gpu, full))
}

/// Reads the totals and adds them to the history, a point a second. CPU usage and network
/// rates compare against the previous reading, so the very first reads them as zero.
fn record(sampler: &mut Sampler, gpu: Option<crate::gpu::Gpu>, full: bool) -> Stats {
    sampler.system.refresh_cpu_usage();
    sampler.system.refresh_memory();
    sampler.networks.refresh(true);
    let elapsed = sampler.last.elapsed().as_secs_f64().max(0.001);
    sampler.last = Instant::now();

    let mount = system_mount();
    // Free space and temperatures only show on screen, so a hidden tick keeps the last ones.
    let previous = sampler.latest.as_ref().map(|stats| {
        (
            stats.disk_free,
            stats.disk_total,
            stats.cpu_temp,
            stats.gpu_temp,
            stats.network,
        )
    });
    let disk = sampler
        .disks
        .list_mut()
        .iter_mut()
        .find(|disk| disk.mount_point() == Path::new(&mount));
    let (disk_free, disk_total) = match (disk, previous) {
        (_, Some((free, total, ..))) if !full => (free, total),
        (Some(disk), _) => {
            disk.refresh();
            (disk.available_space(), disk.total_space())
        }
        (None, _) => (0, 0),
    };
    let (down, up) = throughput(&sampler.networks);
    // Windows: the GPU's from its driver, the CPU's from the PawnIO helper (cputemp.rs).
    // macOS: both from the SMC (temps.rs), read at most every two seconds.
    let (cpu_temp, gpu_temp) = if let (false, Some((_, _, cpu, gpu, _))) = (full, previous) {
        (cpu, gpu)
    } else if cfg!(windows) {
        (
            crate::cputemp::read(),
            gpu.as_ref().and_then(|gpu| gpu.temperature),
        )
    } else {
        match sampler.temps {
            Some((at, temps)) if at.elapsed() < Duration::from_secs(2) => temps,
            _ => {
                let temps = crate::temps::read();
                sampler.temps = Some((Instant::now(), temps));
                temps
            }
        }
    };
    let stats = Stats {
        cpu: sampler.system.global_cpu_usage(),
        memory_used: sampler.system.used_memory(),
        memory_total: sampler.system.total_memory(),
        disk_free,
        disk_total,
        net_down: (down as f64 / elapsed) as u64,
        net_up: (up as f64 / elapsed) as u64,
        gpu,
        cpu_temp,
        gpu_temp,
        network: if full {
            crate::net::alert()
        } else {
            previous.and_then(|(.., network)| network)
        },
        app: None,
    };
    while sampler
        .history
        .front()
        .is_some_and(|(at, _)| at.elapsed() > WINDOW)
    {
        sampler.history.pop_front();
    }
    while sampler
        .cores
        .front()
        .is_some_and(|(at, _)| at.elapsed() > WINDOW)
    {
        sampler.cores.pop_front();
    }
    // Reads twice a second while the bar shows; the history keeps one a second. The slack
    // allows for the thread waking a little early.
    let due = sampler
        .history
        .back()
        .is_none_or(|(at, _)| at.elapsed() + Duration::from_millis(50) >= EVERY);
    if due {
        let per_core = sampler
            .system
            .cpus()
            .iter()
            .map(|cpu| cpu.cpu_usage().round().clamp(0.0, 100.0) as u8)
            .collect();
        sampler.cores.push_back((Instant::now(), per_core));
        sampler.history.push_back((
            Instant::now(),
            Point {
                age: 0.0,
                cpu: stats.cpu,
                memory: if stats.memory_total > 0 {
                    stats.memory_used as f32 / stats.memory_total as f32
                } else {
                    0.0
                },
                gpu: stats.gpu.as_ref().map_or(0.0, |gpu| gpu.usage),
                down: stats.net_down,
                up: stats.net_up,
            },
        ));
    }
    sampler.latest = Some(stats.clone());
    stats
}

/// The CPU drill-in's heatmap: each logical processor's average load in 90 stretches of ten
/// seconds, oldest first, with none where there's no reading (before Zephyr started).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreHeat {
    pub cores: usize,
    /// `columns[time][core]`, 0–100.
    pub columns: Vec<Option<Vec<u8>>>,
    /// Each processor's latest load.
    pub now: Vec<u8>,
}

pub fn core_heat() -> CoreHeat {
    const COLUMNS: usize = 90;
    let span = WINDOW.as_secs_f32() / COLUMNS as f32;
    let Ok(guard) = SAMPLER.lock() else {
        return CoreHeat {
            cores: 0,
            columns: Vec::new(),
            now: Vec::new(),
        };
    };
    let Some(sampler) = guard.as_ref() else {
        return CoreHeat {
            cores: 0,
            columns: Vec::new(),
            now: Vec::new(),
        };
    };
    let cores = sampler.system.cpus().len();
    let mut sums = vec![(vec![0u32; cores], 0u32); COLUMNS];
    for (at, loads) in &sampler.cores {
        let age = at.elapsed().as_secs_f32();
        let column = COLUMNS as isize - 1 - (age / span) as isize;
        if column < 0 || loads.len() != cores {
            continue;
        }
        let (total, count) = &mut sums[column as usize];
        for (sum, load) in total.iter_mut().zip(loads) {
            *sum += u32::from(*load);
        }
        *count += 1;
    }
    CoreHeat {
        cores,
        columns: sums
            .into_iter()
            .map(|(total, count)| {
                (count > 0).then(|| total.iter().map(|sum| (sum / count) as u8).collect())
            })
            .collect(),
        now: sampler
            .cores
            .back()
            .map(|(_, loads)| loads.clone())
            .unwrap_or_default(),
    }
}

/// Windows: each process's GPU load (its busiest engine), 0–100. Empty elsewhere.
pub fn gpu_by_process() -> std::collections::HashMap<u32, f32> {
    SAMPLER
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(|sampler| sampler.gpu_by_process.clone()))
        .unwrap_or_default()
}
/// Bytes moved since the last refresh on one interface: the one the internet goes through,
/// or without the monitor the busiest real one. Summing them all counts a VPN's traffic twice
/// (the tunnel and the link under it) and counts loopback as network.
fn throughput(networks: &Networks) -> (u64, u64) {
    let list = networks.list();
    if let Some(data) = crate::net::primary_interface().and_then(|name| list.get(&name)) {
        return (data.received(), data.transmitted());
    }
    list.iter()
        .filter(|(name, _)| !virtual_interface(name))
        .map(|(_, data)| (data.received(), data.transmitted()))
        .max_by_key(|(down, up)| down + up)
        .unwrap_or((0, 0))
}

/// Loopback, tunnels and virtual switches, by name.
fn virtual_interface(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "lo", "utun", "tun", "tap", "wg", "ipsec", "ppp", "gif", "stf", "awdl", "llw", "bridge",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
        || [
            "loopback",
            "vpn",
            "wireguard",
            "tailscale",
            "vethernet",
            "virtual",
            "pseudo",
        ]
        .iter()
        .any(|word| name.contains(word))
}

/// The app and the processes working for it: children (WebView2 on Windows), and on macOS
/// the WebKit services launchd starts on its behalf, found by who is responsible for them.
fn app_usage(system: &mut System) -> AppUsage {
    use sysinfo::{Pid, ProcessesToUpdate};
    system.refresh_processes(ProcessesToUpdate::All, true);
    let own = Pid::from_u32(std::process::id());
    let processes = system.processes();
    let mut ours: Vec<Pid> = vec![own];
    // Descendants, breadth first.
    let mut index = 0;
    while index < ours.len() {
        let parent = ours[index];
        ours.extend(
            processes
                .iter()
                .filter(|(_, process)| process.parent() == Some(parent))
                .map(|(pid, _)| *pid),
        );
        index += 1;
    }
    // A dev build started from a terminal shares that terminal's responsibility, so its
    // WebKit services are credited to whatever the app's own responsible process is.
    #[cfg(target_os = "macos")]
    let responsible = responsible_for(own.as_u32()).unwrap_or(own.as_u32());
    #[cfg(target_os = "macos")]
    for (pid, _) in processes.iter().filter(|(_, process)| {
        process
            .name()
            .to_string_lossy()
            .starts_with("com.apple.WebKit")
    }) {
        if responsible_for(pid.as_u32()) == Some(responsible) && !ours.contains(pid) {
            ours.push(*pid);
        }
    }
    let mut usage = AppUsage {
        cpu: 0.0,
        memory: 0,
        processes: 0,
    };
    for pid in &ours {
        if let Some(process) = processes.get(pid) {
            usage.cpu += process.cpu_usage();
            usage.memory += footprint(pid.as_u32()).unwrap_or_else(|| process.memory());
            usage.processes += 1;
        }
    }
    usage
}

/// The process macOS holds responsible for `pid` (an app, for the XPC services it uses).
#[cfg(target_os = "macos")]
fn responsible_for(pid: u32) -> Option<u32> {
    use std::sync::OnceLock;
    type Responsible = unsafe extern "C" fn(libc::pid_t) -> libc::pid_t;
    static FUNCTION: OnceLock<Option<Responsible>> = OnceLock::new();
    let function = FUNCTION.get_or_init(|| {
        // SAFETY: dlsym on the default handle with a valid C string; the symbol, when
        // present, has this signature.
        unsafe {
            let symbol = libc::dlsym(
                libc::RTLD_DEFAULT,
                c"responsibility_get_pid_responsible_for_pid".as_ptr(),
            );
            (!symbol.is_null())
                .then(|| std::mem::transmute::<*mut libc::c_void, Responsible>(symbol))
        }
    });
    // SAFETY: the function takes any pid and returns -1 when it can't say.
    let responsible = unsafe { (*function)?(pid as libc::pid_t) };
    (responsible > 0).then_some(responsible as u32)
}

/// macOS: a process's resource usage (CPU time, disk bytes, footprint); none for processes
/// the user can't read.
#[cfg(target_os = "macos")]
pub fn rusage(pid: u32) -> Option<libc::rusage_info_v2> {
    let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    // SAFETY: `info` is a rusage_info_v2, which RUSAGE_INFO_V2 fills.
    let result = unsafe {
        libc::proc_pid_rusage(
            pid as libc::c_int,
            libc::RUSAGE_INFO_V2,
            (&mut info as *mut libc::rusage_info_v2).cast(),
        )
    };
    (result == 0).then_some(info)
}

/// macOS: the physical footprint, what Activity Monitor shows as Memory.
#[cfg(target_os = "macos")]
fn footprint(pid: u32) -> Option<u64> {
    rusage(pid).map(|info| info.ri_phys_footprint)
}

#[cfg(not(target_os = "macos"))]
fn footprint(_pid: u32) -> Option<u64> {
    None
}

fn empty() -> Stats {
    Stats {
        cpu: 0.0,
        memory_used: 0,
        memory_total: 0,
        disk_free: 0,
        disk_total: 0,
        net_down: 0,
        net_up: 0,
        gpu: None,
        cpu_temp: None,
        gpu_temp: None,
        network: None,
        app: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_fold_into_stretches_oldest_first() {
        // A 10-second window in five stretches; the newest reading is 0 seconds old.
        let readings = [(9.5, 1), (7.5, 7), (6.1, 3), (3.0, 4), (0.0, 5), (12.0, 99)];
        assert_eq!(buckets(readings, 10.0, 5, i32::max), [1, 7, 4, 5]);
        assert!(buckets(readings, 10.0, 0, i32::max).is_empty());
    }

    #[test]
    fn tunnels_and_loopback_are_not_the_line() {
        for name in [
            "lo0",
            "utun3",
            "Loopback Pseudo-Interface 1",
            "vEthernet (WSL)",
            "Tailscale",
        ] {
            assert!(virtual_interface(name), "{name}");
        }
        for name in ["Wi-Fi", "Ethernet", "en0", "en7"] {
            assert!(!virtual_interface(name), "{name}");
        }
    }

    #[test]
    fn samples_this_computer() {
        let first = sample();
        assert!(first.memory_total > 0);
        assert!(first.memory_used <= first.memory_total);
        assert!(first.disk_total > 0, "the system disk wasn't found");
        let second = sample();
        assert!((0.0..=100.0).contains(&second.cpu));
        // Test builds are debug builds, so the app's own use is counted too.
        let app = second.app.expect("the app's own use");
        assert!(app.processes >= 1 && app.memory > 0);
    }
}
