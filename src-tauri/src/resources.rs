//! System Resources: the bar's live process view. Processes are grouped by app (on macOS by
//! the outermost `.app` in their path, on Windows by program name), sorted and filtered here,
//! and only the rows that can show are sent to the page, about once a second while it is
//! open. The process table is dropped half a minute after the view closes.

use std::collections::{HashMap, HashSet};
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
            user: self.user.clone(),
            path: self.path.clone(),
            command: self.command.clone(),
            protected: PROTECTED
                .iter()
                .any(|protected| protected.eq_ignore_ascii_case(&self.name)),
            relaunch: RELAUNCHED
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&self.name)),
            locked: true,
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

fn app_bundle(exe: &Path) -> Option<(String, String)> {
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
    sort_rows(&mut rows, sort);
    rows.truncate(limit);
    Snapshot {
        totals,
        cores: stats::cores(),
        history: stats::history(),
        rows,
        processes,
        uptime: System::uptime(),
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
    let mut pids = vec![0 as libc::pid_t; 8192];
    // SAFETY: the buffer's size in bytes is passed with it.
    let count = unsafe {
        libc::proc_listallpids(
            pids.as_mut_ptr().cast(),
            (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int,
        )
    };
    pids.truncate(count.max(0) as usize);

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

/// By `cpu`, `memory`, `disk` (most first) or `name`; rows that can't be read go last.
fn sort_rows(rows: &mut [Row], sort: &str) {
    rows.sort_by(|left, right| {
        let value = |row: &Row| -> Option<f64> {
            match sort {
                "memory" => row.memory.map(|value| value as f64),
                "disk" => row.disk.map(|value| value as f64),
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
    });
}

/// Quits (or with `force`, kills) every process in a group. Apps are asked to quit the way
/// the Dock asks, so they can offer to save; other processes get SIGTERM (or SIGKILL).
pub fn quit(name: &str, pids: &[u32], force: bool) -> Result<(), String> {
    if force
        && PROTECTED
            .iter()
            .any(|protected| protected.eq_ignore_ascii_case(name))
    {
        return Err(format!(
            "{name} keeps the system running, so it can't be force quit"
        ));
    }
    let targets = polite_targets(pids, force);
    let mut failed = 0;
    for pid in &targets {
        if !quit_one(*pid, force) {
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
    #[cfg(unix)]
    for pid in pids {
        // SAFETY: kill only signals the pid; launchd starts Finder and the Dock again.
        unsafe { libc::kill(*pid as libc::pid_t, libc::SIGTERM) };
    }
    #[cfg(windows)]
    {
        for pid in pids {
            quit_one(*pid, true);
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        std::process::Command::new("explorer.exe")
            .spawn()
            .map_err(|err| format!("Couldn't start Explorer again: {err}"))?;
    }
    Ok(())
}

/// macOS: a polite quit asks only the app itself, which closes its own helpers once it has
/// had the chance to save; signalling the helpers too would crash them out from under it.
#[cfg(target_os = "macos")]
fn polite_targets(pids: &[u32], force: bool) -> Vec<u32> {
    use objc2_app_kit::NSRunningApplication;
    if force {
        return pids.to_vec();
    }
    let apps: Vec<u32> = pids
        .iter()
        .copied()
        .filter(|pid| {
            NSRunningApplication::runningApplicationWithProcessIdentifier(*pid as i32).is_some()
        })
        .collect();
    if apps.is_empty() { pids.to_vec() } else { apps }
}

#[cfg(not(target_os = "macos"))]
fn polite_targets(pids: &[u32], _force: bool) -> Vec<u32> {
    pids.to_vec()
}

#[cfg(target_os = "macos")]
fn quit_one(pid: u32, force: bool) -> bool {
    use objc2_app_kit::NSRunningApplication;
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32) {
        return if force {
            app.forceTerminate()
        } else {
            app.terminate()
        };
    }
    // SAFETY: kill only signals the pid; no memory is involved.
    unsafe {
        libc::kill(
            pid as libc::pid_t,
            if force { libc::SIGKILL } else { libc::SIGTERM },
        ) == 0
    }
}

#[cfg(windows)]
fn quit_one(pid: u32, force: bool) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};
    if !force {
        // Asking politely: close the program's windows, as their close buttons would.
        return crate::running::close_process_windows(pid);
    }
    // SAFETY: the handle is checked and closed; TerminateProcess only ends that process.
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, pid) else {
            return false;
        };
        let done = TerminateProcess(handle, 1).is_ok();
        let _ = CloseHandle(handle);
        done
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn quit_one(pid: u32, force: bool) -> bool {
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
    pids.iter().any(|pid| {
        NSRunningApplication::runningApplicationWithProcessIdentifier(*pid as i32).is_some_and(
            |app| app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows),
        )
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
