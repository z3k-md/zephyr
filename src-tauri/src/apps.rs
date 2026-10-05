use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Bang words that scope the bar to apps instead of naming a destination.
pub const SCOPE_TRIGGERS: &[&str] = &["app", "apps"];

/// How long a scan stays fresh before a summon triggers a background rescan.
const STALE_AFTER: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// Stable launch target: an AppUserModelID on Windows, a bundle path on macOS.
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchEntry {
    pub app_id: String,
    pub uses: u32,
    pub last_used: i64,
}

pub const LAUNCH_LIMIT: usize = 200;
pub const OVERRIDE_LIMIT: usize = 200;

pub fn is_scope(trigger: &str) -> bool {
    SCOPE_TRIGGERS.contains(&trigger)
}

/// What app matching needs to know, borrowed from the index and saved state.
#[derive(Clone, Copy)]
pub struct Catalog<'a> {
    pub apps: &'a [App],
    pub launches: &'a [LaunchEntry],
    pub overrides: &'a [String],
    pub now: i64,
}

impl<'a> Catalog<'a> {
    pub fn ranked(&self, query: &str, limit: usize) -> Vec<(&'a App, Strength)> {
        ranked(self.apps, self.launches, query, self.now, limit)
    }

    pub fn preferred(&self, query: &str) -> Option<&'a App> {
        preferred(self.apps, self.launches, self.overrides, query, self.now)
    }

    /// Like `preferred`, but ignoring learned overrides: would this text have launched an app?
    pub fn would_launch(&self, query: &str) -> bool {
        preferred(self.apps, self.launches, &[], query, self.now).is_some()
    }

    pub fn recent(&self, limit: usize) -> Vec<&'a App> {
        recent(self.apps, self.launches, self.now, limit)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Strength {
    Subsequence,
    Substring,
    Initials,
    WordPrefix,
    Prefix,
    Exact,
}

impl Strength {
    /// Strong matches are what the user plausibly meant to launch, so Enter takes them.
    pub fn is_strong(self) -> bool {
        self >= Strength::Initials
    }
}

pub fn strength(name: &str, query: &str) -> Option<Strength> {
    let name = name.to_lowercase();
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return None;
    }
    if name == query {
        return Some(Strength::Exact);
    }
    if name.starts_with(&query) {
        return Some(Strength::Prefix);
    }
    let words: Vec<&str> = name
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    if (0..words.len()).any(|index| words[index..].join(" ").starts_with(&query)) {
        return Some(Strength::WordPrefix);
    }
    if query.chars().count() >= 2 && !query.contains(' ') {
        let initials: String = words
            .iter()
            .filter_map(|word| word.chars().next())
            .collect();
        if initials.starts_with(&query) {
            return Some(Strength::Initials);
        }
    }
    if name.contains(&query) {
        return Some(Strength::Substring);
    }
    if query.chars().count() >= 3 && is_subsequence(&query, &name) {
        return Some(Strength::Subsequence);
    }
    None
}

fn is_subsequence(needle: &str, haystack: &str) -> bool {
    let mut rest = haystack.chars();
    needle
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .all(|ch| rest.any(|candidate| candidate == ch))
}

fn frecency(launches: &[LaunchEntry], app_id: &str, now: i64) -> f64 {
    launches
        .iter()
        .find(|entry| entry.app_id == app_id)
        .map(|entry| {
            let age_hours = (now - entry.last_used).max(0) as f64 / 3600.0;
            entry.uses as f64 / (1.0 + age_hours / 24.0)
        })
        .unwrap_or(0.0)
}

/// Apps matching `query`, best first: match strength, then launch frecency, then shorter names.
pub fn ranked<'a>(
    apps: &'a [App],
    launches: &[LaunchEntry],
    query: &str,
    now: i64,
    limit: usize,
) -> Vec<(&'a App, Strength)> {
    let mut matches: Vec<(&App, Strength, f64)> = apps
        .iter()
        .filter_map(|app| {
            strength(&app.name, query).map(|found| (app, found, frecency(launches, &app.id, now)))
        })
        .collect();
    matches.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then(right.2.total_cmp(&left.2))
            .then(left.0.name.len().cmp(&right.0.name.len()))
            .then(left.0.name.cmp(&right.0.name))
    });
    matches.truncate(limit);
    matches
        .into_iter()
        .map(|(app, found, _)| (app, found))
        .collect()
}

/// Apps for an empty `!app` scope: most launched first, then alphabetical.
pub fn recent<'a>(
    apps: &'a [App],
    launches: &[LaunchEntry],
    now: i64,
    limit: usize,
) -> Vec<&'a App> {
    let mut all: Vec<(&App, f64)> = apps
        .iter()
        .map(|app| (app, frecency(launches, &app.id, now)))
        .collect();
    all.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then(left.0.name.to_lowercase().cmp(&right.0.name.to_lowercase()))
    });
    all.truncate(limit);
    all.into_iter().map(|(app, _)| app).collect()
}

/// The app Enter should launch for unscoped free text, if any.
pub fn preferred<'a>(
    apps: &'a [App],
    launches: &[LaunchEntry],
    overrides: &[String],
    query: &str,
    now: i64,
) -> Option<&'a App> {
    let query = query.trim();
    if query.chars().count() < 2 {
        return None;
    }
    let lowered = query.to_lowercase();
    if overrides.iter().any(|entry| entry == &lowered) {
        return None;
    }
    ranked(apps, launches, query, now, 1)
        .into_iter()
        .next()
        .filter(|(_, found)| found.is_strong())
        .map(|(app, _)| app)
}

pub fn record_launch(launches: &mut Vec<LaunchEntry>, app_id: &str, now: i64) {
    if let Some(entry) = launches.iter_mut().find(|entry| entry.app_id == app_id) {
        entry.uses = entry.uses.saturating_add(1);
        entry.last_used = now;
    } else {
        launches.push(LaunchEntry {
            app_id: app_id.to_string(),
            uses: 1,
            last_used: now,
        });
    }
    if launches.len() > LAUNCH_LIMIT {
        launches.sort_by_key(|entry| std::cmp::Reverse(entry.last_used));
        launches.truncate(LAUNCH_LIMIT);
    }
}

/// Remembers that this exact text was meant for the web, so it stops launching an app.
pub fn record_override(overrides: &mut Vec<String>, query: &str) {
    let lowered = query.trim().to_lowercase();
    if lowered.is_empty() {
        return;
    }
    overrides.retain(|entry| entry != &lowered);
    overrides.push(lowered);
    if overrides.len() > OVERRIDE_LIMIT {
        let excess = overrides.len() - OVERRIDE_LIMIT;
        overrides.drain(..excess);
    }
}

/// Installed apps, rescanned in the background so a summon never waits on discovery.
#[derive(Default)]
pub struct AppIndex {
    apps: RwLock<Vec<App>>,
    scanned_at: Mutex<Option<Instant>>,
    scanning: AtomicBool,
}

impl AppIndex {
    pub fn snapshot(&self) -> Vec<App> {
        self.apps
            .read()
            .map(|apps| apps.clone())
            .unwrap_or_default()
    }

    pub fn find(&self, id: &str) -> Option<App> {
        self.apps
            .read()
            .ok()
            .and_then(|apps| apps.iter().find(|app| app.id == id).cloned())
    }

    /// Starts a scan on a worker thread unless one is running or the last one is still fresh.
    pub fn refresh_if_stale(index: &'static AppIndex) {
        let fresh = index
            .scanned_at
            .lock()
            .ok()
            .and_then(|at| *at)
            .is_some_and(|at| at.elapsed() < STALE_AFTER);
        if fresh || index.scanning.swap(true, Ordering::SeqCst) {
            return;
        }
        std::thread::spawn(move || {
            let started = Instant::now();
            let found = discover();
            log::info!("found {} apps in {:?}", found.len(), started.elapsed());
            if let Ok(mut apps) = index.apps.write() {
                *apps = found;
            }
            if let Ok(mut at) = index.scanned_at.lock() {
                *at = Some(Instant::now());
            }
            index.scanning.store(false, Ordering::SeqCst);
        });
    }
}

pub fn index() -> &'static AppIndex {
    use std::sync::OnceLock;
    static INDEX: OnceLock<AppIndex> = OnceLock::new();
    INDEX.get_or_init(AppIndex::default)
}

#[cfg(any(windows, target_os = "macos"))]
fn dedupe(mut apps: Vec<App>) -> Vec<App> {
    let mut seen = std::collections::HashSet::new();
    apps.retain(|app| seen.insert(app.name.to_lowercase()));
    apps.sort_by_key(|app| app.name.to_lowercase());
    apps
}

#[cfg(any(windows, test))]
fn is_noise(name: &str) -> bool {
    let lowered = name.to_lowercase();
    ["uninstall", "readme", "release notes", "documentation"]
        .iter()
        .any(|word| lowered.contains(word))
}

/// Parses `Get-StartApps | ConvertTo-Json`, which is an object for one app and an array otherwise.
#[cfg(any(windows, test))]
fn parse_start_apps(json: &str) -> Vec<App> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json.trim_start_matches('\u{feff}'))
    else {
        return Vec::new();
    };
    let entries = match value {
        serde_json::Value::Array(entries) => entries,
        other => vec![other],
    };
    entries
        .iter()
        .filter_map(|entry| {
            let name = entry.get("Name")?.as_str()?.trim();
            let id = entry.get("AppID")?.as_str()?.trim();
            // Internet shortcuts pinned to Start are web links, not apps.
            if name.is_empty() || id.is_empty() || id.contains("://") || is_noise(name) {
                return None;
            }
            Some(App {
                id: id.to_string(),
                name: name.to_string(),
            })
        })
        .collect()
}

#[cfg(windows)]
pub fn discover() -> Vec<App> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // Get-StartApps lists Win32 shortcuts and packaged apps under one AppUserModelID scheme.
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::OutputEncoding = [Text.Encoding]::UTF8; Get-StartApps | Select-Object Name, AppID | ConvertTo-Json -Compress",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match output {
        Ok(output) if output.status.success() => {
            dedupe(parse_start_apps(&String::from_utf8_lossy(&output.stdout)))
        }
        Ok(output) => {
            log::error!(
                "Get-StartApps failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            Vec::new()
        }
        Err(err) => {
            log::error!("couldn't run PowerShell to list apps: {err}");
            Vec::new()
        }
    }
}

#[cfg(target_os = "macos")]
pub fn discover() -> Vec<App> {
    let mut roots = vec![
        std::path::PathBuf::from("/Applications"),
        std::path::PathBuf::from("/System/Applications"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(std::path::PathBuf::from(home).join("Applications"));
    }
    let mut apps = scan_bundles(&roots, 3);
    let finder = std::path::Path::new("/System/Library/CoreServices/Finder.app");
    if finder.exists() {
        apps.push(App {
            id: finder.to_string_lossy().into_owned(),
            name: "Finder".into(),
        });
    }
    dedupe(apps)
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn discover() -> Vec<App> {
    Vec::new()
}

/// Collects `.app` bundles under `roots` without descending into the bundles themselves.
#[cfg(any(target_os = "macos", test))]
fn scan_bundles(roots: &[std::path::PathBuf], depth: usize) -> Vec<App> {
    fn walk(dir: &std::path::Path, depth: usize, apps: &mut Vec<App>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if path.extension().is_some_and(|ext| ext == "app") {
                if let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) {
                    apps.push(App {
                        id: path.to_string_lossy().into_owned(),
                        name: name.to_string(),
                    });
                }
            } else if depth > 1 {
                walk(&path, depth - 1, apps);
            }
        }
    }

    let mut apps = Vec::new();
    for root in roots {
        walk(root, depth, &mut apps);
    }
    apps
}

#[cfg(windows)]
pub fn launch(app: &App) -> Result<(), String> {
    std::process::Command::new("explorer.exe")
        .arg(format!("shell:AppsFolder\\{}", app.id))
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("Couldn't open {}: {err}", app.name))
}

#[cfg(target_os = "macos")]
pub fn launch(app: &App) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(&app.id)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("Couldn't open {}: {err}", app.name))
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn launch(app: &App) -> Result<(), String> {
    Err(format!("Launching {} isn't supported here yet", app.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> App {
        App {
            id: format!("id-{name}"),
            name: name.to_string(),
        }
    }

    #[test]
    fn match_strength_tiers() {
        assert_eq!(strength("Excel", "excel"), Some(Strength::Exact));
        assert_eq!(strength("Google Chrome", "goo"), Some(Strength::Prefix));
        assert_eq!(strength("Google Chrome", "chr"), Some(Strength::WordPrefix));
        assert_eq!(
            strength("Visual Studio Code", "studio co"),
            Some(Strength::WordPrefix)
        );
        assert_eq!(
            strength("Visual Studio Code", "vsc"),
            Some(Strength::Initials)
        );
        assert_eq!(strength("Notepad", "pad"), Some(Strength::Substring));
        assert_eq!(strength("Photoshop", "phsp"), Some(Strength::Subsequence));
        assert_eq!(strength("Notion", "notion api"), None);
        assert_eq!(strength("Notion", "  "), None);
    }

    #[test]
    fn free_text_does_not_launch_an_app() {
        let apps = vec![app("Notion"), app("Excel")];
        assert!(preferred(&apps, &[], &[], "notion api pagination", 0).is_none());
        assert!(preferred(&apps, &[], &[], "how to excel at interviews", 0).is_none());
    }

    #[test]
    fn a_name_prefix_launches_the_app() {
        let apps = vec![app("Google Chrome"), app("Excel")];
        assert_eq!(
            preferred(&apps, &[], &[], "chr", 0).map(|app| app.name.as_str()),
            Some("Google Chrome")
        );
        assert_eq!(
            preferred(&apps, &[], &[], "Excel", 0).map(|app| app.name.as_str()),
            Some("Excel")
        );
    }

    #[test]
    fn single_characters_and_weak_matches_stay_searches() {
        let apps = vec![app("Calculator"), app("Notepad")];
        assert!(preferred(&apps, &[], &[], "c", 0).is_none());
        assert!(preferred(&apps, &[], &[], "pad", 0).is_none());
    }

    #[test]
    fn an_override_keeps_that_text_on_the_web() {
        let apps = vec![app("Excel")];
        let mut overrides = Vec::new();
        record_override(&mut overrides, " Excel ");
        assert!(preferred(&apps, &[], &overrides, "excel", 0).is_none());
        assert!(preferred(&apps, &[], &overrides, "exc", 0).is_some());
    }

    #[test]
    fn launch_frecency_breaks_ties() {
        let apps = vec![app("Code"), app("Codex")];
        let mut launches = Vec::new();
        record_launch(&mut launches, "id-Codex", 100);
        record_launch(&mut launches, "id-Codex", 100);
        let top = ranked(&apps, &launches, "cod", 100, 2);
        assert_eq!(top[0].0.name, "Codex");
        // Strength still wins over frecency.
        let exact = ranked(&apps, &launches, "code", 100, 2);
        assert_eq!(exact[0].0.name, "Code");
    }

    #[test]
    fn recent_apps_lead_the_empty_scope() {
        let apps = vec![app("Alpha"), app("Beta"), app("Gamma")];
        let mut launches = Vec::new();
        record_launch(&mut launches, "id-Gamma", 10);
        let names: Vec<_> = recent(&apps, &launches, 10, 3)
            .into_iter()
            .map(|app| app.name.as_str())
            .collect();
        assert_eq!(names, ["Gamma", "Alpha", "Beta"]);
    }

    #[test]
    fn parses_start_apps_json() {
        let many = r#"[{"Name":"Excel","AppID":"Microsoft.Office.EXCEL.EXE.15"},
            {"Name":"Uninstall Foo","AppID":"{6D809377}\\foo\\uninstall.exe"},
            {"Name":"Docs","AppID":"https://example.com"},
            {"Name":"Settings","AppID":"windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel"}]"#;
        let names: Vec<_> = parse_start_apps(many)
            .into_iter()
            .map(|app| app.name)
            .collect();
        assert_eq!(names, ["Excel", "Settings"]);

        let one = "\u{feff}{\"Name\":\"Notepad\",\"AppID\":\"Microsoft.WindowsNotepad_8wekyb3d8bbwe!App\"}";
        assert_eq!(parse_start_apps(one).len(), 1);
        assert!(parse_start_apps("").is_empty());
    }

    #[test]
    fn scans_app_bundles_without_entering_them() {
        let root = std::env::temp_dir().join(format!("zephyr-apps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Safari.app/Contents/Helper.app")).unwrap();
        std::fs::create_dir_all(root.join("Utilities/Terminal.app")).unwrap();
        std::fs::write(root.join("notes.txt"), "").unwrap();

        let mut names: Vec<_> = scan_bundles(std::slice::from_ref(&root), 3)
            .into_iter()
            .map(|app| app.name)
            .collect();
        names.sort();
        assert_eq!(names, ["Safari", "Terminal"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
