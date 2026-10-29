//! Small helpers that several modules share.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since the Unix epoch.
pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// A command for a console program. On Windows it gets no console window, which would
/// otherwise flash up for every run.
pub fn hidden_command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Starts a program that carries on by itself (an app, Explorer, Terminal) without waiting for
/// it. On macOS a thread waits instead, so the finished child is reaped rather than left a
/// zombie until Zephyr quits; Windows has no zombies, so nothing waits there.
pub fn spawn_detached(command: &mut Command) -> std::io::Result<()> {
    let child = command.spawn()?;
    #[cfg(unix)]
    std::thread::spawn(move || {
        let mut child = child;
        let _ = child.wait();
    });
    #[cfg(not(unix))]
    drop(child);
    Ok(())
}

/// Writes through a temp file beside `path` (making its folder if needed), so a crash never
/// leaves half a file behind.
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    write_synced(Path::new(&tmp), data)?;
    std::fs::rename(&tmp, path)
}

/// Writes `data` and waits for it to reach the disk, so a rename after it never swaps in a
/// file a power cut left empty.
pub fn write_synced(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(path)?;
    file.write_all(data)?;
    file.sync_all()
}

/// Windows: the full path of the program process `pid` runs, when it can be read.
#[cfg(windows)]
pub fn process_path(pid: u32) -> Option<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: a handle opened, read and closed here.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let path = image_path(process);
        let _ = CloseHandle(process);
        path
    }
}

/// Windows: the full path of the program behind an open process handle, which needs at least
/// `PROCESS_QUERY_LIMITED_INFORMATION`.
#[cfg(windows)]
pub fn image_path(process: windows::Win32::Foundation::HANDLE) -> Option<String> {
    use windows::Win32::System::Threading::{PROCESS_NAME_WIN32, QueryFullProcessImageNameW};
    use windows::core::PWSTR;
    let mut path = [0u16; 1024];
    let mut length = path.len() as u32;
    // SAFETY: a buffer and its length in characters, which the call updates.
    unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut length,
        )
        .ok()?;
    }
    Some(String::from_utf16_lossy(&path[..length as usize]))
}

/// Shows a system notification; a failure is only logged.
pub fn notify(app: &tauri::AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    if let Err(err) = app.notification().builder().title(title).body(body).show() {
        log::error!("couldn't show a notification: {err}");
    }
}

/// The one HTTP client: suggestions, icons, weather, the public address, the speed test and
/// sync share its connection pool and TLS setup. Each request sets its own time limit.
pub fn http() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("Zephyr")
            .connect_timeout(std::time::Duration::from_secs(8))
            .build()
            .unwrap_or_else(|err| {
                // Only a TLS backend that won't start gets here; a bare client still works.
                log::error!("couldn't set up the HTTP client, using a plain one: {err}");
                reqwest::Client::new()
            })
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_detached_program_starts_or_says_why_not() {
        #[cfg(windows)]
        let mut quick = super::hidden_command("cmd");
        #[cfg(windows)]
        quick.args(["/C", "exit"]);
        #[cfg(unix)]
        let mut quick = std::process::Command::new("true");
        assert!(super::spawn_detached(&mut quick).is_ok());
        let mut missing = std::process::Command::new("no-such-program-zz");
        assert!(super::spawn_detached(&mut missing).is_err());
    }
}
