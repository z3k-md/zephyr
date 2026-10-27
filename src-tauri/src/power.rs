//! Power commands typed in the bar: shut down, restart, sleep, lock and sign out, now or on a
//! timer ("shutdown in 30m", "restart at 11pm"), and "cancel shutdown". Parsing is pure and
//! tested; running goes through [`commands_for`] so the exact system commands are tested too.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use chrono::{DateTime, Local, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Shutdown,
    Restart,
    Sleep,
    Lock,
    SignOut,
    /// Restart straight into the BIOS (UEFI firmware settings).
    Firmware,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Shutdown => "Shut down",
            Action::Restart => "Restart",
            Action::Sleep => "Sleep",
            Action::Lock => "Lock",
            Action::SignOut => "Sign out",
            Action::Firmware => "Restart to BIOS",
        }
    }

    /// As a noun: "the scheduled shutdown".
    pub fn noun(self) -> &'static str {
        match self {
            Action::Shutdown => "shutdown",
            Action::Restart => "restart",
            Action::Sleep => "sleep",
            Action::Lock => "lock",
            Action::SignOut => "sign-out",
            Action::Firmware => "restart to BIOS",
        }
    }

    /// Ending the session asks for a second Enter; sleep and lock don't lose anything.
    pub fn needs_confirm(self) -> bool {
        matches!(
            self,
            Action::Shutdown | Action::Restart | Action::SignOut | Action::Firmware
        )
    }

    /// Whether this computer can do it: restarting to the BIOS needs UEFI firmware, on
    /// Windows or Linux. Macs can't be sent to firmware from software.
    pub fn available(self) -> bool {
        self != Action::Firmware || firmware_restart_supported()
    }
}

#[cfg(windows)]
fn firmware_restart_supported() -> bool {
    use std::sync::OnceLock;
    use windows::Win32::System::SystemInformation::{
        FIRMWARE_TYPE, FirmwareTypeUefi, GetFirmwareType,
    };
    static UEFI: OnceLock<bool> = OnceLock::new();
    *UEFI.get_or_init(|| {
        let mut kind = FIRMWARE_TYPE::default();
        unsafe { GetFirmwareType(&mut kind) }.is_ok() && kind == FirmwareTypeUefi
    })
}

#[cfg(target_os = "linux")]
fn firmware_restart_supported() -> bool {
    std::path::Path::new("/sys/firmware/efi").exists()
}

#[cfg(not(any(windows, target_os = "linux")))]
fn firmware_restart_supported() -> bool {
    false
}

/// The words each action answers to, longest first so "shut down" wins over "shut".
const WORDS: &[(&str, Action)] = &[
    // Before "restart" and "reboot", which they start with.
    ("restart to bios", Action::Firmware),
    ("reboot to bios", Action::Firmware),
    ("restart to uefi", Action::Firmware),
    ("reboot to uefi", Action::Firmware),
    ("restart to firmware", Action::Firmware),
    ("reboot to firmware", Action::Firmware),
    ("boot to bios", Action::Firmware),
    ("firmware settings", Action::Firmware),
    ("firmware", Action::Firmware),
    ("bios", Action::Firmware),
    ("uefi", Action::Firmware),
    ("shut down", Action::Shutdown),
    ("shutdown", Action::Shutdown),
    ("power off", Action::Shutdown),
    ("poweroff", Action::Shutdown),
    ("turn off", Action::Shutdown),
    ("restart", Action::Restart),
    ("reboot", Action::Restart),
    ("sleep", Action::Sleep),
    ("suspend", Action::Sleep),
    ("lock screen", Action::Lock),
    ("lock", Action::Lock),
    ("sign out", Action::SignOut),
    ("signout", Action::SignOut),
    ("log out", Action::SignOut),
    ("logout", Action::SignOut),
    ("log off", Action::SignOut),
    ("logoff", Action::SignOut),
];

/// A scheduled action, kept in state.json so the bar can show and cancel it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub action: Action,
    /// Unix seconds.
    pub at: i64,
}

/// What the typed text would do, for the bar's Power row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub action: Action,
    /// "Shut down", "Restart at 11:00 PM", "Cancel the scheduled shutdown".
    pub label: String,
    /// When it would run, in unix seconds; none means now.
    pub at: Option<i64>,
    pub cancel: bool,
    pub confirm: bool,
    /// The whole text names the action (not just the start of a word), so Enter may take it.
    pub exact: bool,
}

pub fn preview(input: &str, now: DateTime<Local>) -> Option<Preview> {
    let text = input.trim().trim_start_matches('!').trim().to_lowercase();
    if text.len() < 3 {
        return None;
    }
    let (cancel, rest) = match ["cancel ", "abort ", "stop "]
        .iter()
        .find_map(|prefix| text.strip_prefix(prefix))
    {
        Some(rest) => (true, rest.trim()),
        None => (false, text.as_str()),
    };

    // Just the start of a command word ("shu"): offer it without taking Enter.
    if !cancel
        && !rest.contains(' ')
        && let Some((_, action)) = WORDS
            .iter()
            .filter(|(word, action)| word.starts_with(rest) && *word != rest && action.available())
            // The shortest word it starts: "rest" offers Restart, not Restart to BIOS.
            .min_by_key(|(word, _)| word.len())
            .filter(|_| !WORDS.iter().any(|(word, _)| *word == rest))
    {
        return Some(Preview {
            action: *action,
            label: action.label().into(),
            at: None,
            cancel: false,
            confirm: action.needs_confirm(),
            exact: false,
        });
    }

    let (word, action) = WORDS.iter().find(|(word, action)| {
        action.available() && (rest == *word || rest.starts_with(&format!("{word} ")))
    })?;
    let mut tail = rest[word.len()..].trim();
    for filler in ["the computer", "computer", "the pc", "pc", "now"] {
        if let Some(stripped) = tail.strip_prefix(filler) {
            tail = stripped.trim();
        }
    }

    if cancel {
        // Restart to BIOS is never scheduled, so there's nothing of it to cancel.
        if !tail.is_empty() || *action == Action::Firmware {
            return None;
        }
        return Some(Preview {
            action: *action,
            label: format!("Cancel the scheduled {}", action.noun()),
            at: None,
            cancel: true,
            confirm: false,
            exact: true,
        });
    }

    let at = if tail.is_empty() {
        None
    } else {
        Some(when(tail, now)?)
    };
    // Restart to BIOS is only ever wanted now.
    if *action == Action::Firmware && at.is_some() {
        return None;
    }
    let label = match at {
        None => action.label().to_string(),
        Some(at) => format!("{} {}", action.label(), describe(at, now)),
    };
    Some(Preview {
        action: *action,
        label,
        at,
        cancel: false,
        confirm: action.needs_confirm(),
        exact: true,
    })
}

/// "in 30m", "in 1h 15m", "after 90 minutes", "at 11pm", "at 23:30", "at noon".
fn when(text: &str, now: DateTime<Local>) -> Option<i64> {
    if let Some(rest) = text
        .strip_prefix("in ")
        .or_else(|| text.strip_prefix("after "))
    {
        let seconds = duration(rest.trim())?;
        return (seconds > 0).then(|| now.timestamp().saturating_add(seconds));
    }
    let rest = text.strip_prefix("at ")?.trim();
    let time = clock(rest)?;
    let today = now.date_naive().and_time(time);
    let mut at = Local.from_local_datetime(&today).earliest()?;
    if at <= now {
        at += chrono::Duration::days(1);
    }
    Some(at.timestamp())
}

/// "1h 15m", "1h15m", "90 minutes", "1.5 hours", "45s" → seconds.
fn duration(text: &str) -> Option<i64> {
    let mut total = 0f64;
    let mut number = String::new();
    let mut unit = String::new();
    let mut parts = 0;
    let flush = |number: &mut String, unit: &mut String, total: &mut f64| -> Option<()> {
        if number.is_empty() {
            return if unit.is_empty() { Some(()) } else { None };
        }
        let value: f64 = number.parse().ok()?;
        let scale = match unit.as_str() {
            "h" | "hr" | "hrs" | "hour" | "hours" => 3600.0,
            "m" | "min" | "mins" | "minute" | "minutes" | "" => 60.0,
            "s" | "sec" | "secs" | "second" | "seconds" => 1.0,
            _ => return None,
        };
        *total += value * scale;
        number.clear();
        unit.clear();
        Some(())
    };
    for character in text.chars() {
        if character.is_ascii_digit() || character == '.' {
            if !unit.is_empty() {
                flush(&mut number, &mut unit, &mut total)?;
                parts += 1;
            }
            number.push(character);
        } else if character.is_alphabetic() {
            if number.is_empty() {
                return None;
            }
            unit.push(character);
        } else if character.is_whitespace() || character == ',' {
            if !unit.is_empty() {
                flush(&mut number, &mut unit, &mut total)?;
                parts += 1;
            }
        } else if character != '&' {
            return None;
        }
    }
    if !number.is_empty() {
        flush(&mut number, &mut unit, &mut total)?;
        parts += 1;
    }
    // Capped at the longest timer Windows takes, so a huge number can't overflow the time.
    (parts > 0 && total >= 1.0).then(|| total.min(MAX_DELAY as f64).round() as i64)
}

/// The longest delay `shutdown /t` accepts: ten years, in seconds.
const MAX_DELAY: i64 = 315_360_000;

/// "11pm", "11 pm", "11:30pm", "23:30", "7:15 am", "noon", "midnight".
fn clock(text: &str) -> Option<NaiveTime> {
    match text {
        "noon" => return NaiveTime::from_hms_opt(12, 0, 0),
        "midnight" => return NaiveTime::from_hms_opt(0, 0, 0),
        _ => {}
    }
    let compact = text.replace(' ', "");
    let (digits, meridiem) = if let Some(rest) = compact.strip_suffix("am") {
        (rest, Some(false))
    } else if let Some(rest) = compact.strip_suffix("pm") {
        (rest, Some(true))
    } else {
        (compact.as_str(), None)
    };
    let (hour, minute) = match digits.split_once(':') {
        Some((hour, minute)) => (hour.parse::<u32>().ok()?, minute.parse::<u32>().ok()?),
        None => (digits.parse::<u32>().ok()?, 0),
    };
    let hour = match meridiem {
        Some(pm) => {
            if !(1..=12).contains(&hour) {
                return None;
            }
            (hour % 12) + if pm { 12 } else { 0 }
        }
        None => hour,
    };
    NaiveTime::from_hms_opt(hour, minute, 0)
}

/// "at 11:00 PM, in 2h 14m" or "in 30m" when it's soon.
pub fn describe(at: i64, now: DateTime<Local>) -> String {
    let left = at.saturating_sub(now.timestamp()).max(0);
    let span = span(left);
    if left < 3600 {
        return format!("in {span}");
    }
    let time = Local
        .timestamp_opt(at, 0)
        .single()
        .map(|time| time.format("%-I:%M %p").to_string())
        .unwrap_or_default();
    format!("at {time}, in {span}")
}

fn span(seconds: i64) -> String {
    let (hours, minutes) = (seconds / 3600, (seconds % 3600 + 59) / 60);
    match (hours, minutes) {
        (0, 0) => "less than a minute".into(),
        (0, minutes) => format!("{minutes}m"),
        (hours, 0) => format!("{hours}h"),
        (hours, minutes) => format!("{hours}h {minutes}m"),
    }
}

/// The system commands that run an action now. A scheduled one runs them when it's due,
/// from Zephyr's own timer: Windows' `shutdown /t N` would force apps closed without letting
/// them save (any `/t` above 0 implies `/f`).
pub fn commands_for(action: Action) -> Vec<Vec<String>> {
    let owned = |parts: &[&str]| {
        parts
            .iter()
            .map(|part| part.to_string())
            .collect::<Vec<_>>()
    };
    if cfg!(windows) {
        match action {
            Action::Shutdown => vec![owned(&["shutdown", "/s", "/t", "0"])],
            Action::Restart => vec![owned(&["shutdown", "/r", "/t", "0"])],
            // /fw needs an administrator, so Windows shows its UAC prompt; the restart starts
            // once it's allowed.
            Action::Firmware => vec![owned(&[ELEVATED, "shutdown", "/r", "/fw", "/t", "0"])],
            Action::SignOut => vec![owned(&["shutdown", "/l"])],
            // Slept in-process: rundll32 passes its own arguments to SetSuspendState, so a PC
            // with hibernation on hibernated instead (see `sleep_now`).
            Action::Sleep => vec![owned(&[SLEEP_IN_PROCESS])],
            Action::Lock => vec![owned(&["rundll32.exe", "user32.dll,LockWorkStation"])],
        }
    } else {
        let event = |name: &str| {
            owned(&[
                "osascript",
                "-e",
                &format!("tell application \"System Events\" to {name}"),
            ])
        };
        match action {
            Action::Shutdown => vec![event("shut down")],
            Action::Restart => vec![event("restart")],
            Action::SignOut => vec![event("log out")],
            Action::Sleep => vec![owned(&["pmset", "sleepnow"])],
            // Locked in-process, which needs no permission (see `lock_screen`).
            Action::Lock => vec![owned(&[LOCK_IN_PROCESS])],
            // Linux asks for permission itself (polkit); a Mac can't, so it's never offered.
            Action::Firmware if cfg!(target_os = "linux") => {
                vec![owned(&["systemctl", "reboot", "--firmware-setup"])]
            }
            Action::Firmware => Vec::new(),
        }
    }
}

/// Stands in for a command: lock the screen in-process.
const LOCK_IN_PROCESS: &str = "zephyr:lock";

/// Stands in for a command: put Windows to sleep in-process.
const SLEEP_IN_PROCESS: &str = "zephyr:sleep";

/// Before a command: run it as an administrator (Windows asks with its UAC prompt).
const ELEVATED: &str = "zephyr:elevated";

/// Runs a program as an administrator, hidden: Windows shows its UAC prompt first, and a
/// declined prompt is an error.
#[cfg(windows)]
fn run_elevated(program: &str, args: &[String]) -> Result<(), String> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    use windows::core::{HSTRING, w};

    let program = HSTRING::from(program);
    let args = HSTRING::from(args.join(" "));
    let result = unsafe { ShellExecuteW(None, w!("runas"), &program, &args, None, SW_HIDE) };
    // ShellExecute reports success as a value above 32.
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err("Windows didn't allow it (the administrator prompt was declined)".into())
    }
}

#[cfg(not(windows))]
fn run_elevated(program: &str, _args: &[String]) -> Result<(), String> {
    Err(format!(
        "Running {program} as an administrator is only for Windows"
    ))
}

/// The login framework's own "lock now", the one the menu bar's Lock Screen uses. Unlike
/// pressing ⌃⌘Q through System Events, it needs neither Automation nor Accessibility.
#[cfg(target_os = "macos")]
fn sac_lock() -> Option<extern "C" fn() -> i32> {
    const LOGIN: &std::ffi::CStr =
        c"/System/Library/PrivateFrameworks/login.framework/Versions/Current/login";
    // SAFETY: dlopen and dlsym take valid C strings; the symbol has this signature.
    unsafe {
        let handle = libc::dlopen(LOGIN.as_ptr(), libc::RTLD_LAZY);
        if handle.is_null() {
            return None;
        }
        let symbol = libc::dlsym(handle, c"SACLockScreenImmediate".as_ptr());
        (!symbol.is_null())
            .then(|| std::mem::transmute::<*mut libc::c_void, extern "C" fn() -> i32>(symbol))
    }
}

#[cfg(target_os = "macos")]
fn lock_screen() -> Result<(), String> {
    if let Some(lock) = sac_lock()
        && lock() == 0
    {
        return Ok(());
    }
    // Older or changed systems: the keyboard shortcut, which does need permissions.
    let status = std::process::Command::new("osascript")
        .args([
            "-e",
            "tell application \"System Events\" to keystroke \"q\" using {control down, command down}",
        ])
        .status()
        .map_err(|err| err.to_string())?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("osascript failed ({status})"))
}

#[cfg(not(target_os = "macos"))]
fn lock_screen() -> Result<(), String> {
    Err("Locking from Zephyr isn't supported here".into())
}

/// Sleeps now: not hibernate, whatever the hibernation setting, with wake events allowed.
#[cfg(windows)]
fn sleep_now() -> Result<(), String> {
    use windows::Win32::System::Power::SetSuspendState;
    // SAFETY: plain flags, no pointers.
    if unsafe { SetSuspendState(false, false, false) } {
        Ok(())
    } else {
        Err(windows::core::Error::from_thread().message())
    }
}

#[cfg(not(windows))]
fn sleep_now() -> Result<(), String> {
    Err("Sleeping from Zephyr this way is only for Windows".into())
}

/// While testing, `ZEPHYR_POWER_DRY_RUN=1` logs the commands instead of running them.
fn execute(commands: &[Vec<String>]) -> Result<(), String> {
    for command in commands {
        if std::env::var_os("ZEPHYR_POWER_DRY_RUN").is_some() {
            log::info!("power (dry run): {}", command.join(" "));
            continue;
        }
        if command[0] == LOCK_IN_PROCESS {
            lock_screen()?;
            continue;
        }
        if command[0] == SLEEP_IN_PROCESS {
            sleep_now()?;
            continue;
        }
        if command[0] == ELEVATED && command.len() > 1 {
            run_elevated(&command[1], &command[2..])?;
            continue;
        }
        let status = crate::util::hidden_command(&command[0])
            .args(&command[1..])
            .status()
            .map_err(|err| err.to_string())?;
        if !status.success() {
            return Err(format!("{} failed ({status})", command[0]));
        }
    }
    Ok(())
}

static TIMER: AtomicU64 = AtomicU64::new(0);

fn save(app: &AppHandle, schedule: Option<Schedule>) {
    let state = app.state::<crate::state::AppState>();
    let saved = state.update(|persisted| {
        persisted.power = schedule;
        Ok(())
    });
    match saved {
        Ok(snapshot) => {
            use tauri::Emitter;
            let _ = app.emit("state-changed", &snapshot);
        }
        Err(err) => log::error!("couldn't save the power schedule: {err}"),
    }
}

/// Runs an action now, or schedules it for `at` (unix seconds).
pub fn run(app: &AppHandle, action: Action, at: Option<i64>) -> Result<(), String> {
    let now = Local::now().timestamp();
    // `at` comes from the page; saturating so a wild value can't overflow.
    let delay = at.map(|at| at.saturating_sub(now).max(0));
    match delay {
        None | Some(0) => {
            cancel(app);
            log::info!("power: {} now", action.label());
            execute(&commands_for(action))
        }
        Some(_) if action == Action::Firmware => Err("Restart to BIOS only runs right away".into()),
        Some(delay) => {
            // One schedule at a time; a new one replaces the old.
            cancel(app);
            let at = now + delay;
            save(app, Some(Schedule { action, at }));
            arm(app, Schedule { action, at });
            log::info!("power: {} scheduled in {delay}s", action.label());
            Ok(())
        }
    }
}

/// Cancels the scheduled action, if there is one.
pub fn cancel(app: &AppHandle) {
    TIMER.fetch_add(1, Ordering::SeqCst);
    let pending = app
        .state::<crate::state::AppState>()
        .snapshot()
        .ok()
        .and_then(|snapshot| snapshot.power);
    if let Some(schedule) = pending {
        save(app, None);
        log::info!("power: cancelled the scheduled {}", schedule.action.label());
    }
}

/// Warns a few minutes ahead, then runs the action.
fn arm(app: &AppHandle, schedule: Schedule) {
    let generation = TIMER.load(Ordering::SeqCst);
    let app = app.clone();
    std::thread::spawn(move || {
        let warn_at = schedule.at - 5 * 60;
        let wait_until = |at: i64| {
            loop {
                if TIMER.load(Ordering::SeqCst) != generation {
                    return false;
                }
                let left = at - Local::now().timestamp();
                if left <= 0 {
                    return true;
                }
                std::thread::sleep(Duration::from_secs(left.min(20) as u64));
            }
        };
        if warn_at > Local::now().timestamp() && wait_until(warn_at) {
            crate::util::notify(
                &app,
                &format!("{} in 5 minutes", schedule.action.label()),
                "Open Zephyr to cancel it",
            );
        }
        if !wait_until(schedule.at) {
            return;
        }
        // Cleared first, so a shutdown that goes through doesn't leave it for the next start.
        save(&app, None);
        if let Err(err) = execute(&commands_for(schedule.action)) {
            log::error!("couldn't {}: {err}", schedule.action.label().to_lowercase());
        }
    });
}

/// On launch: picks a saved schedule back up, or drops one whose time has passed.
pub fn resume(app: &AppHandle) {
    let pending = app
        .state::<crate::state::AppState>()
        .snapshot()
        .ok()
        .and_then(|snapshot| snapshot.power);
    let Some(schedule) = pending else {
        return;
    };
    // Earlier versions scheduled restarts to the BIOS too, and set Windows' own timer for
    // shutdowns and restarts; a BIOS one is dropped, and an OS timer is cleared (nothing
    // pending is fine) since Zephyr's timer runs it now.
    if cfg!(windows) && matches!(schedule.action, Action::Shutdown | Action::Restart) {
        let _ = execute(&[vec!["shutdown".into(), "/a".into()]]);
    }
    if schedule.at <= Local::now().timestamp() || schedule.action == Action::Firmware {
        save(app, None);
        return;
    }
    arm(app, schedule);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(hour: u32, minute: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 10, 10, hour, minute, 0)
            .single()
            .unwrap()
    }

    #[test]
    fn restart_to_bios_answers_to_its_names_where_the_firmware_allows() {
        let now = at(20, 0);
        for text in [
            "bios",
            "reboot to bios",
            "Restart to BIOS",
            "uefi",
            "firmware",
        ] {
            let preview = preview(text, now);
            if Action::Firmware.available() {
                let preview = preview.unwrap_or_else(|| panic!("{text}"));
                assert_eq!(preview.action, Action::Firmware, "{text}");
                assert!(preview.confirm, "{text}");
            } else {
                assert!(preview.is_none(), "{text}");
            }
        }
        // Typing the start of "restart" still offers Restart.
        assert_eq!(preview("rest", now).unwrap().action, Action::Restart);
        assert_eq!(preview("restart", now).unwrap().action, Action::Restart);
        assert_eq!(
            preview("restart in 5m", now).unwrap().action,
            Action::Restart
        );
    }

    #[cfg(windows)]
    #[test]
    fn restart_to_bios_runs_shutdown_as_an_administrator() {
        assert_eq!(
            commands_for(Action::Firmware),
            [[ELEVATED, "shutdown", "/r", "/fw", "/t", "0"]]
        );
    }

    #[test]
    fn restart_to_bios_is_only_offered_for_now() {
        let now = at(20, 0);
        assert!(preview("bios in 5m", now).is_none());
        assert!(preview("restart to bios at 11pm", now).is_none());
        assert!(preview("cancel bios", now).is_none());
    }

    #[test]
    fn names_and_aliases_map_to_actions() {
        let now = at(20, 0);
        for (text, action) in [
            ("shutdown", Action::Shutdown),
            ("Shut down", Action::Shutdown),
            ("power off the computer", Action::Shutdown),
            ("reboot", Action::Restart),
            ("restart now", Action::Restart),
            ("lock", Action::Lock),
            ("sleep", Action::Sleep),
            ("log out", Action::SignOut),
        ] {
            let preview = preview(text, now).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(preview.action, action, "{text}");
            assert!(preview.exact && preview.at.is_none(), "{text}");
        }
        assert!(preview("shutdown the world", now).is_none());
        assert!(preview("locksmith near me", now).is_none());
        assert!(preview("restaurants", now).is_none());
    }

    #[test]
    fn the_start_of_a_word_is_offered_without_taking_enter() {
        let preview = preview("shu", at(20, 0)).unwrap();
        assert_eq!(preview.action, Action::Shutdown);
        assert!(!preview.exact);
        assert!(super::preview("sh", at(20, 0)).is_none());
    }

    #[test]
    fn schedules_parse_from_durations_and_clock_times() {
        let now = at(20, 46);
        let base = now.timestamp();
        let after = |text: &str| preview(text, now).and_then(|preview| preview.at);
        assert_eq!(after("shutdown in 30m"), Some(base + 1800));
        assert_eq!(after("restart in 1h 15m"), Some(base + 4500));
        assert_eq!(after("shutdown in 1h15m"), Some(base + 4500));
        assert_eq!(after("sleep in 90 minutes"), Some(base + 5400));
        assert_eq!(after("shutdown in 1.5 hours"), Some(base + 5400));
        assert_eq!(after("shutdown after 45s"), Some(base + 45));
        assert_eq!(after("shutdown in 10"), Some(base + 600));
        // 11pm today, 7am tomorrow.
        assert_eq!(after("shutdown at 11pm"), Some(at(23, 0).timestamp()));
        assert_eq!(after("shutdown at 23:30"), Some(at(23, 30).timestamp()));
        assert_eq!(
            after("restart at 7:15 am"),
            Some(at(7, 15).timestamp() + 86_400)
        );
        assert!(preview("shutdown in soon", now).is_none());
        assert!(preview("shutdown at 25", now).is_none());
        assert_eq!(
            preview("shutdown at 11pm", now).unwrap().label,
            "Shut down at 11:00 PM, in 2h 14m"
        );
        assert_eq!(
            preview("shutdown in 30m", now).unwrap().label,
            "Shut down in 30m"
        );
    }

    #[test]
    fn a_huge_delay_is_capped_instead_of_overflowing() {
        let now = at(20, 0);
        let preview = preview("shutdown in 99999999999999999999h", now).unwrap();
        assert_eq!(preview.at, Some(now.timestamp() + MAX_DELAY));
    }

    #[test]
    fn cancelling_is_its_own_preview() {
        let preview = preview("cancel shutdown", at(20, 0)).unwrap();
        assert!(preview.cancel && !preview.confirm);
        assert_eq!(preview.label, "Cancel the scheduled shutdown");
    }

    #[test]
    fn ending_the_session_needs_a_second_enter() {
        assert!(Action::Shutdown.needs_confirm());
        assert!(Action::SignOut.needs_confirm());
        assert!(!Action::Lock.needs_confirm());
        assert!(!Action::Sleep.needs_confirm());
    }

    #[cfg(windows)]
    #[test]
    fn windows_shutdowns_run_now_so_apps_can_save() {
        // `/t 0`: any longer and Windows force-closes apps without asking them to save.
        assert_eq!(
            commands_for(Action::Shutdown),
            vec![vec!["shutdown", "/s", "/t", "0"]]
        );
        assert_eq!(
            commands_for(Action::Restart),
            vec![vec!["shutdown", "/r", "/t", "0"]]
        );
        assert_eq!(commands_for(Action::SignOut), vec![vec!["shutdown", "/l"]]);
        // Sleep is in-process, so hibernation being on can't turn it into a hibernate.
        assert_eq!(
            commands_for(Action::Sleep),
            [[SLEEP_IN_PROCESS.to_string()]]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_permission_free_lock_is_there() {
        // Only looks the function up; calling it would lock this Mac.
        assert!(sac_lock().is_some());
        assert_eq!(commands_for(Action::Lock), [[LOCK_IN_PROCESS.to_string()]]);
    }
}
