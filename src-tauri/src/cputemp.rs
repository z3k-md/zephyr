//! Windows: the CPU's temperature, opt-in from Settings › Resources. Windows only lets a
//! driver read it, so this uses PawnIO (a signed, open-source driver) and its signed AMD and
//! Intel modules. PawnIO only answers administrators, so a small helper does the reading: this
//! same program started with `--cpu-temp-helper` from a Task Scheduler task that runs with
//! the highest privileges. Turning it on asks Windows once (one UAC prompt) to install PawnIO
//! when it's missing and to register the task; Zephyr then starts the task itself at launch.
//!
//! The helper puts each reading in a small shared memory block anyone signed in can read,
//! and quits when Zephyr does or when it's turned off.

/// The Task Scheduler task that starts the helper. At the library's top level: creating it in
/// a folder of its own failed with "path not found" from schtasks /XML.
#[cfg(windows)]
const TASK: &str = "Zephyr CPU temperature";
/// The argument that makes this program the helper instead of the app.
pub const HELPER_ARG: &str = "--cpu-temp-helper";

#[cfg(windows)]
mod imp {
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use windows::Win32::Foundation::{
        CloseHandle, HANDLE, HLOCAL, INVALID_HANDLE_VALUE, LocalFree,
    };
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::Win32::System::Memory::{
        CreateFileMappingW, FILE_MAP_READ, FILE_MAP_WRITE, MapViewOfFile, OpenFileMappingW,
        PAGE_READWRITE, UnmapViewOfFile,
    };
    use windows::Win32::System::Threading::{
        CreateEventW, CreateMutexW, GetExitCodeProcess, INFINITE, OpenEventW, OpenMutexW,
        ReleaseMutex, SYNCHRONIZATION_SYNCHRONIZE, SetEvent, WaitForSingleObject,
    };
    use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    use windows::core::{HSTRING, PCSTR, PCWSTR, s};

    use super::TASK;

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
    /// AMD's SMN registers sit behind PCI config space other tools use too; this named mutex is
    /// how they take turns.
    const PCI_MUTEX: &str = r"Global\Access_PCI";

    const AMD_MODULE: &[u8] = include_bytes!("../resources/pawnio/AMDFamily17.bin");
    const INTEL_MODULE: &[u8] = include_bytes!("../resources/pawnio/IntelMSR.bin");

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |time| time.as_millis() as u64)
    }

    // ---- The app's side ----

    /// The helper's latest reading, when it's running and read in the last few seconds.
    pub fn read() -> Option<f32> {
        // SAFETY: a read-only view of the helper's block, its size, released before return.
        unsafe {
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

    /// Marks Zephyr as running for the helper, and starts the helper when the reading is on.
    /// Running the task needs no prompt: it was set up with permission when turned on.
    pub fn start(enabled: bool) {
        // SAFETY: a named mutex owned for the life of the process; its handle is never closed.
        unsafe {
            let _ = CreateMutexW(None, true, &HSTRING::from(ALIVE));
            if let Ok(stop) = CreateEventW(None, true, false, &HSTRING::from(STOP)) {
                // Kept open so the helper can find it; reset in case an old one lingered.
                let _ = windows::Win32::System::Threading::ResetEvent(stop);
            }
        }
        if enabled {
            std::thread::spawn(|| {
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

    fn pawnio_library() -> Option<std::path::PathBuf> {
        let base = std::env::var_os("ProgramFiles")?;
        Some(std::path::PathBuf::from(base).join(r"PawnIO\PawnIOLib.dll"))
    }

    /// Turns the reading on: one administrator prompt installs PawnIO when it's missing and
    /// registers the helper's task, then the helper starts.
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
        let mut steps = Vec::new();
        if !pawnio_installed() {
            steps.push(
                "winget install --id namazso.PawnIO -e --silent --accept-package-agreements \
                 --accept-source-agreements"
                    .to_string(),
            );
        }
        // From XML, since schtasks' own flags can't lift the defaults that would stop it: on
        // battery, and after three days of running.
        let definition = std::env::temp_dir().join("zephyr-cpu-temperature-task.xml");
        write_task(&definition, &exe)?;
        steps.push(format!(
            "schtasks /Create /F /TN \"{TASK}\" /XML \"{}\"",
            definition.display()
        ));
        steps.push(format!("schtasks /Run /TN \"{TASK}\""));
        elevated(&steps.join(" && "))?;
        if !pawnio_installed() {
            return Err("PawnIO didn't install. Try again, or install it from pawnio.eu.".into());
        }
        Ok(())
    }

    /// The helper's task: at sign-in and whenever Zephyr starts it, as this user with the
    /// highest privileges, on battery too, for as long as it runs, one copy at a time.
    fn write_task(path: &std::path::Path, exe: &std::path::Path) -> Result<(), String> {
        let escape = |text: &str| {
            text.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
        };
        let user = match (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
            (Ok(domain), Ok(name)) => format!("{domain}\\{name}"),
            (_, Ok(name)) => name,
            _ => return Err("Couldn't tell which user to set it up for.".into()),
        };
        let user = escape(&user);
        let xml = format!(
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
      <Command>{}</Command>
      <Arguments>{}</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
            escape(&exe.display().to_string()),
            super::HELPER_ARG
        );
        // UTF-16 with a byte-order mark, as the declaration says.
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
        std::fs::write(path, bytes).map_err(|err| err.to_string())
    }

    /// Turns the reading off: the helper quits, and one prompt removes its task. PawnIO stays
    /// installed (other tools may use it; it's removable from Installed apps).
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
        elevated(&format!("schtasks /Delete /F /TN \"{TASK}\""))
    }

    /// Runs `command` through cmd as administrator, after Windows asks, and waits for it.
    fn elevated(command: &str) -> Result<(), String> {
        // What the commands print goes to a file, so a failure can say why.
        let log = std::env::temp_dir().join("zephyr-cpu-temperature.log");
        let _ = std::fs::remove_file(&log);
        let parameters = HSTRING::from(format!("/c ({command}) > \"{}\" 2>&1", log.display()));
        let verb = HSTRING::from("runas");
        let file = HSTRING::from("cmd.exe");
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
                let output = std::fs::read(&log).unwrap_or_default();
                let reason = last_line(&output);
                log::error!("cpu temperature setup failed ({code}): {reason}");
                return Err(if reason.is_empty() {
                    format!("Setting it up failed (code {code}).")
                } else {
                    format!("Setting it up failed: {reason}")
                });
            }
        }
        Ok(())
    }

    /// The last line a command printed, as text. cmd writes in the console's code page, and
    /// winget in UTF-16, so both are tried.
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

    // ---- The helper's side ----

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
        /// PawnIO's library, an executor, and the module for this CPU loaded into it.
        fn open(module: &[u8]) -> Option<PawnIo> {
            let path = HSTRING::from(pawnio_library()?.as_os_str());
            // SAFETY: PawnIO's documented exports, called with the arguments they take.
            unsafe {
                let library = LoadLibraryW(&path).ok()?;
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
            // Tctl, as AMD's own tools show it: 0.125 °C steps in bits 21–31, 49 lower when
            // the range-select bit is set.
            Vendor::Amd => {
                // SAFETY: a mutex handle this thread holds only around the read.
                unsafe { WaitForSingleObject(pci, 1000) };
                let value = pawn.call(s!("ioctl_read_smn"), &[0x0005_9800]);
                // SAFETY: released as soon as the register is read.
                unsafe {
                    let _ = ReleaseMutex(pci);
                }
                let value = value?;
                let mut celsius = ((value >> 21) & 0x7FF) as f32 / 8.0;
                if value & (1 << 19) != 0 {
                    celsius -= 49.0;
                }
                Some(celsius)
            }
            // The package's distance below its throttle point, and that point.
            Vendor::Intel => {
                let status = pawn.call(s!("ioctl_read_msr"), &[0x1B1])?;
                let target = pawn.call(s!("ioctl_read_msr"), &[0x1A2])?;
                let tj_max = ((target >> 16) & 0xFF) as f32;
                Some(tj_max - ((status >> 16) & 0x7F) as f32)
            }
        }
    }

    /// The helper: reads once a second until Zephyr quits or the reading is turned off.
    pub fn helper() {
        // Dev builds are console programs, so the task opens a terminal window for them; the
        // helper lets it go. (Release builds have no console.)
        // SAFETY: detaching from a console this process may or may not have.
        unsafe {
            let _ = windows::Win32::System::Console::FreeConsole();
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
                        let set = WaitForSingleObject(handle, 0).0 == 0;
                        let _ = CloseHandle(handle);
                        set
                    })
                    .unwrap_or(false);
                (alive, stopped)
            };
            if stopped || (!alive && started.elapsed() > Duration::from_secs(15)) {
                break;
            }
            if let Some(celsius) = sample(&pawn, vendor, pci) {
                let reading = Reading {
                    celsius,
                    taken_ms: now_ms(),
                };
                // SAFETY: the view is the size of one reading and this is its only writer.
                unsafe { std::ptr::write_volatile(view.Value.cast::<Reading>(), reading) };
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}

#[cfg(windows)]
pub use imp::{disable, enable, helper, pawnio_installed, read, start};

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
}

#[cfg(not(windows))]
pub use imp_other::{disable, enable, helper, pawnio_installed, read, start};
