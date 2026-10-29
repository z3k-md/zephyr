//! System Resources: the bar's live process view. Processes are grouped by app (on macOS by
//! the outermost `.app` in their path, on Windows by program name), sorted and filtered here,
//! and only the rows that can show are sent to the page, about once a second while it is
//! open. The process table is dropped half a minute after the view closes.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use serde::Serialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind, Users};

use crate::stats::{self, Point, Stats};

/// Processes whose force quit would take the system down with them.
#[cfg(target_os = "macos")]
const PROTECTED: &[&str] = &["kernel_task", "launchd", "WindowServer", "loginwindow"];
#[cfg(windows)]
const PROTECTED: &[&str] = &[
    "system", "csrss", "wininit", "winlogon", "lsass", "smss", "services", "dwm",
];
#[cfg(not(any(target_os = "macos", windows)))]
const PROTECTED: &[&str] = &["init", "systemd"];

/// Apps that are relaunched rather than quit for good.
const RELAUNCHED: &[&str] = &["Finder", "Dock", "explorer"];

/// One app (or program) and the processes that make it up.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    /// Stable across ticks, so the selection follows the app, not its position.
    pub key: String,
    pub name: String,
    /// For a command-line program: its command and folder ("bun dev · ~/code/zephyr").
    pub subtitle: String,
    /// `app:<bundle path>` for an app, else a glyph.
    pub icon: String,
    pub pids: Vec<u32>,
    /// Percent of one core (macOS) or of the machine (Windows); none when unreadable.
    pub cpu: Option<f32>,
    pub memory: Option<u64>,
    /// Bytes read and written per second.
    pub disk: Option<u64>,
    /// Windows: the busiest GPU engine any of its processes uses, 0–100.
    pub gpu: Option<f32>,
    pub user: String,
    /// The app bundle or program, for Reveal and Copy Path.
    pub path: String,
    pub command: String,
    /// Force quit is refused (the system needs it).
    pub protected: bool,
    /// Finder, Dock, Explorer: relaunched instead of quit.
    pub relaunch: bool,
    /// Its CPU and memory can't be read without administrator rights.
    pub locked: bool,
    /// An app with a window the user works in (Windows: a window in Alt+Tab; macOS: a Dock
    /// app). These list first, as Apps, ahead of the background processes.
    pub windowed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub totals: Stats,
    pub cores: usize,
    pub history: Vec<Point>,
    pub rows: Vec<Row>,
    /// Every process counted, shown or not.
    pub processes: usize,
    /// Seconds since the computer started.
    pub uptime: u64,
    /// None on a computer without one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery: Option<Battery>,
}

struct Table {
    system: System,
    users: Users,
    /// Processes whose path, command, folder and owner were read. Those never change, and
    /// for other users' processes the reads fail every time, so each is asked once.
    described: HashSet<Pid>,
    /// What each process is, worked out once when it's first seen.
    identities: HashMap<u32, Identity>,
    /// macOS: each process's CPU time and disk bytes at the last tick.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    counters: HashMap<u32, (u64, u64)>,
    /// Each row's CPU and memory, a reading a tick while the view is open, for the detail
    /// pane's graph.
    histories: HashMap<String, VecDeque<(Instant, f32, u64)>>,
    last: Instant,
    used: Instant,
}

/// Which row a process belongs to, and the details that row shows.
struct Identity {
    key: String,
    name: String,
    icon: String,
    path: String,
    command: String,
    subtitle: String,
    user: String,
}

impl Identity {
    fn of(process: &sysinfo::Process, users: &Users) -> Option<Identity> {
        let name = process.name().to_string_lossy().into_owned();
        if name.is_empty() {
            return None;
        }
        let user = process
            .user_id()
            .and_then(|id| users.get_user_by_id(id))
            .map(|user| user.name().to_string())
            .unwrap_or_default();
        let command: Vec<String> = process
            .cmd()
            .iter()
            .map(|part| part.to_string_lossy().into_owned())
            .collect();
        if let Some((bundle, stem)) = process.exe().and_then(app_bundle) {
            return Some(Identity {
                key: format!("app:{bundle}"),
                name: stem,
                icon: format!("app:{bundle}"),
                path: bundle,
                command: command.join(" "),
                subtitle: String::new(),
                user,
            });
        }
        let label = if cfg!(windows) {
            name.trim_end_matches(".exe").to_string()
        } else {
            name.clone()
        };
        let subtitle = if command.len() > 1 {
            let args = command
                .iter()
                .skip(1)
                .take(3)
                .map(|part| part.rsplit('/').next().unwrap_or(part))
                .collect::<Vec<_>>()
                .join(" ");
            let folder = process
                .cwd()
                .map(|cwd| home_short(&cwd.to_string_lossy()))
                .filter(|cwd| cwd != "/")
                .map(|cwd| format!(" · {cwd}"))
                .unwrap_or_default();
            format!("{label} {args}{folder}")
        } else {
            String::new()
        };
        Some(Identity {
            key: format!("proc:{}", name.to_lowercase()),
            name: label,
            icon: "glyph:terminal".to_string(),
            path: process
                .exe()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            command: command.join(" "),
            subtitle,
            user,
        })
    }

    /// An empty row for this process's group, unread until a process in it is measured.
    fn row(&self) -> Row {
        Row {
            key: self.key.clone(),
            name: self.name.clone(),
            subtitle: self.subtitle.clone(),
            icon: self.icon.clone(),
            pids: Vec::new(),
            cpu: None,
            memory: None,
            disk: None,
            gpu: None,
            user: self.user.clone(),
            path: self.path.clone(),
            // Left out of the list; Show info reads it (`detail`).
            command: String::new(),
            protected: PROTECTED
                .iter()
                .any(|protected| protected.eq_ignore_ascii_case(&self.name)),
            relaunch: RELAUNCHED
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&self.name)),
            locked: true,
            windowed: false,
        }
    }
}

/// One process's share of the last tick; none when it can't be read.
struct Usage {
    cpu: f32,
    memory: u64,
    disk: u64,
}

static TABLE: Mutex<Option<Table>> = Mutex::new(None);

/// Drops the process table once the view has been closed for half a minute.
pub fn idle() {
    if let Ok(mut table) = TABLE.lock()
        && table
            .as_ref()
            .is_some_and(|table| table.used.elapsed().as_secs() >= 30)
    {
        *table = None;
    }
}

pub(crate) fn app_bundle(exe: &Path) -> Option<(String, String)> {
    // The outermost .app, so helpers inside an app's bundle count toward the app.
    let mut path = std::path::PathBuf::new();
    for part in exe.components() {
        path.push(part);
        let name = part.as_os_str().to_string_lossy();
        if let Some(stem) = name.strip_suffix(".app") {
            return Some((path.to_string_lossy().into_owned(), stem.to_string()));
        }
    }
    None
}

fn home_short(path: &str) -> String {
    let home =
        std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_default();
    let home = home.trim_end_matches(['/', '\\']);
    match path.strip_prefix(home) {
        Some(rest) if !home.is_empty() && (rest.is_empty() || rest.starts_with(['/', '\\'])) => {
            format!("~{rest}")
        }
        _ => path.to_string(),
    }
}

/// The current tick: totals, the minute of history, and up to `limit` rows.
pub fn snapshot(sort: &str, filter: &str, limit: usize) -> Snapshot {
    let totals = stats::totals();
    let mut guard = TABLE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let table = guard.get_or_insert_with(|| Table {
        system: System::new(),
        users: Users::new_with_refreshed_list(),
        described: HashSet::new(),
        identities: HashMap::new(),
        counters: HashMap::new(),
        histories: HashMap::new(),
        last: Instant::now(),
        used: Instant::now(),
    });
    let elapsed = table.last.elapsed().as_secs_f64().max(0.2);
    let usage = measure(table, elapsed);
    table.last = Instant::now();
    table.used = Instant::now();
    let mut groups: HashMap<&str, Row> = HashMap::new();
    for (pid, identity) in &table.identities {
        let measured = usage.get(pid);
        let row = groups
            .entry(identity.key.as_str())
            .or_insert_with(|| identity.row());
        row.pids.push(*pid);
        // macOS keeps other users' processes to themselves without administrator rights.
        if let Some(measured) = measured {
            row.locked = false;
            row.cpu = Some(row.cpu.unwrap_or(0.0) + measured.cpu);
            row.memory = Some(row.memory.unwrap_or(0) + measured.memory);
            row.disk = Some(row.disk.unwrap_or(0) + measured.disk);
        }
    }
    let processes = table.identities.len();
    let mut rows: Vec<Row> = groups.into_values().collect();
    let gpu = stats::gpu_by_process();
    for row in &mut rows {
        let busiest = row.pids.iter().filter_map(|pid| gpu.get(pid)).copied();
        row.gpu = (!gpu.is_empty()).then(|| busiest.fold(0.0, f32::max));
    }
    remember(table, &rows);
    drop(guard);

    let filter = filter.trim().to_lowercase();
    rows.retain(|row| {
        filter.is_empty()
            || row.name.to_lowercase().contains(&filter)
            || row.path.to_lowercase().contains(&filter)
            || row.pids.iter().any(|pid| pid.to_string() == filter)
    });
    for row in &mut rows {
        // Only a lone command-line program says what it's running and where.
        if row.pids.len() > 1 {
            row.subtitle.clear();
        }
        row.pids.sort_unstable();
    }
    mark_windowed(&mut rows);
    sort_rows(&mut rows, sort);
    rows.truncate(limit);
    Snapshot {
        totals,
        cores: stats::cores(),
        history: stats::history_buckets(120),
        rows,
        processes,
        uptime: System::uptime(),
        battery: battery(),
    }
}

/// Reads only the details new processes need, and forgets the ones that ended.
fn describe(table: &mut Table, changed: &[Pid]) {
    if changed.is_empty() {
        let alive = table.system.processes();
        table.described.retain(|pid| alive.contains_key(pid));
        table
            .identities
            .retain(|pid, _| alive.contains_key(&Pid::from_u32(*pid)));
        return;
    }
    table.system.refresh_processes_specifics(
        ProcessesToUpdate::Some(changed),
        true,
        ProcessRefreshKind::nothing()
            .with_exe(UpdateKind::Always)
            .with_cmd(UpdateKind::Always)
            .with_cwd(UpdateKind::Always)
            .with_user(UpdateKind::Always),
    );
    let alive = table.system.processes();
    table.described.retain(|pid| alive.contains_key(pid));
    table
        .identities
        .retain(|pid, _| alive.contains_key(&Pid::from_u32(*pid)));
    for pid in changed {
        if let Some(process) = alive.get(pid) {
            table.described.insert(*pid);
            if let Some(identity) = Identity::of(process, &table.users) {
                table.identities.insert(pid.as_u32(), identity);
            }
        }
    }
}

/// macOS: one resource-usage read per process (about 4 ms for 800), with memory as the
/// footprint Activity Monitor shows. Other users' processes can't be read and are left out.
#[cfg(target_os = "macos")]
fn measure(table: &mut Table, elapsed: f64) -> HashMap<u32, Usage> {
    use std::sync::OnceLock;
    static TIMEBASE: OnceLock<(u64, u64)> = OnceLock::new();
    // CPU times come in Mach ticks, which aren't nanoseconds on Apple silicon.
    #[allow(deprecated)]
    let &(numer, denom) = TIMEBASE.get_or_init(|| {
        let mut info = libc::mach_timebase_info { numer: 0, denom: 0 };
        // SAFETY: fills the struct it is given.
        let ok = unsafe { libc::mach_timebase_info(&mut info) } == 0;
        if ok && info.denom > 0 {
            (u64::from(info.numer), u64::from(info.denom))
        } else {
            (1, 1)
        }
    });
    // Sized from the count asked first, with room for processes started meanwhile; a full
    // buffer may have been cut short, so it's asked again with more.
    // SAFETY: without a buffer the call only counts.
    let mut room = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) }.max(0) as usize + 64;
    let mut pids = Vec::new();
    loop {
        pids.resize(room, 0 as libc::pid_t);
        // SAFETY: the buffer's size in bytes is passed with it.
        let count = unsafe {
            libc::proc_listallpids(
                pids.as_mut_ptr().cast(),
                (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int,
            )
        }
        .max(0) as usize;
        if count < room || room >= 1 << 20 {
            pids.truncate(count.min(room));
            break;
        }
        room *= 2;
    }

    let mut usage = HashMap::with_capacity(pids.len());
    let mut counters = HashMap::with_capacity(pids.len());
    for pid in &pids {
        let Some(info) = stats::rusage(*pid as u32) else {
            continue;
        };
        let cpu_ns = (info.ri_user_time + info.ri_system_time) * numer / denom;
        let disk_bytes = info.ri_diskio_bytesread + info.ri_diskio_byteswritten;
        let (cpu, disk) = match table.counters.get(&(*pid as u32)) {
            Some(&(cpu_before, disk_before)) => (
                (cpu_ns.saturating_sub(cpu_before) as f64 / (elapsed * 1e9) * 100.0) as f32,
                (disk_bytes.saturating_sub(disk_before) as f64 / elapsed) as u64,
            ),
            None => (0.0, 0),
        };
        counters.insert(*pid as u32, (cpu_ns, disk_bytes));
        usage.insert(
            *pid as u32,
            Usage {
                cpu,
                memory: info.ri_phys_footprint,
                disk,
            },
        );
    }
    let listed: HashSet<Pid> = pids.iter().map(|pid| Pid::from_u32(*pid as u32)).collect();
    let mut changed: Vec<Pid> = listed
        .iter()
        .filter(|pid| !table.described.contains(pid))
        .copied()
        .collect();
    changed.extend(table.described.iter().filter(|pid| !listed.contains(pid)));
    table.counters = counters;
    describe(table, &changed);
    usage
}

/// Elsewhere sysinfo reads it; Windows reports CPU per core, so it's divided down to a share
/// of the machine as Task Manager shows it.
#[cfg(not(target_os = "macos"))]
fn measure(table: &mut Table, elapsed: f64) -> HashMap<u32, Usage> {
    table.system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_disk_usage(),
    );
    let fresh: Vec<Pid> = table
        .system
        .processes()
        .keys()
        .filter(|pid| !table.described.contains(pid))
        .copied()
        .collect();
    describe(table, &fresh);
    let cores = stats::cores() as f32;
    table
        .system
        .processes()
        .iter()
        .map(|(pid, process)| {
            let disk = process.disk_usage();
            let cpu = if cfg!(windows) {
                process.cpu_usage() / cores
            } else {
                process.cpu_usage()
            };
            (
                pid.as_u32(),
                Usage {
                    cpu,
                    memory: process.memory(),
                    disk: ((disk.read_bytes + disk.written_bytes) as f64 / elapsed) as u64,
                },
            )
        })
        .collect()
}

/// Marks the rows with a window the user works in.
#[cfg(windows)]
fn mark_windowed(rows: &mut [Row]) {
    use std::sync::Mutex;
    // The window list is otherwise only read as the bar is summoned; while Resources is open
    // it's read again every few seconds, so a window opened meanwhile moves its app up.
    static LISTED: Mutex<Option<Instant>> = Mutex::new(None);
    if let Ok(mut listed) = LISTED.lock()
        && listed.is_none_or(|at| at.elapsed() >= std::time::Duration::from_secs(5))
    {
        *listed = Some(Instant::now());
        crate::running::refresh();
    }
    let programs: HashSet<String> = crate::running::all()
        .into_iter()
        .map(|window| window.exe.to_lowercase())
        .collect();
    for row in rows {
        row.windowed = !row.path.is_empty() && programs.contains(&row.path.to_lowercase());
    }
}

#[cfg(target_os = "macos")]
fn mark_windowed(rows: &mut [Row]) {
    use objc2_app_kit::{NSApplicationActivationPolicy, NSWorkspace};
    // Dock apps ("regular" activation): what the user works in, not agents and helpers.
    let apps: HashSet<u32> = objc2::rc::autoreleasepool(|_| {
        NSWorkspace::sharedWorkspace()
            .runningApplications()
            .iter()
            .filter(|app| app.activationPolicy() == NSApplicationActivationPolicy::Regular)
            .map(|app| app.processIdentifier() as u32)
            .collect()
    });
    for row in rows {
        row.windowed = row.pids.iter().any(|pid| apps.contains(pid));
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn mark_windowed(_rows: &mut [Row]) {}

/// Apps with windows first, then the rest; each by `cpu`, `memory`, `disk` (most first) or
/// `name`, and rows that can't be read go last.
fn sort_rows(rows: &mut [Row], sort: &str) {
    rows.sort_by(|left, right| {
        right
            .windowed
            .cmp(&left.windowed)
            .then_with(|| by_value(left, right, sort))
    });
}

fn by_value(left: &Row, right: &Row, sort: &str) -> std::cmp::Ordering {
    let value = |row: &Row| -> Option<f64> {
        match sort {
            "memory" => row.memory.map(|value| value as f64),
            "disk" => row.disk.map(|value| value as f64),
            "gpu" => row.gpu.map(f64::from),
            "name" => None,
            _ => row.cpu.map(f64::from),
        }
    };
    if sort == "name" {
        return left.name.to_lowercase().cmp(&right.name.to_lowercase());
    }
    match (value(left), value(right)) {
        (Some(a), Some(b)) => b.total_cmp(&a).then_with(|| left.name.cmp(&right.name)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left.name.cmp(&right.name),
    }
}

/// How far back a row's detail graph goes, like the tiles.
const HISTORY: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Adds this tick's reading to each row's history, and forgets rows that are gone.
fn remember(table: &mut Table, rows: &[Row]) {
    let now = Instant::now();
    let seen: HashSet<&str> = rows.iter().map(|row| row.key.as_str()).collect();
    table.histories.retain(|key, _| seen.contains(key.as_str()));
    for row in rows {
        let (Some(cpu), Some(memory)) = (row.cpu, row.memory) else {
            continue;
        };
        let history = table.histories.entry(row.key.clone()).or_default();
        while history
            .front()
            .is_some_and(|(at, _, _)| at.elapsed() > HISTORY)
        {
            history.pop_front();
        }
        history.push_back((now, cpu, memory));
    }
}

/// One row in depth, for Show info: its processes' facts and its CPU and memory since the
/// view opened.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    pub key: String,
    /// The first (oldest) process's ID, its parent's name and when it started, in seconds
    /// since 1970.
    pub pid: u32,
    pub parent: String,
    pub started: u64,
    /// Bytes read and written since each process started, summed.
    pub read: u64,
    pub written: u64,
    /// What it was started with; the list leaves this out to keep each tick small.
    pub command: String,
    /// Oldest first, in up to 120 stretches (each the highest in it): seconds ago, CPU %,
    /// memory bytes.
    pub history: Vec<(f32, f32, u64)>,
}

pub fn detail(key: &str, pids: &[u32]) -> Option<Detail> {
    let guard = TABLE.lock().ok()?;
    let table = guard.as_ref()?;
    let processes: Vec<&sysinfo::Process> = pids
        .iter()
        .filter_map(|pid| table.system.process(Pid::from_u32(*pid)))
        .collect();
    let first = processes.iter().min_by_key(|process| process.start_time());
    let parent = first
        .and_then(|process| process.parent())
        .and_then(|parent| table.system.process(parent))
        .map(|parent| parent.name().to_string_lossy().into_owned())
        .unwrap_or_default();
    Some(Detail {
        key: key.to_string(),
        pid: first.map_or(pids.first().copied().unwrap_or(0), |process| {
            process.pid().as_u32()
        }),
        parent,
        started: first.map_or(0, |process| process.start_time()),
        read: processes
            .iter()
            .map(|process| process.disk_usage().total_read_bytes)
            .sum(),
        written: processes
            .iter()
            .map(|process| process.disk_usage().total_written_bytes)
            .sum(),
        command: pids
            .iter()
            .find_map(|pid| table.identities.get(pid))
            .map(|identity| identity.command.clone())
            .unwrap_or_default(),
        history: table
            .histories
            .get(key)
            .map(|history| buckets(history, 120))
            .unwrap_or_default(),
    })
}

/// A row's history in `count` stretches over the window, each the highest CPU and memory in
/// it at its newest reading's age; empty stretches are left out.
fn buckets(history: &VecDeque<(Instant, f32, u64)>, count: usize) -> Vec<(f32, f32, u64)> {
    let readings = history.iter().map(|(at, cpu, memory)| {
        let age = at.elapsed().as_secs_f32();
        (age, (age, *cpu, *memory))
    });
    stats::buckets(
        readings,
        HISTORY.as_secs_f32(),
        count,
        |(kept_age, kept_cpu, kept_memory), (age, cpu, memory)| {
            (
                kept_age.min(age),
                kept_cpu.max(cpu),
                kept_memory.max(memory),
            )
        },
    )
}

/// The battery, read at most every 30 seconds.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    /// 0–100.
    pub percent: f32,
    /// `charging`, `discharging`, `full` or `unknown`.
    pub state: String,
    /// Seconds until empty (discharging) or full (charging), where the system estimates it.
    pub seconds_left: Option<u64>,
}

fn battery() -> Option<Battery> {
    static CACHE: Mutex<Option<(Instant, Option<Battery>)>> = Mutex::new(None);
    let mut cache = CACHE.lock().ok()?;
    if let Some((at, battery)) = cache.as_ref()
        && at.elapsed().as_secs() < 30
    {
        return battery.clone();
    }
    let read = read_battery();
    *cache = Some((Instant::now(), read.clone()));
    read
}

fn read_battery() -> Option<Battery> {
    use starship_battery::State;
    use starship_battery::units::ratio::percent;
    use starship_battery::units::time::second;
    let manager = starship_battery::Manager::new().ok()?;
    let battery = manager.batteries().ok()?.flatten().next()?;
    let (state, left) = match battery.state() {
        State::Charging => ("charging", battery.time_to_full()),
        State::Discharging => ("discharging", battery.time_to_empty()),
        State::Full => ("full", None),
        _ => ("unknown", None),
    };
    Some(Battery {
        percent: battery.state_of_charge().get::<percent>(),
        state: state.into(),
        seconds_left: left.map(|time| time.get::<second>() as u64),
    })
}

/// Where the memory is, for the Memory drill-in. Windows: what's in use, the modified and
/// standby lists (cache Windows can hand back) and what's free, plus the commit charge.
/// macOS: see its `memory_breakdown`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryBreakdown {
    pub total: u64,
    pub in_use: u64,
    pub modified: u64,
    pub standby: u64,
    pub free: u64,
    pub committed: u64,
    pub commit_limit: u64,
    pub paged_pool: u64,
    pub nonpaged_pool: u64,
    /// macOS: memory the system keeps resident, and what the compressor holds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wired: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compressed: Option<u64>,
}

#[cfg(windows)]
pub fn memory_breakdown() -> Option<MemoryBreakdown> {
    use windows::Win32::System::Performance::{
        PDH_FMT_COUNTERVALUE, PDH_FMT_LARGE, PDH_HCOUNTER, PDH_HQUERY, PdhAddEnglishCounterW,
        PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue, PdhOpenQueryW,
    };
    use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
    use windows::core::{PCWSTR, w};

    let mut info = PERFORMANCE_INFORMATION {
        cb: std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32,
        ..Default::default()
    };
    // SAFETY: a structure of the size it says it is.
    unsafe { GetPerformanceInfo(&mut info, info.cb) }.ok()?;
    let page = info.PageSize as u64;

    // The page lists, as Resource Monitor reads them.
    let paths: [PCWSTR; 5] = [
        w!("\\Memory\\Modified Page List Bytes"),
        w!("\\Memory\\Standby Cache Core Bytes"),
        w!("\\Memory\\Standby Cache Normal Priority Bytes"),
        w!("\\Memory\\Standby Cache Reserve Bytes"),
        w!("\\Memory\\Free & Zero Page List Bytes"),
    ];
    let mut values = [0u64; 5];
    // SAFETY: a query opened, read once and closed here, with counters added to it.
    unsafe {
        let mut query = PDH_HQUERY::default();
        if PdhOpenQueryW(None, 0, &mut query) == 0 {
            let mut counters = [PDH_HCOUNTER::default(); 5];
            for (path, counter) in paths.iter().zip(counters.iter_mut()) {
                let _ = PdhAddEnglishCounterW(query, *path, 0, counter);
            }
            if PdhCollectQueryData(query) == 0 {
                for (counter, value) in counters.iter().zip(values.iter_mut()) {
                    let mut out = PDH_FMT_COUNTERVALUE::default();
                    if PdhGetFormattedCounterValue(*counter, PDH_FMT_LARGE, None, &mut out) == 0 {
                        *value = out.Anonymous.largeValue.max(0) as u64;
                    }
                }
            }
            PdhCloseQuery(query);
        }
    }
    let total = info.PhysicalTotal as u64 * page;
    let available = info.PhysicalAvailable as u64 * page;
    Some(MemoryBreakdown {
        total,
        in_use: total.saturating_sub(available),
        modified: values[0],
        standby: values[1] + values[2] + values[3],
        free: values[4],
        committed: info.CommitTotal as u64 * page,
        commit_limit: info.CommitLimit as u64 * page,
        paged_pool: info.KernelPaged as u64 * page,
        nonpaged_pool: info.KernelNonpaged as u64 * page,
        wired: None,
        compressed: None,
    })
}

/// macOS: Activity Monitor's split. App memory goes in `in_use`, cached files (which macOS
/// hands back the moment an app needs them) in `standby`, alongside wired and compressed
/// memory; the Windows-only lists and commit charge stay zero.
#[cfg(target_os = "macos")]
pub fn memory_breakdown() -> Option<MemoryBreakdown> {
    // SAFETY: plain counters, for which all zeros is a valid value.
    let mut info: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    // SAFETY: the statistics structure and its size in integers, which the call fills in.
    #[allow(deprecated)]
    let status = unsafe {
        libc::host_statistics64(
            libc::mach_host_self(),
            libc::HOST_VM_INFO64,
            (&mut info as *mut libc::vm_statistics64).cast(),
            &mut count,
        )
    };
    if status != libc::KERN_SUCCESS {
        return None;
    }
    // SAFETY: no pointers.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(0) as u64;
    let mut total = 0u64;
    let mut size = std::mem::size_of::<u64>();
    // SAFETY: a u64 and its size, which hw.memsize fills.
    let read = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&mut total as *mut u64).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if read != 0 || total == 0 || page == 0 {
        return None;
    }
    Some(mac_breakdown(&info, page, total))
}

#[cfg(target_os = "macos")]
fn mac_breakdown(info: &libc::vm_statistics64, page: u64, total: u64) -> MemoryBreakdown {
    let pages = |count: u32| u64::from(count) * page;
    let app = pages(info.internal_page_count).saturating_sub(pages(info.purgeable_count));
    let wired = pages(info.wire_count);
    let compressed = pages(info.compressor_page_count);
    let cached = pages(info.external_page_count) + pages(info.purgeable_count);
    MemoryBreakdown {
        total,
        in_use: app,
        standby: cached,
        // Whatever the rest leave, so the parts add up to what's installed.
        free: total.saturating_sub(app + wired + compressed + cached),
        wired: Some(wired),
        compressed: Some(compressed),
        ..MemoryBreakdown::default()
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn memory_breakdown() -> Option<MemoryBreakdown> {
    None
}

/// Quits (or with `force`, kills) every process in a group. Apps are asked to quit the way
/// the Dock asks, so they can offer to save; other processes get SIGTERM (or SIGKILL).
pub fn quit(name: &str, pids: &[u32], force: bool) -> Result<(), String> {
    // The page's list can be a tick old, and an ended process's pid can be handed to another
    // since, so only the processes that are still `name` are acted on. That also makes the
    // protected check below hold for what actually runs, not just what the page called it.
    let pids = still_named(name, pids);
    if pids.is_empty() {
        return Err(format!("{name} isn't running any more"));
    }
    if force
        && PROTECTED
            .iter()
            .any(|protected| protected.eq_ignore_ascii_case(name))
    {
        return Err(format!(
            "{name} keeps the system running, so it can't be force quit"
        ));
    }
    let targets = polite_targets(&pids, force);
    let mut failed = 0;
    for pid in &targets {
        if !quit_one(*pid, force, name) {
            failed += 1;
        }
    }
    if failed == targets.len() && !targets.is_empty() {
        return Err(format!(
            "Couldn't quit {name}; it may need administrator rights"
        ));
    }
    Ok(())
}

/// Finder and the Dock come back on their own once quit; Explorer is started again.
pub fn relaunch(name: &str, pids: &[u32]) -> Result<(), String> {
    if !RELAUNCHED
        .iter()
        .any(|known| known.eq_ignore_ascii_case(name))
    {
        return Err(format!("{name} isn't one Zephyr relaunches"));
    }
    let pids = still_named(name, pids);
    #[cfg(unix)]
    for pid in &pids {
        // SAFETY: kill only signals the pid; launchd starts Finder and the Dock again.
        unsafe { libc::kill(*pid as libc::pid_t, libc::SIGTERM) };
    }
    #[cfg(windows)]
    {
        for pid in &pids {
            quit_one(*pid, true, name);
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        crate::util::spawn_detached(&mut std::process::Command::new("explorer.exe"))
            .map_err(|err| format!("Couldn't start Explorer again: {err}"))?;
    }
    Ok(())
}

/// The processes among `pids` that, read afresh, still belong to the row called `name`.
fn still_named(name: &str, pids: &[u32]) -> Vec<u32> {
    let asked: Vec<Pid> = pids.iter().map(|pid| Pid::from_u32(*pid)).collect();
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&asked),
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
    );
    let users = Users::new();
    pids.iter()
        .copied()
        .filter(|pid| {
            system
                .process(Pid::from_u32(*pid))
                .and_then(|process| Identity::of(process, &users))
                .is_some_and(|identity| identity.name.eq_ignore_ascii_case(name))
        })
        .collect()
}

/// macOS: a polite quit asks only the app itself, which closes its own helpers once it has
/// had the chance to save; signalling the helpers too would crash them out from under it.
#[cfg(target_os = "macos")]
fn polite_targets(pids: &[u32], force: bool) -> Vec<u32> {
    use objc2_app_kit::NSRunningApplication;
    if force {
        return pids.to_vec();
    }
    // Called off the main thread, which has no pool to drain what AppKit autoreleases.
    let apps: Vec<u32> = objc2::rc::autoreleasepool(|_| {
        pids.iter()
            .copied()
            .filter(|pid| {
                NSRunningApplication::runningApplicationWithProcessIdentifier(*pid as i32).is_some()
            })
            .collect()
    });
    if apps.is_empty() { pids.to_vec() } else { apps }
}

#[cfg(not(target_os = "macos"))]
fn polite_targets(pids: &[u32], _force: bool) -> Vec<u32> {
    pids.to_vec()
}

#[cfg(target_os = "macos")]
fn quit_one(pid: u32, force: bool, _name: &str) -> bool {
    use objc2_app_kit::NSRunningApplication;
    let asked = objc2::rc::autoreleasepool(|_| {
        NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32).map(|app| {
            if force {
                app.forceTerminate()
            } else {
                app.terminate()
            }
        })
    });
    if let Some(done) = asked {
        return done;
    }
    // SAFETY: kill only signals the pid; no memory is involved.
    unsafe {
        libc::kill(
            pid as libc::pid_t,
            if force { libc::SIGKILL } else { libc::SIGTERM },
        ) == 0
    }
}

/// Windows: a forced quit opens the process first and checks, through that handle, that it
/// still runs the program `name` names, so a pid reused in the meantime is left alone.
#[cfg(windows)]
fn quit_one(pid: u32, force: bool, name: &str) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, TerminateProcess,
    };
    if !force {
        // Asking politely: close the program's windows, as their close buttons would.
        return crate::running::close_process_windows(pid);
    }
    // SAFETY: the handle is checked and closed; TerminateProcess only ends that process.
    unsafe {
        let Ok(handle) = OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        ) else {
            return false;
        };
        let same = crate::util::image_path(handle).is_some_and(|path| {
            Path::new(&path)
                .file_stem()
                .is_some_and(|stem| stem.to_string_lossy().eq_ignore_ascii_case(name))
        });
        let done = same && TerminateProcess(handle, 1).is_ok();
        let _ = CloseHandle(handle);
        done
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn quit_one(pid: u32, force: bool, _name: &str) -> bool {
    // SAFETY: kill only signals the pid.
    unsafe {
        libc::kill(
            pid as libc::pid_t,
            if force { libc::SIGKILL } else { libc::SIGTERM },
        ) == 0
    }
}

/// Brings an app's windows to the front. False when it has none to show.
#[cfg(target_os = "macos")]
pub fn switch_to(pids: &[u32]) -> bool {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    objc2::rc::autoreleasepool(|_| {
        pids.iter().any(|pid| {
            NSRunningApplication::runningApplicationWithProcessIdentifier(*pid as i32).is_some_and(
                |app| app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows),
            )
        })
    })
}

#[cfg(windows)]
pub fn switch_to(pids: &[u32]) -> bool {
    pids.iter().any(|pid| crate::running::focus_process(*pid))
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn switch_to(_pids: &[u32]) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, cpu: Option<f32>, windowed: bool) -> Row {
        Row {
            key: name.into(),
            name: name.into(),
            subtitle: String::new(),
            icon: String::new(),
            pids: Vec::new(),
            cpu,
            memory: None,
            disk: None,
            gpu: None,
            user: String::new(),
            path: String::new(),
            command: String::new(),
            protected: false,
            relaunch: false,
            locked: false,
            windowed,
        }
    }

    #[test]
    fn apps_with_windows_sort_ahead_of_background_processes() {
        let mut rows = vec![
            row("indexer", Some(40.0), false),
            row("editor", Some(2.0), true),
            row("locked", None, true),
            row("browser", Some(9.0), true),
            row("updater", Some(1.0), false),
        ];
        sort_rows(&mut rows, "cpu");
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["browser", "editor", "locked", "indexer", "updater"]);
        sort_rows(&mut rows, "name");
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["browser", "editor", "locked", "indexer", "updater"]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn helpers_count_toward_their_app() {
        let helper = Path::new(
            "/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper",
        );
        assert_eq!(
            app_bundle(helper),
            Some((
                "/Applications/Google Chrome.app".to_string(),
                "Google Chrome".to_string()
            ))
        );
        assert_eq!(app_bundle(Path::new("/usr/bin/top")), None);
    }

    #[test]
    fn only_processes_still_under_the_name_are_acted_on() {
        let own = std::process::id();
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[Pid::from_u32(own)]),
            true,
            ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
        );
        let name = Identity::of(system.process(Pid::from_u32(own)).unwrap(), &Users::new())
            .unwrap()
            .name;
        assert_eq!(still_named(&name.to_uppercase(), &[own]), [own]);
        // A pid now running something else, or nothing at all.
        assert!(still_named("lsass", &[own]).is_empty());
        assert!(still_named(&name, &[u32::MAX - 1]).is_empty());
        assert_eq!(
            quit("lsass", &[own], true),
            Err("lsass isn't running any more".to_string())
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_memory_splits_as_activity_monitor_does() {
        // SAFETY: plain counters.
        let mut info: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
        info.internal_page_count = 50;
        info.purgeable_count = 10;
        info.wire_count = 20;
        info.compressor_page_count = 5;
        info.external_page_count = 15;
        let parts = mac_breakdown(&info, 100, 20_000);
        assert_eq!(parts.in_use, 4_000);
        assert_eq!(parts.wired, Some(2_000));
        assert_eq!(parts.compressed, Some(500));
        assert_eq!(parts.standby, 2_500);
        assert_eq!(parts.free, 11_000);
        assert!(memory_breakdown().is_some_and(|parts| parts.total > 0));
    }
    #[test]
    fn a_tick_lists_this_process_grouped_and_sorted() {
        let first = snapshot("cpu", "", 500);
        assert!(first.processes > 1);
        let second = snapshot("memory", "", 500);
        let memories: Vec<u64> = second.rows.iter().filter_map(|row| row.memory).collect();
        assert!(memories.windows(2).all(|pair| pair[0] >= pair[1]));
        let own = std::process::id();
        let filtered = snapshot("cpu", &own.to_string(), 10);
        assert!(filtered.rows.iter().any(|row| row.pids.contains(&own)));
    }
}
