use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::apps::{self, LaunchEntry};
use crate::clipboard::ClipboardSettings;
use crate::files;
use crate::history::{self, HistoryEntry};
use crate::shortcut::{DEFAULT_SHORTCUT, LEGACY_DEFAULT_SHORTCUT};

/// A field missing from the file falls back to its default instead of discarding every setting.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Persisted {
    #[serde(default = "missing_schema_version")]
    pub version: u32,
    pub summon_shortcut: String,
    pub launch_at_startup: bool,
    /// Windows: start from a logon task right at sign-in instead of the startup list, which
    /// Windows delays. Off unless chosen, since Defender may flag the task.
    pub instant_startup: bool,
    pub history: Vec<HistoryEntry>,
    pub launches: Vec<LaunchEntry>,
    /// Lowercased texts the user sent to the web even though they matched an app.
    pub app_overrides: Vec<String>,
    /// Folders file search indexes; none saved means the home folder.
    pub file_roots: Option<Vec<String>>,
    /// Folders left out of file search.
    pub file_excludes: Vec<String>,
    /// Files opened from the bar, keyed by full path, for ranking and the empty `!f` list.
    pub file_opens: Vec<LaunchEntry>,
    /// Settings pages opened from the bar, by setting id, for ranking.
    pub setting_opens: Vec<LaunchEntry>,
    /// Views opened (Network, Resources, Notes…), by view id, to fill the empty bar's Views.
    pub view_opens: Vec<LaunchEntry>,
    /// Clipboard history: on or off, its shortcut, retention and ignored apps.
    pub clipboard: ClipboardSettings,
    /// Global shortcut that opens the notes window; empty for none.
    #[serde(default = "default_notes_shortcut")]
    pub notes_shortcut: String,
    /// Best typing test result per mode, e.g. `time-30` or `words-25`.
    pub typing_bests: Vec<TypingBest>,
    /// Reopening the bar within this many seconds returns to the view it closed on
    /// (clipboard, typing, notes); 0 always starts at the search box.
    #[serde(default = "default_resume_seconds")]
    pub resume_seconds: u32,
    /// The view the bar opens to while one is active (`notes`, `clipboard`, …); empty for the search
    /// bar.
    pub active_mode: String,
    /// Open to the active mode rather than the search bar.
    #[serde(default = "yes")]
    pub return_to_mode: bool,
    /// The shell for commands run from the bar (`>`), and the ones run so far.
    pub shell: crate::shell::ShellSettings,
    /// Where the user Ctrl+dragged the bar, in physical pixels; none opens it centered on the
    /// screen with the cursor.
    pub bar_position: Option<BarPosition>,
    /// The color palette every window uses; one of [`THEMES`].
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Whether the theme was picked in Settings, rather than left at a default; an old
    /// default moves to a new one, a pick never does.
    pub theme_chosen: bool,
    /// Whether first-run setup was finished or skipped. Files from before setup existed
    /// read as done, so upgrading never shows it.
    #[serde(default = "yes")]
    pub onboarding_done: bool,
    /// Dev builds: show setup on every launch, for testing it.
    pub onboarding_every_launch: bool,
    /// A shutdown, restart, sleep, lock or sign-out set for later from the bar.
    pub power: Option<crate::power::Schedule>,
    /// Notes open in their own windows, reopened where they were at the next start.
    pub note_windows: Vec<NoteWindow>,
    /// Whether Ctrl+R and Ctrl+Shift+S record and snapshot Zephyr's window for bug reports.
    #[serde(default = "yes")]
    pub bug_capture: bool,
    /// Whether file search may read Desktop, Documents and Downloads. macOS asks before an
    /// app reads them, so they wait until file search is first used rather than prompting
    /// at launch.
    pub protected_folders: bool,
    /// Whether the bar's footer shows CPU, memory, disk and network.
    #[serde(default = "yes")]
    pub show_stats: bool,
    /// Hotkeys and aliases the user gave commands, apps and bookmarks.
    pub bindings: Vec<Binding>,
    /// Apps (and links and folders) opened together: "Coding", "Gaming".
    pub launch_groups: Vec<LaunchGroup>,
    /// Where the weather is for; none uses where the IP address says the computer is.
    pub weather_place: Option<crate::weather::Place>,
    /// `c` or `f` for the weather's temperatures (and wind in km/h or mph); none follows
    /// the region's own way.
    pub weather_unit: Option<String>,
    /// With no place picked, whether the weather asks ipapi.co where the IP address is.
    #[serde(default = "yes")]
    pub weather_by_ip: bool,
    /// Whether the connection monitor pings the router and 1.1.1.1 once a second.
    #[serde(default = "yes")]
    pub watch_connection: bool,
    /// The last speed test's result.
    pub last_speed: Option<crate::net::speed::SpeedResult>,
    /// Windows: read the CPU's temperature through PawnIO and an elevated helper (cputemp.rs).
    pub cpu_temp: bool,
    /// Windows: Zephyr takes the volume keys and shows its own overlay (volume_keys.rs).
    pub volume_keys: bool,
    /// Where the volume overlay shows: `bottom` or `top`, centered, or `bottom-right`.
    pub volume_overlay: String,
}

/// The palettes Settings > Appearance offers; the first is the default.
pub const THEMES: &[&str] = &["sidebar", "violet", "graphite", "ocean", "forest", "ember"];

fn default_theme() -> String {
    THEMES[0].into()
}

/// A named set of things opened in one go from the bar or a hotkey.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct LaunchGroup {
    pub id: String,
    pub name: String,
    /// A glyph name for its row; empty uses the first app's icon.
    pub icon: String,
    pub items: Vec<GroupItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct GroupItem {
    /// `app`, `url` or `folder`.
    pub kind: String,
    /// An app id, a URL, or a folder path.
    pub value: String,
    /// What the row and Settings call it.
    pub label: String,
}

/// A hotkey and/or an alias for one thing the bar can do. `target` names it: `view:clip`,
/// `scope:files`, `app:<id>`, `dest:<id>`, `bookmark:<url>`, `group:<id>` or `setting:<id>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Binding {
    pub target: String,
    /// What Settings calls it ("Clipboard", "Spotify").
    pub label: String,
    /// A global shortcut that runs it from anywhere; empty for none.
    pub hotkey: String,
    /// Typed in the bar (then a space) to go straight to it; empty for none.
    pub alias: String,
    /// Fires as soon as the alias is typed, no space: for symbols like `=`.
    pub direct: bool,
}

/// A note open in its own window, and where that window was, in physical pixels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteWindow {
    pub id: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BarPosition {
    pub x: i32,
    pub y: i32,
}

fn yes() -> bool {
    true
}

fn default_resume_seconds() -> u32 {
    120
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TypingBest {
    pub mode: String,
    pub wpm: f64,
    pub raw: f64,
    pub accuracy: f64,
    pub consistency: f64,
    pub at: i64,
}

#[cfg(target_os = "macos")]
fn default_notes_shortcut() -> String {
    "command+option+n".into()
}

#[cfg(not(target_os = "macos"))]
fn default_notes_shortcut() -> String {
    "ctrl+alt+n".into()
}

const SCHEMA_VERSION: u32 = 4;

/// Files written before `version` existed should still migrate.
fn missing_schema_version() -> u32 {
    1
}

impl Default for Persisted {
    fn default() -> Self {
        Self::fresh()
    }
}

impl Persisted {
    pub fn fresh() -> Self {
        Self {
            version: SCHEMA_VERSION,
            summon_shortcut: DEFAULT_SHORTCUT.to_string(),
            launch_at_startup: false,
            instant_startup: false,
            history: Vec::new(),
            launches: Vec::new(),
            app_overrides: Vec::new(),
            file_roots: None,
            file_excludes: Vec::new(),
            file_opens: Vec::new(),
            setting_opens: Vec::new(),
            view_opens: Vec::new(),
            clipboard: ClipboardSettings::default(),
            notes_shortcut: default_notes_shortcut(),
            typing_bests: Vec::new(),
            resume_seconds: default_resume_seconds(),
            active_mode: String::new(),
            return_to_mode: true,
            shell: crate::shell::ShellSettings::default(),
            bar_position: None,
            theme: default_theme(),
            theme_chosen: false,
            onboarding_done: false,
            onboarding_every_launch: false,
            power: None,
            note_windows: Vec::new(),
            bug_capture: true,
            protected_folders: false,
            show_stats: true,
            bindings: Vec::new(),
            launch_groups: Vec::new(),
            weather_place: None,
            weather_unit: None,
            weather_by_ip: true,
            watch_connection: true,
            last_speed: None,
            cpu_temp: false,
            volume_keys: false,
            volume_overlay: "bottom".to_string(),
        }
    }

    pub fn set_clipboard(&mut self, settings: ClipboardSettings) -> Result<(), String> {
        let settings = settings.normalized()?;
        let clashes = |other: &str| crate::shortcut::clashes(other, &settings.shortcut);
        if clashes(&self.summon_shortcut) {
            return Err("Clipboard history needs a different shortcut from the bar".into());
        }
        // The rest only clash while history's shortcut is registered, which is when it's on.
        if settings.enabled {
            if clashes(&self.notes_shortcut) {
                return Err("That shortcut already opens notes".into());
            }
            if let Some(binding) = self
                .bindings
                .iter()
                .find(|binding| clashes(&binding.hotkey))
            {
                return Err(format!("That shortcut already runs {}", binding.label));
            }
        }
        self.clipboard = settings;
        Ok(())
    }

    pub fn set_notes_shortcut(&mut self, shortcut: String) -> Result<(), String> {
        let shortcut = shortcut.trim().to_string();
        let shortcut = if shortcut.is_empty() {
            shortcut
        } else {
            crate::shortcut::canonical_shortcut(&shortcut)?
        };
        let clashes = |other: &str| crate::shortcut::clashes(other, &shortcut);
        if clashes(&self.summon_shortcut) || clashes(&self.clipboard.shortcut) {
            return Err("Notes needs its own shortcut".into());
        }
        if let Some(binding) = self
            .bindings
            .iter()
            .find(|binding| clashes(&binding.hotkey))
        {
            return Err(format!("That shortcut already runs {}", binding.label));
        }
        self.notes_shortcut = shortcut;
        Ok(())
    }

    /// Checks a summon shortcut against the other shortcuts Zephyr registers.
    pub fn check_summon_shortcut(&self, shortcut: &str) -> Result<(), String> {
        let clashes = |other: &str| crate::shortcut::clashes(other, shortcut);
        if clashes(&self.clipboard.shortcut) {
            return Err("That shortcut already opens clipboard history".into());
        }
        if clashes(&self.notes_shortcut) {
            return Err("That shortcut already opens notes".into());
        }
        if let Some(binding) = self
            .bindings
            .iter()
            .find(|binding| clashes(&binding.hotkey))
        {
            return Err(format!("That shortcut already runs {}", binding.label));
        }
        Ok(())
    }

    /// The optional shortcuts to register alongside the summon one.
    pub fn extra_shortcuts(&self) -> crate::shortcut::Extras<'_> {
        crate::shortcut::Extras {
            clipboard: self
                .clipboard
                .enabled
                .then_some(self.clipboard.shortcut.as_str()),
            notes: Some(self.notes_shortcut.as_str()),
            bindings: &self.bindings,
        }
    }

    /// Keeps `result` if it beats the best for its mode; says whether it did.
    pub fn record_typing(&mut self, result: TypingBest) -> Result<bool, String> {
        let valid_mode = result.mode.len() <= 16
            && result
                .mode
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-');
        if !valid_mode || !result.wpm.is_finite() || !(0.0..=400.0).contains(&result.wpm) {
            return Err("That result doesn't look right".into());
        }
        match self
            .typing_bests
            .iter_mut()
            .find(|best| best.mode == result.mode)
        {
            Some(best) if best.wpm >= result.wpm => Ok(false),
            Some(best) => {
                *best = result;
                Ok(true)
            }
            None => {
                self.typing_bests.push(result);
                Ok(true)
            }
        }
    }

    pub fn record(&mut self, query: &str, link: bool, now: i64) {
        history::record(&mut self.history, query, link, now);
    }

    pub fn record_launch(&mut self, app_id: &str, now: i64) {
        apps::record_launch(&mut self.launches, app_id, now);
    }

    pub fn record_file_open(&mut self, path: &str, now: i64) {
        apps::record_launch(&mut self.file_opens, path, now);
    }

    pub fn file_config(&self) -> files::Config {
        let mut config = files::Config::resolve(self.file_roots.as_deref(), &self.file_excludes);
        if !self.protected_folders {
            config.excludes.extend(files::protected_folders());
        }
        config
    }

    /// Saves file search folders; `None` roots go back to the home folder.
    pub fn set_file_folders(
        &mut self,
        roots: Option<Vec<String>>,
        excludes: Vec<String>,
    ) -> Result<(), String> {
        self.file_roots = roots
            .map(|roots| files::normalize_folders(&roots))
            .transpose()?;
        self.file_excludes = files::normalize_folders(&excludes)?;
        Ok(())
    }

    pub fn record_setting_open(&mut self, setting_id: &str, now: i64) {
        apps::record_launch(&mut self.setting_opens, setting_id, now);
    }

    pub fn record_view_open(&mut self, view: &str, now: i64) {
        apps::record_launch(&mut self.view_opens, view, now);
    }

    pub fn record_app_override(&mut self, query: &str) {
        apps::record_override(&mut self.app_overrides, query);
    }

    pub fn set_general(&mut self, shortcut: String, launch_at_startup: bool) {
        self.summon_shortcut = shortcut;
        self.launch_at_startup = launch_at_startup;
    }

    /// Brings a file from an older version up to date; says whether anything changed.
    fn migrate(&mut self) -> bool {
        // An older build run since (an installed release, say) can put back what went with
        // destinations, so they go on every load, not only on the move to version 3.
        let dropped = self.drop_destinations();
        // A launch group left with nothing in it (Settings drops these itself; this catches one
        // left by a crash or an older version).
        let groups = self.launch_groups.len();
        self.launch_groups.retain(|group| !group.items.is_empty());
        let dropped = dropped || self.launch_groups.len() != groups;
        if self.version >= SCHEMA_VERSION {
            return dropped;
        }
        // Installs that still have the old default summon shortcut move onto the current one.
        if self.version < 2 && self.summon_shortcut == LEGACY_DEFAULT_SHORTCUT {
            self.summon_shortcut = DEFAULT_SHORTCUT.to_string();
        }
        // Violet was the default before Sidebar; anyone who never picked moves over.
        if self.version < 4 && !self.theme_chosen && self.theme == "violet" {
            self.theme = default_theme();
        }
        self.version = SCHEMA_VERSION;
        true
    }

    /// Searches used to go to one of several sites (destinations); now they all go to Google.
    /// History for the others goes, with the hotkeys and aliases that armed one and the old
    /// AI views. Says whether there were any.
    fn drop_destinations(&mut self) -> bool {
        let before = (self.history.len(), self.bindings.len());
        self.history
            .retain(|entry| matches!(entry.destination_id.as_str(), "google" | "url"));
        self.bindings
            .retain(|binding| match binding.target.split_once(':') {
                Some(("dest", _)) => false,
                Some(("view", view)) => !matches!(view, "ai" | "claude"),
                _ => true,
            });
        before != (self.history.len(), self.bindings.len())
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        // Compact: the file is read by Zephyr, not people, and history makes it long.
        let data = serde_json::to_vec(self).map_err(|err| err.to_string())?;
        let tmp = path.with_extension("json.tmp");
        // Flushed to disk before the rename, so a power cut can't leave an empty file.
        crate::util::write_synced(&tmp, &data).map_err(|err| err.to_string())?;
        // Rename replaces the old file, on Windows too. It can fail for a moment while
        // something (an antivirus scan, a backup) has the file open, so it gets a few tries.
        // Never delete the old file first: if the rename then failed, no settings file is left.
        let mut attempt = 0;
        loop {
            match fs::rename(&tmp, path) {
                Ok(()) => return Ok(()),
                Err(_) if attempt < 3 => {
                    attempt += 1;
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(err) => return Err(err.to_string()),
            }
        }
    }

    pub fn load(path: &Path) -> Self {
        let backup = path.with_extension("json.bak");
        // A save interrupted between writing and renaming leaves only the new copy behind. A
        // file cut short or left empty falls back to the copy kept at the last good start.
        let mut sources = vec![path.to_path_buf(), path.with_extension("json.tmp")];
        if path.exists() {
            sources.push(backup.clone());
        }
        for (at, source) in sources.iter().enumerate() {
            let Ok(bytes) = fs::read(source) else {
                continue;
            };
            let Some(mut state) = Self::parse(&bytes) else {
                log::error!("{} could not be read", source.display());
                // Kept aside for recovery by hand; never over the good backup.
                if at == 0 && !bytes.is_empty() {
                    let _ = fs::write(path.with_extension("json.corrupt"), &bytes);
                }
                continue;
            };
            if at == 0
                && let Err(err) = crate::util::write_atomic(&backup, &bytes)
            {
                log::warn!("couldn't back up settings: {err}");
            }
            if (state.migrate() || at != 0)
                && let Err(err) = state.save(path)
            {
                log::error!("couldn't save migrated settings: {err}");
            }
            return state;
        }
        Self::fresh()
    }

    /// Reads a settings file field by field: one that doesn't fit (an older or newer version
    /// shaped it differently) goes back to its default without taking the rest with it, and
    /// a list loses only the items that don't fit. None when it isn't a settings file at all.
    fn parse(bytes: &[u8]) -> Option<Self> {
        use serde_json::{Map, Value};
        let value: Value = serde_json::from_slice(bytes).ok()?;
        if !value.is_object() {
            return None;
        }
        if let Ok(state) = Self::deserialize(&value) {
            return Some(state);
        }
        let Value::Object(fields) = value else {
            return None;
        };
        let fits = |key: &str, value: &Value| {
            let one = Map::from_iter([(key.to_string(), value.clone())]);
            Self::deserialize(&Value::Object(one)).is_ok()
        };
        let mut kept = Map::new();
        for (key, value) in fields {
            if fits(&key, &value) {
                kept.insert(key, value);
                continue;
            }
            if let Value::Array(items) = &value {
                let items: Vec<Value> = items
                    .iter()
                    .filter(|item| fits(&key, &Value::Array(vec![(*item).clone()])))
                    .cloned()
                    .collect();
                let items = Value::Array(items);
                if fits(&key, &items) {
                    log::warn!("dropped settings in {key} that didn't fit");
                    kept.insert(key, items);
                    continue;
                }
            }
            log::warn!("{key} in settings didn't fit; using its default");
        }
        Self::deserialize(&Value::Object(kept)).ok()
    }
}

pub struct AppState {
    /// Shared, so a read (every keystroke takes one) is a reference count, not a deep copy.
    /// Held only to read or swap it, never while the disk is written.
    inner: Mutex<std::sync::Arc<Persisted>>,
    /// Taken for a whole update, so updates apply and reach the disk one at a time, in order:
    /// an older copy never lands over a newer one.
    writer: Mutex<()>,
    path: PathBuf,
}

impl AppState {
    pub fn load(path: PathBuf) -> Self {
        let persisted = Persisted::load(&path);
        Self {
            inner: Mutex::new(std::sync::Arc::new(persisted)),
            writer: Mutex::new(()),
            path,
        }
    }

    pub fn snapshot(&self) -> Result<std::sync::Arc<Persisted>, String> {
        Ok(self.lock()?.clone())
    }

    pub fn update<F>(&self, mutate: F) -> Result<Persisted, String>
    where
        F: FnOnce(&mut Persisted) -> Result<(), String>,
    {
        // It guards no data, so a panic in another update leaves nothing to distrust.
        let _writing = self
            .writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Work on a copy so a failed mutation or save leaves memory matching disk.
        let current = self.snapshot()?;
        let mut next = Persisted::clone(&current);
        mutate(&mut next)?;
        next.save(&self.path)?;
        *self.lock()? = std::sync::Arc::new(next.clone());
        Ok(next)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, std::sync::Arc<Persisted>>, String> {
        self.inner
            .lock()
            .map_err(|_| "Settings were left in a bad state. Restart Zephyr.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcut::{DEFAULT_SHORTCUT, LEGACY_DEFAULT_SHORTCUT};

    #[test]
    fn legacy_default_shortcut_moves_to_the_platform_default() {
        let mut state = Persisted::fresh();
        state.version = 1;
        state.summon_shortcut = LEGACY_DEFAULT_SHORTCUT.into();
        assert!(state.migrate());
        assert_eq!(state.summon_shortcut, DEFAULT_SHORTCUT);
        assert_eq!(state.version, SCHEMA_VERSION);
        assert!(!state.migrate());
    }

    #[test]
    fn a_custom_shortcut_survives_the_default_change() {
        let mut state = Persisted::fresh();
        state.version = 1;
        state.summon_shortcut = "ctrl+alt+k".into();
        assert!(state.migrate());
        assert_eq!(state.summon_shortcut, "ctrl+alt+k");
        assert_eq!(state.version, SCHEMA_VERSION);
    }

    #[test]
    fn what_pointed_at_other_destinations_is_dropped() {
        let mut state: Persisted = serde_json::from_str(
            r#"{
                "version": 3,
                "defaultDestinationId": "pubmed",
                "destinations": [
                    {"id": "google", "name": "Google", "triggers": ["g"], "urlTemplate": "https://www.google.com/search?q={query}", "suggest": "google", "pinned": true, "builtin": true, "disabled": false}
                ],
                "history": [
                    {"query": "cats", "destinationId": "google", "uses": 1, "lastUsed": 1},
                    {"query": "example.com", "destinationId": "url", "uses": 1, "lastUsed": 1},
                    {"query": "crispr", "destinationId": "pubmed", "uses": 1, "lastUsed": 1}
                ],
                "bindings": [
                    {"target": "view:ai", "label": "Ask AI", "hotkey": "ctrl+alt+a"},
                    {"target": "dest:google", "label": "Google", "alias": "s"},
                    {"target": "app:code", "label": "Code", "alias": "c"}
                ]
            }"#,
        )
        .unwrap();
        assert!(state.migrate());
        let kept: Vec<&str> = state.history.iter().map(|e| e.query.as_str()).collect();
        assert_eq!(kept, ["cats", "example.com"]);
        assert_eq!(state.bindings.len(), 1);
        assert_eq!(state.bindings[0].target, "app:code");
        assert!(!state.migrate());
    }

    #[test]
    fn a_partial_file_keeps_the_fields_it_has() {
        let state: Persisted =
            serde_json::from_str(r#"{"summonShortcut":"ctrl+alt+k","launchAtStartup":true}"#)
                .unwrap();
        assert_eq!(state.summon_shortcut, "ctrl+alt+k");
        assert!(state.launch_at_startup);
        assert_eq!(state.version, 1);
        assert!(state.history.is_empty());
    }

    #[test]
    fn a_field_that_doesnt_fit_keeps_the_other_settings() {
        let state = Persisted::parse(
            br#"{
                "version": 4,
                "summonShortcut": "ctrl+alt+k",
                "power": {"kind": "shutdown", "when": "later"},
                "clipboard": 7,
                "theme": "ocean",
                "history": [
                    {"query": "cats", "destinationId": "google", "uses": 1, "lastUsed": 1},
                    {"query": 5}
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(state.summon_shortcut, "ctrl+alt+k");
        assert_eq!(state.theme, "ocean");
        assert!(state.power.is_none());
        assert_eq!(
            state.clipboard.shortcut,
            ClipboardSettings::default().shortcut
        );
        let kept: Vec<&str> = state.history.iter().map(|e| e.query.as_str()).collect();
        assert_eq!(kept, ["cats"]);
        assert!(Persisted::parse(b"").is_none());
        assert!(Persisted::parse(b"[1]").is_none());
        assert!(Persisted::parse(br#"{"summonShortcut": "ctrl+al"#).is_none());
    }

    #[test]
    fn a_cut_short_or_empty_file_falls_back_to_the_backup() {
        let dir = std::env::temp_dir().join(format!("zephyr-backup-{}", std::process::id()));
        let path = dir.join("state.json");
        let mut state = Persisted::fresh();
        state.summon_shortcut = "ctrl+alt+k".into();
        state.save(&path).unwrap();
        // Loading a good file keeps it as the backup.
        assert_eq!(Persisted::load(&path).summon_shortcut, "ctrl+alt+k");

        for broken in [&b""[..], br#"{"summonShortcut": "ctrl+al"#] {
            fs::write(&path, broken).unwrap();
            assert_eq!(Persisted::load(&path).summon_shortcut, "ctrl+alt+k");
            // The backup was put back, and a broken file never replaces it.
            assert_eq!(Persisted::load(&path).summon_shortcut, "ctrl+alt+k");
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn violet_moves_to_the_new_default_once_unless_picked() {
        let mut state = Persisted::fresh();
        state.version = 3;
        state.theme = "violet".into();
        assert!(state.migrate());
        assert_eq!(state.theme, THEMES[0]);

        let mut picked = Persisted::fresh();
        picked.version = 3;
        picked.theme = "violet".into();
        picked.theme_chosen = true;
        picked.migrate();
        assert_eq!(picked.theme, "violet");

        let mut current = Persisted::fresh();
        current.theme = "violet".into();
        assert!(!current.migrate());
        assert_eq!(current.theme, "violet");
    }

    #[test]
    fn shortcut_clashes_ignore_case_and_order() {
        let mut state = Persisted::fresh();
        state.summon_shortcut = "ctrl+alt+k".into();
        state.clipboard.shortcut = "ctrl+shift+v".into();
        state.bindings.push(Binding {
            target: "app:code".into(),
            label: "Code".into(),
            hotkey: "ctrl+alt+c".into(),
            ..Binding::default()
        });
        assert!(state.set_notes_shortcut("Ctrl+Alt+K".into()).is_err());
        assert!(state.set_notes_shortcut("Shift+Ctrl+V".into()).is_err());
        assert!(state.set_notes_shortcut("alt+ctrl+c".into()).is_err());
        assert!(state.set_notes_shortcut("ctrl+alt+n".into()).is_ok());
        assert!(state.set_notes_shortcut(String::new()).is_ok());

        state.notes_shortcut = "ctrl+alt+n".into();
        assert!(state.check_summon_shortcut("Ctrl+Alt+N").is_err());
        assert!(state.check_summon_shortcut("CTRL+SHIFT+V").is_err());
        assert_eq!(
            state.check_summon_shortcut("Alt+Ctrl+C"),
            Err("That shortcut already runs Code".to_string())
        );
        assert!(state.check_summon_shortcut("alt+space").is_ok());
    }

    #[test]
    fn a_rejected_update_leaves_memory_untouched() {
        let dir = std::env::temp_dir().join(format!("zephyr-state-{}", std::process::id()));
        let app_state = AppState::load(dir.join("state.json"));
        let before = app_state.snapshot().unwrap();
        let result = app_state.update(|persisted| {
            persisted.summon_shortcut = "ctrl+alt+k".into();
            Err("no".into())
        });
        assert!(result.is_err());
        assert_eq!(
            app_state.snapshot().unwrap().summon_shortcut,
            before.summon_shortcut
        );
        let _ = fs::remove_dir_all(dir);
    }
}
