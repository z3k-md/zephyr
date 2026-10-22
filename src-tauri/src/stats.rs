//! Live system stats for the bar's footer and System Resources: CPU, memory, free space on
//! the system disk and network throughput. A background thread reads them once a second
//! (about a millisecond each: totals only, never the process list), so Resources always
//! opens with the last minute.

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use sysinfo::{Disks, Networks, System};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    /// Across all cores, 0–100.
    pub cpu: f32,
    pub memory_used: u64,
    pub memory_total: u64,
    pub disk_free: u64,
    pub disk_total: u64,
    /// Bytes per second since the last sample, across every interface.
    pub net_down: u64,
    pub net_up: u64,
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
    last: Instant,
    /// The latest reading, which the footer and Resources show.
    latest: Option<Stats>,
    /// The last minute of totals, with when each was read.
    history: std::collections::VecDeque<(Instant, Point)>,
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
    pub down: u64,
    pub up: u64,
}

/// How far back the sparklines go, and how often the thread reads.
const WINDOW: Duration = Duration::from_secs(60);
const EVERY: Duration = Duration::from_secs(1);

/// The last minute of totals, oldest first.
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

/// Starts the background reading, once a second for the life of the app.
pub fn start() {
    std::thread::spawn(|| {
        loop {
            if let Ok(mut guard) = SAMPLER.lock() {
                record(guard.get_or_insert_with(Sampler::new));
            }
            std::thread::sleep(EVERY);
        }
    });
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
        stats.app = Some(app_usage(&mut sampler.system));
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
            last: Instant::now(),
            latest: None,
            history: std::collections::VecDeque::new(),
        }
    }
}

/// Reads the totals and adds them to the history. CPU usage and network rates compare
/// against the previous reading, so the very first reads them as zero.
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
    let (down, up) = sampler
        .networks
        .list()
        .values()
        .fold((0u64, 0u64), |(down, up), data| {
            (down + data.received(), up + data.transmitted())
        });
    let stats = Stats {
        cpu: sampler.system.global_cpu_usage(),
        memory_used: sampler.system.used_memory(),
        memory_total: sampler.system.total_memory(),
        disk_free,
        disk_total,
        net_down: (down as f64 / elapsed) as u64,
        net_up: (up as f64 / elapsed) as u64,
        app: None,
    };
    while sampler
        .history
        .front()
        .is_some_and(|(at, _)| at.elapsed() > WINDOW)
    {
        sampler.history.pop_front();
    }
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
            down: stats.net_down,
            up: stats.net_up,
        },
    ));
    sampler.latest = Some(stats.clone());
    stats
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
        app: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
