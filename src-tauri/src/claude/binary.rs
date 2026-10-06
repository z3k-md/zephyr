//! Finding the user's `claude` CLI. Apps opened from the Dock or Start menu don't inherit the
//! shell's PATH, so the usual install locations are checked before asking the login shell.

use std::path::{Path, PathBuf};
use std::process::Command;

pub fn resolve(configured: &str) -> Option<PathBuf> {
    let configured = configured.trim();
    if !configured.is_empty() {
        let path = PathBuf::from(configured);
        return path.is_file().then_some(path);
    }
    candidates()
        .into_iter()
        .find(|path| path.is_file())
        .or_else(from_shell)
}

#[cfg(not(windows))]
fn candidates() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    vec![
        Path::new(&home).join(".local/bin/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
        Path::new(&home).join(".claude/local/claude"),
    ]
}

#[cfg(windows)]
fn candidates() -> Vec<PathBuf> {
    let profile = std::env::var("USERPROFILE").unwrap_or_default();
    vec![Path::new(&profile).join(".local\\bin\\claude.exe")]
}

#[cfg(not(windows))]
fn from_shell() -> Option<PathBuf> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let output = Command::new(shell)
        .args(["-lc", "command -v claude"])
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let path = PathBuf::from(path);
    path.is_file().then_some(path)
}

/// `where` can list the npm `.cmd` shim first; only a real `.exe` is used.
#[cfg(windows)]
fn from_shell() -> Option<PathBuf> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = Command::new("where")
        .arg("claude")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| PathBuf::from(line.trim()))
        .find(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
                && path.is_file()
        })
}

/// A command for the CLI that never opens a console window on Windows.
pub fn command(binary: &Path) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(binary);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliStatus {
    pub path: Option<String>,
    pub version: Option<String>,
    pub logged_in: Option<bool>,
    pub auth_method: Option<String>,
}

pub fn status(configured: &str) -> CliStatus {
    let Some(path) = resolve(configured) else {
        return CliStatus {
            path: None,
            version: None,
            logged_in: None,
            auth_method: None,
        };
    };
    let version = command(&path)
        .arg("--version")
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|version| !version.is_empty());
    let auth: Option<serde_json::Value> = command(&path)
        .args(["auth", "status"])
        .output()
        .ok()
        .and_then(|out| serde_json::from_slice(&out.stdout).ok());
    CliStatus {
        path: Some(path.to_string_lossy().into_owned()),
        version,
        logged_in: auth.as_ref().and_then(|auth| auth["loggedIn"].as_bool()),
        auth_method: auth
            .as_ref()
            .and_then(|auth| auth["authMethod"].as_str().map(str::to_string)),
    }
}
