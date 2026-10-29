//! Windows: the CPU's temperature, opt-in from Settings › Resources. Windows only lets a
//! driver read it, so this uses PawnIO (a signed, open-source driver) and its signed AMD and
//! Intel modules. PawnIO only answers administrators, so a small helper does the reading: a
//! copy of this program, started with `--cpu-temp-helper` from a Task Scheduler task that runs
//! with the highest privileges.
//!
//! Zephyr installs per user, in a folder the user (and so anything they run) can write. A task
//! that ran that copy elevated would hand administrator rights to whatever replaced it, so
//! turning this on asks Windows once (one UAC prompt) to install PawnIO when it's missing, to
//! copy this program into a folder under Program Files only administrators can change, and to
//! have that copy register the task for itself. The helper refuses to run when it, or the
//! PawnIO library it loads, could be changed without administrator rights. The copy stays as
//! it was set up; Zephyr's own updates don't touch it. Setting it up records the copy's
//! `PROTOCOL` beside it, and a Zephyr that expects a newer one leaves the old copy stopped
//! and offers to set it up again (one prompt). A task left by an older Zephyr, which ran
//! Zephyr's own copy, moves itself over the next time it runs. Uninstalling Zephyr removes the
//! task and the copy too, after one prompt (installer/hooks.nsh).
//!
//! The helper puts each reading in a small shared memory block anyone signed in can read,
//! reads once a second while Zephyr asks for it and not at all otherwise, and quits when
//! Zephyr does or when it's turned off. Turning it off removes the task and the copy.

/// The Task Scheduler task that starts the helper. At the library's top level: creating it in
/// a folder of its own failed with "path not found" from schtasks /XML.
#[cfg(windows)]
const TASK: &str = "Zephyr CPU temperature";
/// The argument that makes this program the helper instead of the app.
pub const HELPER_ARG: &str = "--cpu-temp-helper";
/// What the helper and Zephyr agree on (the shared reading, the events, the task); raised
/// whenever a change needs the helper's copy set up again.
#[cfg(windows)]
const PROTOCOL: u32 = 1;

/// Tctl, as AMD's own tools show it: 0.125 °C steps in bits 21–31 of the SMN register, 49
/// lower when the range-select bit (19) is set.
#[cfg(any(windows, test))]
fn amd_tctl(value: u64) -> f32 {
    let mut celsius = ((value >> 21) & 0x7FF) as f32 / 8.0;
    if value & (1 << 19) != 0 {
        celsius -= 49.0;
    }
    celsius
}

/// Intel: the package's distance below its throttle point (bits 16–22 of
/// IA32_PACKAGE_THERM_STATUS), taken from that point (bits 16–23 of MSR_TEMPERATURE_TARGET).
#[cfg(any(windows, test))]
fn intel_package(status: u64, target: u64) -> f32 {
    let tj_max = ((target >> 16) & 0xFF) as f32;
    tj_max - ((status >> 16) & 0x7F) as f32
}

/// The last line a command printed, as text. cmd writes in the console's code page, and
/// winget in UTF-16, so both are tried.
#[cfg(any(windows, test))]
fn last_line(bytes: &[u8]) -> String {
    let text = if bytes.len() >= 2 && bytes[1] == 0 {
        String::from_utf16_lossy(
            &bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect::<Vec<_>>(),
        )
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or_default()
        .trim_start_matches("ERROR:")
        .trim()
        .to_string()
}

/// SYSTEM, administrators and TrustedInstaller: who may change what the helper runs.
#[cfg(any(windows, test))]
const TRUSTED: [&str; 3] = [
    "S-1-5-18",
    "S-1-5-32-544",
    "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464",
];

/// Whether one access-control entry on a file or folder the helper runs from lets anyone
/// outside `TRUSTED` change it. Entries that only pass down to what's made inside don't count;
/// deny entries are ignored, so this errs towards refusing.
#[cfg(any(windows, test))]
fn lets_others_write(ace_type: u8, ace_flags: u8, mask: u32, sid: &str) -> bool {
    const INHERIT_ONLY: u8 = 0x08;
    // Write or append data (add a file or folder), delete a child, delete, change the
    // permissions or the owner, and the generic all and write rights.
    const WRITE: u32 =
        0x2 | 0x4 | 0x40 | 0x1_0000 | 0x4_0000 | 0x8_0000 | 0x1000_0000 | 0x4000_0000;
    if ace_flags & INHERIT_ONLY != 0 {
        return false;
    }
    match ace_type {
        // Allowed, and allowed with a condition.
        0 | 9 => mask & WRITE != 0 && !TRUSTED.contains(&sid),
        // Denied, and its variants.
        1 | 6 | 10 | 12 => false,
        // Object-specific allows have no place on files; anything else is unknown.
        _ => true,
    }
}

/// The helper's task: at sign-in and whenever Zephyr starts it, as `user` with the highest
/// privileges, on battery too, for as long as it runs, one copy at a time. From XML, since
/// schtasks' own flags can't lift the defaults that would stop it: on battery, and after three
/// days of running.
#[cfg(any(windows, test))]
fn task_xml(user: &str, command: &str) -> String {
    let escape = |text: &str| {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Reads the CPU's temperature for Zephyr through PawnIO.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
      <Arguments>{arguments}</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        user = escape(user),
        command = escape(command),
        arguments = HELPER_ARG
    )
}

#[cfg(windows)]
mod imp {
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use windows::Win32::Foundation::{
        CloseHandle, ERROR_SUCCESS, HANDLE, HLOCAL, INVALID_HANDLE_VALUE, LocalFree,
        WAIT_ABANDONED, WAIT_OBJECT_0,
    };
    use windows::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        GetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT,
    };
    use windows::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, GetAce,
        OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
    };
    use windows::Win32::Storage::Packaging::Appx::{
        GetPackagePathByFullName, GetPackagesByPackageFamily,
    };
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
        LoadLibraryExW, SetDefaultDllDirectories,
    };
    use windows::Win32::System::Memory::{
        CreateFileMappingW, FILE_MAP_READ, FILE_MAP_WRITE, MapViewOfFile, OpenFileMappingW,
        PAGE_READWRITE, UnmapViewOfFile,
    };
    use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
    use windows::Win32::System::Threading::{
        CreateEventW, CreateMutexW, GetExitCodeProcess, INFINITE, OpenEventW, OpenMutexW,
        ReleaseMutex, SYNCHRONIZATION_SYNCHRONIZE, SetEvent, WaitForSingleObject,
    };
    use windows::Win32::UI::Shell::{
        FOLDERID_ProgramFiles, KNOWN_FOLDER_FLAG, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
        SHGetKnownFolderPath, ShellExecuteExW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    use windows::core::{HSTRING, PCSTR, PCWSTR, PWSTR, s};

    use super::{HELPER_ARG, PROTOCOL, TASK};

    /// The shared reading: °C, and when it was taken (ms since 1970).
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Reading {
        celsius: f32,
        taken_ms: u64,
    }

    const MAPPING: &str = r"Global\ZephyrCpuTemperature";
    /// Held by Zephyr for its whole life; the helper quits once it's gone.
    const ALIVE: &str = r"Local\ZephyrRunning";
    /// Set by Zephyr when the reading is turned off.
    const STOP: &str = r"Local\ZephyrCpuTemperatureStop";
    /// Set by Zephyr whenever it looks at the reading, so the helper reads only while it's used.
    const WANTED: &str = r"Local\ZephyrCpuTemperatureWanted";
    /// AMD's SMN registers sit behind PCI config space other tools use too; this named mutex is
    /// how they take turns.
    const PCI_MUTEX: &str = r"Global\Access_PCI";

    /// Where the helper's copy lives, under Program Files, and what it's called. Named apart
    /// from Zephyr so it can be stopped by name.
    const HELPER_FOLDER: &str = "Zephyr CPU Temperature";
    const HELPER_EXE: &str = "zephyr-cpu-temperature.exe";
    /// With `HELPER_ARG`: registers the task for the copy this runs from.
    const INSTALL_ARG: &str = "--install";
    /// With `HELPER_ARG`, from the uninstaller: removes the task and the copy, as turning the
    /// reading off does.
    const REMOVE_ARG: &str = "--remove";
    /// The copy's `PROTOCOL`, written by the copy as it registers its task.
    const PROTOCOL_FILE: &str = "protocol";
    /// What setting it up printed, beside the copy, where only administrators can write.
    const SETUP_LOG: &str = "setup.log";

    /// How often the helper reads while it's used, how long it keeps on after the last look,
    /// and how often it checks on Zephyr meanwhile.
    const SAMPLE_EVERY: Duration = Duration::from_secs(1);
    const STAY_ACTIVE: Duration = Duration::from_secs(30);
    const IDLE_CHECK: Duration = Duration::from_secs(5);

    const AMD_MODULE: &[u8] = include_bytes!("../resources/pawnio/AMDFamily17.bin");
    const INTEL_MODULE: &[u8] = include_bytes!("../resources/pawnio/IntelMSR.bin");

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |time| time.as_millis() as u64)
    }

    /// Program Files, from the system's own record rather than an environment variable the
    /// user could set.
    fn program_files() -> Option<PathBuf> {
        // SAFETY: the returned string is copied, then freed as the call requires.
        unsafe {
            let path =
                SHGetKnownFolderPath(&FOLDERID_ProgramFiles, KNOWN_FOLDER_FLAG(0), None).ok()?;
            let text = path.to_string().ok();
            CoTaskMemFree(Some(path.0 as *const _));
            text.map(PathBuf::from)
        }
    }

    fn helper_dir() -> Option<PathBuf> {
        Some(program_files()?.join(HELPER_FOLDER))
    }

    /// A program in System32, by its full path, so a same-named one elsewhere on the path
    /// can't stand in for it when run as administrator.
    fn system_tool(name: &str) -> String {
        let mut buffer = [0u16; 260];
        // SAFETY: the buffer's length is passed with it.
        let length = unsafe { GetSystemDirectoryW(Some(&mut buffer)) } as usize;
        let dir = String::from_utf16_lossy(&buffer[..length.min(buffer.len())]);
        let dir = if dir.is_empty() {
            r"C:\Windows\System32"
        } else {
            &dir
        };
        format!(r"{dir}\{name}")
    }

    fn same_path(a: &Path, b: &Path) -> bool {
        a.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
    }

    // ---- The app's side ----

    /// The event `read` sets for the helper, made at startup.
    static WANTED_EVENT: OnceLock<usize> = OnceLock::new();

    /// The helper's latest reading, when it's running and read in the last few seconds. Asks
    /// the helper to keep reading, so after a while without asking the first answer is none.
    pub fn read() -> Option<f32> {
        // SAFETY: the event made at startup, never closed; a read-only view of the helper's
        // block, its size, released before return.
        unsafe {
            if let Some(&wanted) = WANTED_EVENT.get() {
                let _ = SetEvent(HANDLE(wanted as *mut _));
            }
            let mapping = OpenFileMappingW(FILE_MAP_READ.0, false, &HSTRING::from(MAPPING)).ok()?;
            let view = MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, size_of::<Reading>());
            let reading = (!view.Value.is_null())
                .then(|| std::ptr::read_volatile(view.Value.cast::<Reading>()));
            let _ = UnmapViewOfFile(view);
            let _ = CloseHandle(mapping);
            let reading = reading?;
            (now_ms().saturating_sub(reading.taken_ms) < 5000 && reading.celsius > 0.0)
                .then_some(reading.celsius)
        }
    }

    /// Whether the helper's copy is set up for this Zephyr's `PROTOCOL`. A copy from before
    /// the protocol was recorded counts as outdated.
    fn helper_current() -> bool {
        helper_dir()
            .and_then(|dir| std::fs::read_to_string(dir.join(PROTOCOL_FILE)).ok())
            .and_then(|text| text.trim().parse::<u32>().ok())
            .is_some_and(|protocol| protocol >= PROTOCOL)
    }

    /// Whether the reading needs setting up again: its copy is missing, or outdated.
    pub fn needs_setup() -> bool {
        !helper_current()
    }

    /// Marks Zephyr as running for the helper, and starts the helper when the reading is on.
    /// Running the task needs no prompt: it was set up with permission when turned on. An
    /// outdated copy isn't started (Settings offers to set it up again); with no copy at all
    /// the task may be an older Zephyr's, which moves itself over as it runs.
    pub fn start(enabled: bool) {
        // SAFETY: named objects owned for the life of the process; their handles are never
        // closed.
        unsafe {
            let _ = CreateMutexW(None, true, &HSTRING::from(ALIVE));
            if let Ok(stop) = CreateEventW(None, true, false, &HSTRING::from(STOP)) {
                // Kept open so the helper can find it; reset in case an old one lingered.
                let _ = windows::Win32::System::Threading::ResetEvent(stop);
            }
            if let Ok(wanted) = CreateEventW(None, false, false, &HSTRING::from(WANTED)) {
                let _ = WANTED_EVENT.set(wanted.0 as usize);
            }
        }
        if enabled {
            std::thread::spawn(|| {
                let outdated = helper_dir().is_some_and(|dir| dir.exists()) && !helper_current();
                if outdated {
                    log::warn!("cpu temperature: the helper is outdated; set it up again");
                    return;
                }
                let _ = crate::util::hidden_command("schtasks")
                    .args(["/Run", "/TN", TASK])
                    .output();
            });
        }
    }

    /// Whether PawnIO's library is installed.
    pub fn pawnio_installed() -> bool {
        pawnio_library().is_some_and(|path| path.exists())
    }

    fn pawnio_library() -> Option<PathBuf> {
        Some(program_files()?.join(r"PawnIO\PawnIOLib.dll"))
    }

    /// Turns the reading on: one administrator prompt installs PawnIO when it's missing, puts
    /// the helper's copy in place and has it register its task, then the helper starts.
    pub fn enable() -> Result<(), String> {
        // A stop left over from turning it off would end the new helper at once.
        // SAFETY: the event this process made at startup, opened by name and cleared.
        unsafe {
            if let Ok(stop) = OpenEventW(
                windows::Win32::System::Threading::EVENT_MODIFY_STATE,
                false,
                &HSTRING::from(STOP),
            ) {
                let _ = windows::Win32::System::Threading::ResetEvent(stop);
                let _ = CloseHandle(stop);
            }
        }
        let exe = std::env::current_exe().map_err(|err| err.to_string())?;
        let dir = helper_dir().ok_or("Couldn't find Program Files.")?;
        let install = if pawnio_installed() {
            None
        } else {
            let winget = winget().ok_or(
                "PawnIO isn't installed, and winget wasn't found to install it. Install PawnIO \
                 from pawnio.eu, then turn this on again.",
            )?;
            Some(format!(
                "\"{}\" install --id namazso.PawnIO -e --silent --accept-package-agreements \
                 --accept-source-agreements",
                winget.display()
            ))
        };
        elevated(
            &setup_command(&exe, &dir, install.as_deref()),
            Some(&dir.join(SETUP_LOG)),
            "Setting it up failed",
        )?;
        if !pawnio_installed() {
            return Err("PawnIO didn't install. Try again, or install it from pawnio.eu.".into());
        }
        Ok(())
    }

    /// winget, from its package's folder under Program Files\WindowsApps, which only the
    /// system can change. Never looked up by name: as administrator, a `winget` planted
    /// earlier on the user's PATH would run instead.
    fn winget() -> Option<PathBuf> {
        let family = HSTRING::from("Microsoft.DesktopAppInstaller_8wekyb3d8bbwe");
        let (mut count, mut length) = (0u32, 0u32);
        // SAFETY: the first calls only size the buffers; the second fill buffers of those sizes.
        unsafe {
            let _ = GetPackagesByPackageFamily(&family, &mut count, None, &mut length, None);
            if count == 0 || length == 0 {
                return None;
            }
            let mut names = vec![PWSTR::null(); count as usize];
            let mut buffer = vec![0u16; length as usize];
            if GetPackagesByPackageFamily(
                &family,
                &mut count,
                Some(names.as_mut_ptr()),
                &mut length,
                Some(PWSTR(buffer.as_mut_ptr())),
            ) != ERROR_SUCCESS
            {
                return None;
            }
            names.iter().take(count as usize).find_map(|name| {
                let mut size = 0u32;
                let _ = GetPackagePathByFullName(PCWSTR(name.0), &mut size, None);
                let mut path = vec![0u16; size as usize];
                if size == 0
                    || GetPackagePathByFullName(
                        PCWSTR(name.0),
                        &mut size,
                        Some(PWSTR(path.as_mut_ptr())),
                    ) != ERROR_SUCCESS
                {
                    return None;
                }
                let end = path
                    .iter()
                    .position(|unit| *unit == 0)
                    .unwrap_or(path.len());
                let exe = PathBuf::from(String::from_utf16_lossy(&path[..end])).join("winget.exe");
                (exe.is_file() && locked_down(&exe)).then_some(exe)
            })
        }
    }

    /// What runs as administrator to set the helper up: stop a helper already running, make
    /// its folder afresh with only SYSTEM and administrators able to change it (and owning
    /// it), run `install` when given, copy `exe` in, and have the copy register its task. The
    /// task's definition is written there by the copy, so nothing outside the folder can swap
    /// it. What's printed goes to the folder's log.
    fn setup_command(exe: &Path, dir: &Path, install: Option<&str>) -> String {
        let taskkill = system_tool("taskkill.exe");
        let ping = system_tool("PING.EXE");
        let icacls = system_tool("icacls.exe");
        let dir = dir.display();
        let helper = format!(r"{dir}\{HELPER_EXE}");
        let install = install
            .map(|step| format!("{step} && "))
            .unwrap_or_default();
        // Wrapped in parentheses so cmd keeps the quotes; the icacls grants are quoted so
        // their parentheses don't end it.
        format!(
            "(\"{taskkill}\" /F /IM {HELPER_EXE} >nul 2>&1 && \"{ping}\" -n 2 127.0.0.1 >nul \
             & (if exist \"{dir}\" rmdir /S /Q \"{dir}\") \
             & mkdir \"{dir}\" && \"{icacls}\" \"{dir}\" /inheritance:r /grant:r \
             \"*S-1-5-18:(OI)(CI)F\" \"*S-1-5-32-544:(OI)(CI)F\" \"*S-1-5-32-545:(OI)(CI)RX\" >nul \
             && ({install}copy /Y \"{exe}\" \"{helper}\" >nul \
             && \"{icacls}\" \"{dir}\" /setowner *S-1-5-32-544 /T /C >nul \
             && \"{helper}\" {HELPER_ARG} {INSTALL_ARG}) > \"{dir}\\{SETUP_LOG}\" 2>&1)",
            exe = exe.display(),
        )
    }

    /// Turns the reading off: the helper quits, and one prompt removes its task and its copy.
    /// PawnIO stays installed (other tools may use it; it's removable from Installed apps).
    pub fn disable() -> Result<(), String> {
        // SAFETY: an event this process created at startup, opened by name and set.
        unsafe {
            if let Ok(stop) = OpenEventW(
                windows::Win32::System::Threading::EVENT_MODIFY_STATE,
                false,
                &HSTRING::from(STOP),
            ) {
                let _ = SetEvent(stop);
                let _ = CloseHandle(stop);
            }
        }
        let dir = helper_dir().ok_or("Couldn't find Program Files.")?;
        let taskkill = system_tool("taskkill.exe");
        let ping = system_tool("PING.EXE");
        let schtasks = system_tool("schtasks.exe");
        let dir = dir.display();
        // Fails only when the task or the folder is still there afterwards.
        elevated(
            &format!(
                "(\"{taskkill}\" /F /IM {HELPER_EXE} >nul 2>&1 && \"{ping}\" -n 2 127.0.0.1 >nul \
                 & \"{schtasks}\" /Delete /F /TN \"{TASK}\" >nul 2>&1 \
                 & (if exist \"{dir}\" rmdir /S /Q \"{dir}\") \
                 & \"{schtasks}\" /Query /TN \"{TASK}\" >nul 2>&1 && exit 1 \
                 || if exist \"{dir}\" (exit 2) else (exit 0))"
            ),
            None,
            "Turning it off failed",
        )
    }

    /// Runs `command` through cmd as administrator, after Windows asks, and waits for it. On
    /// failure, says why from the last line of `log` when there is one.
    fn elevated(command: &str, log: Option<&Path>, failed: &str) -> Result<(), String> {
        // /d: no AutoRun commands from the user's registry.
        let parameters = HSTRING::from(format!("/d /c {command}"));
        let verb = HSTRING::from("runas");
        let file = HSTRING::from(system_tool("cmd.exe"));
        let mut info = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(parameters.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        // SAFETY: the strings outlive the call; the process handle is waited on and closed.
        unsafe {
            ShellExecuteExW(&mut info)
                .map_err(|_| "Windows didn't get permission to set it up.".to_string())?;
            if info.hProcess.is_invalid() {
                return Ok(());
            }
            WaitForSingleObject(info.hProcess, INFINITE);
            let mut code = 0u32;
            let _ = GetExitCodeProcess(info.hProcess, &mut code);
            let _ = CloseHandle(info.hProcess);
            if code != 0 {
                let output = log
                    .and_then(|log| std::fs::read(log).ok())
                    .unwrap_or_default();
                let reason = super::last_line(&output);
                log::error!("cpu temperature: {failed} ({code}): {reason}");
                return Err(if reason.is_empty() {
                    format!("{failed} (code {code}).")
                } else {
                    format!("{failed}: {reason}")
                });
            }
        }
        Ok(())
    }

    // ---- The helper's side ----

    /// Whether `path` can only be changed, and is owned, by SYSTEM, administrators or
    /// TrustedInstaller.
    fn admin_only(path: &Path) -> bool {
        let mut owner = PSID::default();
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: the owner and list point into the descriptor, which is freed last; each
        // entry is read as the type its header names.
        unsafe {
            if GetNamedSecurityInfoW(
                &HSTRING::from(path.as_os_str()),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                Some(&mut owner),
                None,
                Some(&mut dacl),
                None,
                &mut descriptor,
            ) != ERROR_SUCCESS
            {
                return false;
            }
            // No list at all lets everyone in.
            let safe = !dacl.is_null()
                && sid_string(owner).is_some_and(|sid| super::TRUSTED.contains(&sid.as_str()))
                && (0..u32::from((*dacl).AceCount)).all(|index| {
                    let mut ace = std::ptr::null_mut();
                    if GetAce(dacl, index, &mut ace).is_err() {
                        return false;
                    }
                    let header = *ace.cast::<ACE_HEADER>();
                    // Allowed entries (with a condition or not) end in the SID; the others
                    // are judged by type alone.
                    let sid = if matches!(header.AceType, 0 | 9) {
                        let allowed = ace.cast::<ACCESS_ALLOWED_ACE>();
                        sid_string(PSID(std::ptr::addr_of_mut!((*allowed).SidStart).cast()))
                    } else {
                        Some(String::new())
                    };
                    let mask = (*ace.cast::<ACCESS_ALLOWED_ACE>()).Mask;
                    sid.is_some_and(|sid| {
                        !super::lets_others_write(header.AceType, header.AceFlags, mask, &sid)
                    })
                });
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
            safe
        }
    }

    fn sid_string(sid: PSID) -> Option<String> {
        let mut text = PWSTR::null();
        // SAFETY: a SID from a security descriptor; the string is copied and freed.
        unsafe {
            ConvertSidToStringSidW(sid, &mut text).ok()?;
            let result = text.to_string().ok();
            let _ = LocalFree(Some(HLOCAL(text.0.cast())));
            result
        }
    }

    /// Whether `path` and the folder it's in are both `admin_only`, saying which isn't.
    fn locked_down(path: &Path) -> bool {
        let checks = [Some(path), path.parent()];
        checks.into_iter().flatten().all(|path| {
            let safe = admin_only(path);
            if !safe {
                log::error!(
                    "cpu temperature helper: {} can be changed without administrator rights",
                    path.display()
                );
            }
            safe
        })
    }

    type Open = unsafe extern "system" fn(*mut HANDLE) -> i32;
    type Load = unsafe extern "system" fn(HANDLE, *const u8, usize) -> i32;
    type Execute = unsafe extern "system" fn(
        HANDLE,
        PCSTR,
        *const u64,
        usize,
        *mut u64,
        usize,
        *mut usize,
    ) -> i32;
    type Close = unsafe extern "system" fn(HANDLE) -> i32;

    struct PawnIo {
        handle: HANDLE,
        execute: Execute,
        close: Close,
    }

    impl PawnIo {
        /// PawnIO's library, an executor, and the module for this CPU loaded into it. The
        /// library is only loaded when no one but administrators can change it, and what it
        /// loads in turn comes from its own folder or System32.
        fn open(module: &[u8]) -> Option<PawnIo> {
            let library = pawnio_library()?;
            if !locked_down(&library) {
                return None;
            }
            let path = HSTRING::from(library.as_os_str());
            // SAFETY: PawnIO's documented exports, called with the arguments they take.
            unsafe {
                let library = LoadLibraryExW(
                    &path,
                    None,
                    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
                )
                .ok()?;
                let open: Open = std::mem::transmute(GetProcAddress(library, s!("pawnio_open"))?);
                let load: Load = std::mem::transmute(GetProcAddress(library, s!("pawnio_load"))?);
                let execute: Execute =
                    std::mem::transmute(GetProcAddress(library, s!("pawnio_execute"))?);
                let close: Close =
                    std::mem::transmute(GetProcAddress(library, s!("pawnio_close"))?);
                let mut handle = HANDLE::default();
                if open(&mut handle) < 0 {
                    return None;
                }
                if load(handle, module.as_ptr(), module.len()) < 0 {
                    close(handle);
                    return None;
                }
                Some(PawnIo {
                    handle,
                    execute,
                    close,
                })
            }
        }

        fn call(&self, name: PCSTR, input: &[u64]) -> Option<u64> {
            let mut out = [0u64; 1];
            let mut written = 0usize;
            // SAFETY: an open executor, and buffers of the sizes passed.
            let result = unsafe {
                (self.execute)(
                    self.handle,
                    name,
                    input.as_ptr(),
                    input.len(),
                    out.as_mut_ptr(),
                    out.len(),
                    &mut written,
                )
            };
            (result >= 0 && written >= 1).then_some(out[0])
        }
    }

    impl Drop for PawnIo {
        fn drop(&mut self) {
            // SAFETY: the executor is ours and used no more.
            unsafe {
                (self.close)(self.handle);
            }
        }
    }

    #[derive(Clone, Copy)]
    enum Vendor {
        Amd,
        Intel,
    }

    fn vendor() -> Option<Vendor> {
        // Leaf 0 exists on every x86-64 CPU.
        let leaf = std::arch::x86_64::__cpuid(0);
        let mut name = [0u8; 12];
        name[..4].copy_from_slice(&leaf.ebx.to_le_bytes());
        name[4..8].copy_from_slice(&leaf.edx.to_le_bytes());
        name[8..].copy_from_slice(&leaf.ecx.to_le_bytes());
        match &name {
            b"AuthenticAMD" => Some(Vendor::Amd),
            b"GenuineIntel" => Some(Vendor::Intel),
            _ => None,
        }
    }

    /// One reading in °C.
    fn sample(pawn: &PawnIo, vendor: Vendor, pci: HANDLE) -> Option<f32> {
        match vendor {
            Vendor::Amd => {
                // Only read with the lock held (abandoned still hands it over); otherwise skip
                // this reading rather than race another tool.
                // SAFETY: a mutex handle this thread holds only around the read.
                let wait = unsafe { WaitForSingleObject(pci, 1000) };
                if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
                    return None;
                }
                let value = pawn.call(s!("ioctl_read_smn"), &[0x0005_9800]);
                // SAFETY: owned since the wait above, released as soon as the register is read.
                unsafe {
                    let _ = ReleaseMutex(pci);
                }
                Some(super::amd_tctl(value?))
            }
            Vendor::Intel => {
                let status = pawn.call(s!("ioctl_read_msr"), &[0x1B1])?;
                let target = pawn.call(s!("ioctl_read_msr"), &[0x1A2])?;
                Some(super::intel_package(status, target))
            }
        }
    }

    /// The helper, or with `INSTALL_ARG` its setup. Reads while Zephyr asks for it until
    /// Zephyr quits or the reading is turned off.
    pub fn helper() {
        if std::env::args().any(|arg| arg == INSTALL_ARG) {
            if let Err(reason) = install() {
                println!("{reason}");
                std::process::exit(1);
            }
            return;
        }
        if std::env::args().any(|arg| arg == REMOVE_ARG) {
            if let Err(reason) = disable() {
                println!("{reason}");
                std::process::exit(1);
            }
            return;
        }
        // Dev builds are console programs, so the task opens a terminal window for them; the
        // helper lets it go. (Release builds have no console.) Libraries it loads later come
        // from System32 only.
        // SAFETY: detaching from a console this process may or may not have.
        unsafe {
            let _ = windows::Win32::System::Console::FreeConsole();
            let _ = SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32);
        }
        let (Ok(exe), Some(dir)) = (std::env::current_exe(), helper_dir()) else {
            return;
        };
        if !same_path(&exe, &dir.join(HELPER_EXE)) {
            migrate(&exe, &dir);
            return;
        }
        if !locked_down(&exe) {
            return;
        }
        let Some(vendor) = vendor() else {
            return;
        };
        let module = match vendor {
            Vendor::Amd => AMD_MODULE,
            Vendor::Intel => INTEL_MODULE,
        };
        let Some(pawn) = PawnIo::open(module) else {
            log::error!("cpu temperature helper: PawnIO didn't open");
            return;
        };
        // SAFETY: the security descriptor is freed after the mapping is made; the view stays
        // mapped for the helper's life.
        let view = unsafe {
            // Everyone signed in may read it; only SYSTEM and administrators write.
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &HSTRING::from("D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GR;;;IU)"),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
            .is_err()
            {
                return;
            }
            let attributes = SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor.0,
                bInheritHandle: false.into(),
            };
            let mapping = CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                Some(&attributes),
                PAGE_READWRITE,
                0,
                size_of::<Reading>() as u32,
                &HSTRING::from(MAPPING),
            );
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
            let Ok(mapping) = mapping else {
                return;
            };
            MapViewOfFile(mapping, FILE_MAP_WRITE, 0, 0, size_of::<Reading>())
        };
        if view.Value.is_null() {
            return;
        }
        // SAFETY: named objects opened and created by name; each handle lives with the helper.
        let pci =
            unsafe { CreateMutexW(None, false, &HSTRING::from(PCI_MUTEX)).unwrap_or_default() };
        let started = Instant::now();
        // Zephyr's ask, once it has made it: without one (an older Zephyr), it reads always.
        let mut wanted: Option<HANDLE> = None;
        let mut wanted_at = Instant::now();
        let mut sampled: Option<Instant> = None;
        loop {
            // Zephyr gone (after a grace period for it to start), or the reading turned off.
            // SAFETY: handles opened and closed here, waited on without blocking.
            let (alive, stopped) = unsafe {
                let alive = OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, &HSTRING::from(ALIVE))
                    .map(|handle| {
                        let _ = CloseHandle(handle);
                    })
                    .is_ok();
                let stopped = OpenEventW(SYNCHRONIZATION_SYNCHRONIZE, false, &HSTRING::from(STOP))
                    .map(|handle| {
                        let set = WaitForSingleObject(handle, 0) == WAIT_OBJECT_0;
                        let _ = CloseHandle(handle);
                        set
                    })
                    .unwrap_or(false);
                if wanted.is_none() {
                    wanted =
                        OpenEventW(SYNCHRONIZATION_SYNCHRONIZE, false, &HSTRING::from(WANTED)).ok();
                }
                (alive, stopped)
            };
            if stopped || (!alive && started.elapsed() > Duration::from_secs(15)) {
                break;
            }
            let active = wanted.is_none() || wanted_at.elapsed() < STAY_ACTIVE;
            if active && sampled.is_none_or(|at| at.elapsed() >= SAMPLE_EVERY) {
                sampled = Some(Instant::now());
                if let Some(celsius) = sample(&pawn, vendor, pci) {
                    let reading = Reading {
                        celsius,
                        taken_ms: now_ms(),
                    };
                    // SAFETY: the view is the size of one reading and this is its only writer.
                    unsafe { std::ptr::write_volatile(view.Value.cast::<Reading>(), reading) };
                }
            }
            let pause = if active { SAMPLE_EVERY } else { IDLE_CHECK };
            match wanted {
                // SAFETY: an event handle the helper keeps open.
                Some(event) => {
                    if unsafe { WaitForSingleObject(event, pause.as_millis() as u32) }
                        == WAIT_OBJECT_0
                    {
                        wanted_at = Instant::now();
                    }
                }
                None => std::thread::sleep(pause),
            }
        }
    }

    /// Run as administrator from the helper's folder: registers the task for this copy and
    /// starts it. The definition is written in the folder, which only administrators can
    /// change, and removed once registered.
    fn install() -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|err| err.to_string())?;
        let dir = helper_dir().ok_or("Couldn't find Program Files.")?;
        if !same_path(&exe, &dir.join(HELPER_EXE)) || !admin_only(&dir) || !admin_only(&exe) {
            return Err("The helper isn't in its own protected folder.".into());
        }
        let user = match (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
            (Ok(domain), Ok(name)) => format!("{domain}\\{name}"),
            (_, Ok(name)) => name,
            _ => return Err("Couldn't tell which user to set it up for.".into()),
        };
        let xml = super::task_xml(&user, &exe.display().to_string());
        // UTF-16 with a byte-order mark, as the declaration says.
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
        let definition = dir.join("task.xml");
        std::fs::write(&definition, bytes).map_err(|err| err.to_string())?;
        let schtasks = system_tool("schtasks.exe");
        let created = crate::util::hidden_command(&schtasks)
            .args(["/Create", "/F", "/TN", TASK, "/XML"])
            .arg(&definition)
            .status();
        let _ = std::fs::remove_file(&definition);
        if !created.is_ok_and(|status| status.success()) {
            return Err("Windows didn't register the helper's task.".into());
        }
        // Read by Zephyr, which can't run this copy to ask.
        std::fs::write(dir.join(PROTOCOL_FILE), PROTOCOL.to_string())
            .map_err(|err| err.to_string())?;
        let _ = crate::util::hidden_command(&schtasks)
            .args(["/Run", "/TN", TASK])
            .status();
        Ok(())
    }

    /// Started by a task an older Zephyr registered, which runs Zephyr's own copy: already
    /// running as administrator, it sets up the protected copy and its task in place of this
    /// one (the same steps as turning it on), and quits without reading.
    fn migrate(exe: &Path, dir: &Path) {
        log::warn!("cpu temperature helper: moving to {}", dir.display());
        let _ = crate::util::hidden_command(system_tool("cmd.exe"))
            .raw_arg(format!("/d /c {}", setup_command(exe, dir, None)))
            .status();
    }
}

#[cfg(windows)]
pub use imp::{disable, enable, helper, needs_setup, pawnio_installed, read, start};

#[cfg(not(windows))]
mod imp_other {
    pub fn read() -> Option<f32> {
        None
    }
    pub fn start(_enabled: bool) {}
    pub fn pawnio_installed() -> bool {
        false
    }
    pub fn enable() -> Result<(), String> {
        Err("CPU temperature through PawnIO is for Windows".into())
    }
    pub fn disable() -> Result<(), String> {
        Ok(())
    }
    pub fn helper() {}
    pub fn needs_setup() -> bool {
        false
    }
}

#[cfg(not(windows))]
pub use imp_other::{disable, enable, helper, needs_setup, pawnio_installed, read, start};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amd_tctl_reads_eighths_and_the_range_select_bit() {
        let cases = [
            (0u64, 0.0f32),
            (400 << 21, 50.0),
            (401 << 21, 50.125),
            ((400 << 21) | (1 << 19), 1.0),
            ((0x7FF << 21) | 0x7_FFFF, 255.875),
            // Bits above 31 and below 19 don't count.
            ((1 << 32) | (360 << 21) | 0xFFFF, 45.0),
        ];
        for (value, celsius) in cases {
            assert_eq!(amd_tctl(value), celsius, "{value:#x}");
        }
    }

    #[test]
    fn intel_package_counts_down_from_tj_max() {
        let cases = [
            (30u64 << 16, 100u64 << 16, 70.0f32),
            (0, 100 << 16, 100.0),
            // The valid bit and the other fields around the readout don't count.
            (
                (1 << 31) | (1 << 23) | (25 << 16) | 0xFFFF,
                (5 << 24) | (95 << 16) | 0xFFFF,
                70.0,
            ),
            (0x7F << 16, 0xFF << 16, 128.0),
        ];
        for (status, target, celsius) in cases {
            assert_eq!(
                intel_package(status, target),
                celsius,
                "{status:#x} {target:#x}"
            );
        }
    }

    #[test]
    fn last_line_takes_the_last_said() {
        let utf16 =
            |text: &str| -> Vec<u8> { text.encode_utf16().flat_map(u16::to_le_bytes).collect() };
        let cases: [(Vec<u8>, &str); 5] = [
            (Vec::new(), ""),
            (b"one\r\ntwo\r\n\r\n".to_vec(), "two"),
            (
                b"ERROR: Access is denied.\r\n".to_vec(),
                "Access is denied.",
            ),
            (utf16("Found PawnIO\r\n  Installed  \r\n"), "Installed"),
            (b"  \r\n \n".to_vec(), ""),
        ];
        for (bytes, line) in cases {
            assert_eq!(last_line(&bytes), line);
        }
    }

    #[test]
    fn only_trusted_accounts_may_write() {
        const USERS: &str = "S-1-5-32-545";
        const ME: &str = "S-1-5-21-1-2-3-1001";
        let read_execute = 0x1200A9;
        let full = 0x1F01FF;
        let cases = [
            (0, 0, full, "S-1-5-18", false),
            (0, 0, full, "S-1-5-32-544", false),
            (0, 0, full, TRUSTED[2], false),
            (0, 0, read_execute, USERS, false),
            (0, 0, full, ME, true),
            (0, 0, 0x2, USERS, true),
            (0, 0, 0x4_0000, ME, true),
            (0, 0, 0x4000_0000, USERS, true),
            // Passed down only, as CREATOR OWNER's is.
            (0, 0x08 | 0x03, 0x1000_0000, "S-1-3-0", false),
            (9, 0, full, ME, true),
            (1, 0, full, ME, false),
            (5, 0, read_execute, USERS, true),
        ];
        for (ace_type, flags, mask, sid, unsafe_) in cases {
            assert_eq!(
                lets_others_write(ace_type, flags, mask, sid),
                unsafe_,
                "{ace_type} {flags:#x} {mask:#x} {sid}"
            );
        }
    }

    #[test]
    fn task_xml_escapes_and_runs_the_helper() {
        let xml = task_xml(
            r#"PC\a&b"#,
            r"C:\Program Files\Zephyr CPU Temperature\zephyr-cpu-temperature.exe",
        );
        assert!(xml.contains("<UserId>PC\\a&amp;b</UserId>"));
        assert!(xml.contains(
            r"<Command>C:\Program Files\Zephyr CPU Temperature\zephyr-cpu-temperature.exe</Command>"
        ));
        assert!(xml.contains(&format!("<Arguments>{HELPER_ARG}</Arguments>")));
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
    }
}
