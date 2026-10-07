//! Shell commands typed or pasted into the bar (`>` or `!sh`). A command runs in the user's
//! shell from their home folder with no window; its output streams back to the bar, and it
//! can be stopped or reopened in a real terminal.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// How many commands the bar remembers.
const HISTORY_LIMIT: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShellSettings {
    /// `auto`, `powershell`, `gitbash` or `wsl`. macOS and Linux always use the login shell.
    pub program: String,
    /// Commands run from the bar, newest first.
    pub history: Vec<String>,
    /// Where Enter sends a command on Windows: `auto` (Windows Terminal when installed) or
    /// `console` (a plain console window).
    pub terminal: String,
}

impl Default for ShellSettings {
    fn default() -> Self {
        Self {
            program: "auto".into(),
            history: Vec::new(),
            terminal: "auto".into(),
        }
    }
}

impl ShellSettings {
    pub fn record(&mut self, command: &str) {
        let command = command.trim();
        if command.is_empty() {
            return;
        }
        self.history.retain(|entry| entry != command);
        self.history.insert(0, command.to_string());
        self.history.truncate(HISTORY_LIMIT);
    }

    pub fn set_program(&mut self, program: &str) -> Result<(), String> {
        if !matches!(program, "auto" | "powershell" | "gitbash" | "wsl") {
            return Err(format!("Unknown shell {program}"));
        }
        self.program = program.into();
        Ok(())
    }

    pub fn set_terminal(&mut self, terminal: &str) -> Result<(), String> {
        if !matches!(terminal, "auto" | "console") {
            return Err(format!("Unknown terminal {terminal}"));
        }
        self.terminal = terminal.into();
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    #[cfg_attr(not(windows), allow(dead_code))]
    PowerShell,
    GitBash,
    Wsl,
    /// The user's login shell on macOS and Linux.
    #[cfg_attr(windows, allow(dead_code))]
    Login,
}

#[derive(Debug, Clone)]
struct Shell {
    kind: Kind,
    program: PathBuf,
}

impl Shell {
    fn name(&self) -> String {
        match self.kind {
            Kind::PowerShell => "PowerShell".into(),
            Kind::GitBash => "Git Bash".into(),
            Kind::Wsl => "WSL".into(),
            Kind::Login => self
                .program
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "shell".into()),
        }
    }

    /// Arguments that run `command` in `dir` and exit with its status. Afterwards the script
    /// prints the folder it ended in on a line starting with two 0x1E bytes and `ZCWD:`, so a
    /// `cd` carries over to the next command; the bar strips that line from the output.
    fn run_args(&self, command: &str, dir: Option<&str>) -> Vec<String> {
        let posix = |pwd: &str| {
            format!(
                "{command}\n__zephyr_status=$?\nprintf '\\n\\036\\036ZCWD:%s\\n' \"$({pwd})\"\nexit $__zephyr_status"
            )
        };
        match self.kind {
            Kind::PowerShell => vec![
                "-NoLogo".into(),
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-Command".into(),
                // Windows PowerShell writes the console code page to a pipe otherwise.
                format!(
                    "[Console]::OutputEncoding = [Text.Encoding]::UTF8\n{command}\n\
                     $__zephyrOk = $?; $__zephyrCode = $LASTEXITCODE\n\
                     [Console]::Out.Write(\"`n\" + [char]30 + [char]30 + 'ZCWD:' + (Get-Location).ProviderPath + \"`n\")\n\
                     if ($__zephyrCode) {{ exit $__zephyrCode }} elseif (-not $__zephyrOk) {{ exit 1 }}"
                ),
            ],
            // `pwd -W` gives the Windows path, which the next run starts in.
            Kind::GitBash => vec!["-lc".into(), posix("pwd -W 2>/dev/null || pwd")],
            Kind::Wsl => vec![
                "--cd".into(),
                dir.unwrap_or("~").into(),
                "-e".into(),
                "bash".into(),
                "-lc".into(),
                posix("pwd"),
            ],
            // Interactive so the rc file's PATH (bun, nvm, brew) and aliases apply.
            Kind::Login => vec!["-ilc".into(), posix("pwd")],
        }
    }
}

/// What Settings shows: the shells this machine has and the one a command would use.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellInfo {
    pub available: Vec<String>,
    pub using: Option<String>,
    /// Where Enter sends a command.
    pub terminal: String,
    pub has_windows_terminal: bool,
}

pub fn info(settings: &ShellSettings) -> ShellInfo {
    #[cfg(windows)]
    let available = [
        ("gitbash", git_bash().is_some()),
        ("powershell", true),
        ("wsl", wsl().is_some()),
    ]
    .into_iter()
    .filter(|(_, found)| *found)
    .map(|(id, _)| id.to_string())
    .collect();
    #[cfg(not(windows))]
    let available = Vec::new();
    #[cfg(windows)]
    let has_windows_terminal = windows_terminal().is_some();
    #[cfg(not(windows))]
    let has_windows_terminal = false;
    let terminal = if cfg!(target_os = "macos") {
        "Terminal"
    } else if has_windows_terminal && settings.terminal != "console" {
        "Windows Terminal"
    } else {
        "a console window"
    };
    ShellInfo {
        available,
        using: resolve(settings).ok().map(|shell| shell.name()),
        terminal: terminal.into(),
        has_windows_terminal,
    }
}

fn resolve(settings: &ShellSettings) -> Result<Shell, String> {
    #[cfg(windows)]
    {
        let shell = |kind, program| Shell { kind, program };
        match settings.program.as_str() {
            "powershell" => Ok(shell(Kind::PowerShell, powershell())),
            "gitbash" => git_bash()
                .map(|program| shell(Kind::GitBash, program))
                .ok_or_else(|| "Git Bash isn't installed".into()),
            "wsl" => wsl()
                .map(|program| shell(Kind::Wsl, program))
                .ok_or_else(|| "WSL isn't installed".into()),
            _ => Ok(git_bash()
                .map(|program| shell(Kind::GitBash, program))
                .unwrap_or_else(|| shell(Kind::PowerShell, powershell()))),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = settings;
        let program = std::env::var_os("SHELL")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .unwrap_or_else(|| PathBuf::from("/bin/zsh"));
        Ok(Shell {
            kind: Kind::Login,
            program,
        })
    }
}

#[cfg(windows)]
fn git_bash() -> Option<PathBuf> {
    use std::path::Path;
    let mut candidates = Vec::new();
    for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
        if let Some(root) = std::env::var_os(var) {
            candidates.push(Path::new(&root).join("Git\\bin\\bash.exe"));
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(Path::new(&local).join("Programs\\Git\\bin\\bash.exe"));
    }
    // A Git install elsewhere: git.exe sits in Git\cmd, bash.exe in Git\bin.
    if let Some(root) = which("git.exe")
        .as_deref()
        .and_then(Path::parent)
        .and_then(Path::parent)
    {
        candidates.push(root.join("bin\\bash.exe"));
    }
    candidates.into_iter().find(|path| path.is_file())
}

#[cfg(windows)]
fn wsl() -> Option<PathBuf> {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    Some(std::path::Path::new(&root).join("System32\\wsl.exe")).filter(|path| path.is_file())
}

/// PowerShell 7 when it is installed, otherwise the Windows PowerShell every PC has.
#[cfg(windows)]
fn powershell() -> PathBuf {
    if let Some(pwsh) = which("pwsh.exe") {
        return pwsh;
    }
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    std::path::Path::new(&root).join("System32\\WindowsPowerShell\\v1.0\\powershell.exe")
}

/// Windows Terminal's `wt` alias, present on Windows 11 and wherever it was installed.
#[cfg(windows)]
fn windows_terminal() -> Option<PathBuf> {
    which("wt.exe").or_else(|| {
        let local = std::env::var_os("LOCALAPPDATA")?;
        Some(std::path::Path::new(&local).join("Microsoft\\WindowsApps\\wt.exe"))
            .filter(|path| path.exists())
    })
}

#[cfg(windows)]
fn which(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

fn home() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

fn base_command(shell: &Shell) -> Command {
    let mut command = Command::new(&shell.program);
    command.current_dir(home());
    if shell.kind == Kind::GitBash {
        // Git Bash's login profile otherwise changes to the home folder itself.
        command.env("CHERE_INVOKING", "1");
    }
    command
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum ShellEvent {
    Started {
        id: u64,
        shell: String,
    },
    Output {
        text: String,
    },
    Exit {
        code: Option<i32>,
        stopped: bool,
        millis: u64,
    },
    Error {
        message: String,
    },
}

struct Running {
    pid: u32,
    stopped: bool,
}

fn running() -> &'static Mutex<HashMap<u64, Running>> {
    static RUNNING: OnceLock<Mutex<HashMap<u64, Running>>> = OnceLock::new();
    RUNNING.get_or_init(Default::default)
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Starts `command` in `dir` (the folder the previous command ended in, or home); its id,
/// output and exit arrive through `send`.
pub fn run(
    settings: &ShellSettings,
    command: &str,
    dir: Option<&str>,
    send: impl Fn(ShellEvent) + Send + Sync + Clone + 'static,
) -> Result<(), String> {
    let command = command.trim();
    if command.is_empty() {
        return Err("Type a command".into());
    }
    let shell = resolve(settings)?;
    let dir = dir.map(str::trim).filter(|dir| !dir.is_empty());
    let mut process = base_command(&shell);
    // WSL gets its Linux folder through --cd; every other shell starts in a real folder here.
    if shell.kind != Kind::Wsl
        && let Some(dir) = dir.map(PathBuf::from).filter(|dir| dir.is_dir())
    {
        process.current_dir(dir);
    }
    process
        .args(shell.run_args(command, dir))
        .env("PAGER", "cat")
        .env("GIT_PAGER", "cat")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        process.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group, so stopping it also stops whatever it started.
        process.process_group(0);
    }

    let started = Instant::now();
    let mut child = process
        .spawn()
        .map_err(|err| format!("Couldn't start {}: {err}", shell.name()))?;
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut running) = running().lock() {
        running.insert(
            id,
            Running {
                pid: child.id(),
                stopped: false,
            },
        );
    }
    send(ShellEvent::Started {
        id,
        shell: shell.name(),
    });

    let streams: [Option<Box<dyn Read + Send>>; 2] = [
        child.stdout.take().map(|out| Box::new(out) as _),
        child.stderr.take().map(|err| Box::new(err) as _),
    ];
    let readers: Vec<_> = streams
        .into_iter()
        .flatten()
        .map(|stream| {
            let send = send.clone();
            std::thread::spawn(move || pipe(stream, |text| send(ShellEvent::Output { text })))
        })
        .collect();

    std::thread::spawn(move || {
        for reader in readers {
            let _ = reader.join();
        }
        let status = child.wait();
        let stopped = running()
            .lock()
            .ok()
            .and_then(|mut running| running.remove(&id))
            .is_some_and(|entry| entry.stopped);
        match status {
            Ok(status) => send(ShellEvent::Exit {
                code: status.code(),
                stopped,
                millis: started.elapsed().as_millis() as u64,
            }),
            Err(err) => send(ShellEvent::Error {
                message: err.to_string(),
            }),
        }
    });
    Ok(())
}

/// Forwards a stream as text, never splitting a UTF-8 character across two chunks.
fn pipe(mut stream: Box<dyn Read + Send>, mut emit: impl FnMut(String)) {
    let mut buffer = [0u8; 8192];
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let read = match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        pending.extend_from_slice(&buffer[..read]);
        let complete = pending.len() - incomplete_tail(&pending);
        if complete > 0 {
            emit(String::from_utf8_lossy(&pending[..complete]).into_owned());
            pending.drain(..complete);
        }
    }
    if !pending.is_empty() {
        emit(String::from_utf8_lossy(&pending).into_owned());
    }
}

/// Bytes at the end that start a UTF-8 character still waiting for the rest of it.
fn incomplete_tail(bytes: &[u8]) -> usize {
    match std::str::from_utf8(bytes) {
        Err(err) if err.error_len().is_none() => bytes.len() - err.valid_up_to(),
        _ => 0,
    }
}

/// Stops a running command and everything it started.
pub fn stop(id: u64) {
    let pid = running().lock().ok().and_then(|mut running| {
        running.get_mut(&id).map(|entry| {
            entry.stopped = true;
            entry.pid
        })
    });
    let Some(pid) = pid else {
        return;
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGTERM);
    }
}

/// Opens a terminal window in `dir` that runs `command` and stays open at a prompt afterwards.
pub fn open_in_terminal(
    settings: &ShellSettings,
    command: &str,
    dir: Option<&str>,
) -> Result<(), String> {
    let command = command.trim();
    let shell = resolve(settings)?;
    let dir = dir.map(str::trim).filter(|dir| !dir.is_empty());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        let folder = dir
            .filter(|_| shell.kind != Kind::Wsl)
            .map(PathBuf::from)
            .filter(|dir| dir.is_dir())
            .unwrap_or_else(home);
        let args: Vec<String> = match shell.kind {
            Kind::PowerShell if command.is_empty() => vec!["-NoLogo".into(), "-NoExit".into()],
            Kind::PowerShell => vec![
                "-NoLogo".into(),
                "-NoExit".into(),
                "-Command".into(),
                command.into(),
            ],
            Kind::GitBash | Kind::Wsl | Kind::Login => {
                let script = if command.is_empty() {
                    "exec bash -l".to_string()
                } else {
                    format!("{command}\nexec bash -l")
                };
                if shell.kind == Kind::Wsl {
                    let cd = dir.unwrap_or("~").to_string();
                    vec![
                        "--cd".into(),
                        cd,
                        "-e".into(),
                        "bash".into(),
                        "-lc".into(),
                        script,
                    ]
                } else {
                    vec!["-lc".into(), script]
                }
            }
        };
        // Windows Terminal opens a tab in the window already open; otherwise a console window.
        let mut process = match windows_terminal().filter(|_| settings.terminal != "console") {
            Some(wt) => {
                let mut process = Command::new(wt);
                process
                    .args(["-w", "0", "new-tab", "--title", "Zephyr", "-d"])
                    .arg(&folder)
                    .arg(&shell.program)
                    // wt reads a bare ; as the start of its next command.
                    .args(args.iter().map(|arg| arg.replace(';', "\\;")));
                process
            }
            None => {
                let mut process = base_command(&shell);
                process
                    .current_dir(&folder)
                    .args(&args)
                    .creation_flags(CREATE_NEW_CONSOLE);
                process
            }
        };
        process
            .env("CHERE_INVOKING", "1")
            .spawn()
            .map(|_| ())
            .map_err(|err| format!("Couldn't open a terminal: {err}"))
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::PermissionsExt;
        let scripts = std::env::temp_dir().join("zephyr-shell");
        std::fs::create_dir_all(&scripts).map_err(|err| err.to_string())?;
        let script = scripts.join(format!(
            "run-{}.command",
            NEXT_ID.fetch_add(1, Ordering::SeqCst)
        ));
        let program = shell.program.to_string_lossy();
        let folder = match dir {
            Some(dir) => format!("'{}'", dir.replace('\'', "'\\''")),
            None => "~".into(),
        };
        // Terminal runs the file with the login shell, then leaves that shell open.
        std::fs::write(
            &script,
            format!("#!{program} -il\ncd {folder}\n{command}\nexec {program} -il\n"),
        )
        .map_err(|err| err.to_string())?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))
            .map_err(|err| err.to_string())?;
        Command::new("open")
            .arg(&script)
            .spawn()
            .map(|_| ())
            .map_err(|err| format!("Couldn't open Terminal: {err}"))
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = (command, shell, dir);
        Err("Opening a terminal isn't supported here yet".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_keeps_the_newest_first_without_repeats() {
        let mut settings = ShellSettings::default();
        settings.record("ls");
        settings.record("git status");
        settings.record("  ls  ");
        settings.record("");
        assert_eq!(settings.history, vec!["ls", "git status"]);
    }

    #[test]
    fn a_split_character_waits_for_the_rest() {
        let text = "héllo".as_bytes();
        // "h" then the first byte of "é".
        assert_eq!(incomplete_tail(&text[..2]), 1);
        assert_eq!(incomplete_tail(text), 0);
        assert_eq!(incomplete_tail(b"\xff"), 0);
    }

    /// Runs `command` to the end and returns its output and exit code.
    #[cfg(windows)]
    fn finish(settings: &ShellSettings, command: &str, dir: &str) -> (String, Option<i32>) {
        let (sender, receiver) = std::sync::mpsc::channel();
        run(settings, command, Some(dir), move |event| {
            let _ = sender.send(event);
        })
        .unwrap();
        let mut output = String::new();
        for event in receiver {
            match event {
                ShellEvent::Output { text } => output.push_str(&text),
                ShellEvent::Exit { code, .. } => return (output, code),
                ShellEvent::Error { message } => panic!("{message}"),
                ShellEvent::Started { .. } => {}
            }
        }
        panic!("no exit for {command}");
    }

    #[cfg(windows)]
    #[test]
    fn commands_report_their_status_and_the_folder_they_end_in() {
        // Not the temp folder: Git Bash mounts it as /tmp, whose parent is Git's own root.
        let start = std::env::current_dir().unwrap();
        let dir = start.to_string_lossy();
        let parent = start.parent().unwrap().to_string_lossy().replace('\\', "/");
        for program in ["gitbash", "powershell"] {
            if program == "gitbash" && git_bash().is_none() {
                continue;
            }
            let mut settings = ShellSettings::default();
            settings.set_program(program).unwrap();

            let (output, code) = finish(&settings, "cd ..; echo 'héllo; \"quoted\"'", &dir);
            assert_eq!(code, Some(0), "{program}: {output}");
            assert!(output.contains("héllo; \"quoted\""), "{program}: {output}");
            let ended = output
                .split("\u{1e}\u{1e}ZCWD:")
                .nth(1)
                .unwrap_or_default()
                .trim()
                .replace('\\', "/");
            assert!(
                ended.eq_ignore_ascii_case(&parent),
                "{program}: {ended} != {parent}"
            );

            // A failing program's code comes through even though the folder line prints after it.
            let failing = if program == "gitbash" {
                "cmd //c 'exit 3'"
            } else {
                "cmd /c 'exit 3'"
            };
            let (output, code) = finish(&settings, failing, &dir);
            assert_eq!(code, Some(3), "{program}: {output}");
            assert!(output.contains("ZCWD:"), "{program}: {output}");
        }
    }

    #[test]
    fn only_known_shells_can_be_chosen() {
        let mut settings = ShellSettings::default();
        assert!(settings.set_program("wsl").is_ok());
        assert!(settings.set_program("cmd").is_err());
        assert_eq!(settings.program, "wsl");
    }
}
