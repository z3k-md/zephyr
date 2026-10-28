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
    /// Dev builds: the app's own use and when it was read, which walks every process, so
    /// the footer's twice-a-second readings reuse it for a second.
    app: Option<(Instant, AppUsage)>,
}

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

/// How often the thread reads while the bar shows, so the footer's meters move smoothly.
const EVERY_SHOWN: Duration = Duration::from_millis(500);
/// The shortest gap between reads: CPU use over less means little.
const AT_LEAST: Duration = Duration::from_millis(250);

static BAR_VISIBLE: AtomicBool = AtomicBool::new(false);
static THREAD: OnceLock<std::thread::Thread> = OnceLock::new();

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
            let reading = SAMPLER
                .lock()
                .ok()
                .map(|mut guard| record(guard.get_or_insert_with(Sampler::new)));
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
    let Ok(mut guard) = SAMPLER.lock() else {
        return empty();
    };
    let sampler = guard.get_or_insert_with(Sampler::new);
    match &sampler.latest {
        Some(stats) => stats.clone(),
        None => record(sampler),
    }
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
            app: None,
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

/// Reads the totals and adds them to the history, a point a second. CPU usage and network
/// rates compare against the previous reading, so the very first reads them as zero.
fn record(sampler: &mut Sampler) -> Stats {
    sampler.system.refresh_cpu_usage();
    sampler.system.refresh_memory();
    sampler.networks.refresh(true);
    let elapsed = sampler.last.elapsed().as_secs_f64().max(0.001);
    sampler.last = Instant::now();

    let mount = system_mount();
    let disk = sampler
        .disks
        .list_mut()
        .iter_mut()
        .find(|disk| disk.mount_point() == Path::new(&mount));
    let (disk_free, disk_total) = match disk {
        Some(disk) => {
            disk.refresh();
            (disk.available_space(), disk.total_space())
        }
        None => (0, 0),
    };
    let (down, up) = throughput(&sampler.networks);
    let stats = Stats {
        cpu: sampler.system.global_cpu_usage(),
        memory_used: sampler.system.used_memory(),
        memory_total: sampler.system.total_memory(),
        disk_free,
        disk_total,
        net_down: (down as f64 / elapsed) as u64,
        net_up: (up as f64 / elapsed) as u64,
        gpu: sampler.gpu.as_mut().and_then(crate::gpu::Reader::read),
        network: crate::net::alert(),
        app: None,
    };
    while sampler
        .history
        .front()
        .is_some_and(|(at, _)| at.elapsed() > WINDOW)
    {
        sampler.history.pop_front();
    }
    // Reads twice a second while the bar shows; the history keeps one a second. The slack
    // allows for the thread waking a little early.
    let due = sampler
        .history
        .back()
        .is_none_or(|(at, _)| at.elapsed() + Duration::from_millis(50) >= EVERY);
    if due {
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
        network: None,
        app: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
