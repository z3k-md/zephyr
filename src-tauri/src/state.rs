use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::ai::AiSettings;
use crate::apps::{self, LaunchEntry};
use crate::clipboard::ClipboardSettings;
use crate::destination::{self, Destination};
use crate::history::{self, HistoryEntry};
use crate::shortcut::{DEFAULT_SHORTCUT, LEGACY_DEFAULT_SHORTCUT};
use crate::{files, settings};

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
    pub default_destination_id: String,
    pub destinations: Vec<Destination>,
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
    /// The provider and model behind the Ask AI destination; keys are in the keychain.
    pub ai: AiSettings,
    /// Clipboard history: on or off, its shortcut, retention and ignored apps.
    pub clipboard: ClipboardSettings,
    /// Global shortcut that opens the notes window; empty for none.
    #[serde(default = "default_notes_shortcut")]
    pub notes_shortcut: String,
    /// Best typing test result per mode, e.g. `time-30` or `words-25`.
    pub typing_bests: Vec<TypingBest>,
    /// Projects and limits for background Claude Code jobs (`!claude`).
    pub claude: crate::claude::ClaudeSettings,
    /// Reopening the bar within this many seconds returns to the view it closed on
    /// (Claude, clipboard, typing); 0 always starts at the search box.
    #[serde(default = "default_resume_seconds")]
    pub resume_seconds: u32,
    /// The view the bar opens to while one is active (`claude`, `ai`, `notes`, …); empty for
    /// the search bar.
    pub active_mode: String,
    /// Open to the active mode rather than the search bar.
    #[serde(default = "yes")]
    pub return_to_mode: bool,
    /// Where ⌘↵ sends the text: empty for "the other primary" (Ask AI or web search), a
    /// destination id, or `claude:<project id>`.
    pub route_cmd: String,
    /// Where ⌥↵ sends the text: empty for a Claude job in the last project used.
    pub route_alt: String,
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
    /// Browser profiles whose bookmarks stay out of the bar, by source id (`chrome/Default`).
    pub bookmarks_disabled: Vec<String>,
    /// Whether the bar's footer shows the song or video playing, with its controls.
    #[serde(default = "yes")]
    pub show_now_playing: bool,
    /// Whether installed Steam games are listed with the apps.
    #[serde(default = "yes")]
    pub steam_games: bool,
    /// Notes open in their own windows, reopened where they were at the next start.
    pub note_windows: Vec<NoteWindow>,
    /// Whether Ctrl+Alt+R and Ctrl+Alt+S record and snapshot the bar for bug reports.
    #[serde(default = "yes")]
    pub bug_capture: bool,
    /// Whether file search may read Desktop, Documents and Downloads. macOS asks before an
    /// app reads them, so they wait until file search is first used rather than prompting
    /// at launch.
    pub protected_folders: bool,
    /// Whether the bar's footer shows CPU, memory, disk and network.
    #[serde(default = "yes")]
    pub show_stats: bool,
    /// The one AI the bar offers (a destination id); empty means the first AI listed.
    pub ai_choice: String,
    /// Hotkeys and aliases the user gave commands, apps, destinations and bookmarks.
    pub bindings: Vec<Binding>,
    /// Apps (and links and folders) opened together: "Coding", "Gaming".
    pub launch_groups: Vec<LaunchGroup>,
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

/// A hotkey and/or an alias for one thing the bar can do. `target` names it: `view:ai`,
/// `scope:files`, `app:<id>`, `dest:<id>`, `bookmark:<url>`, `group:<id>` or `setting:<id>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Binding {
    pub target: String,
    /// What Settings calls it ("Ask AI", "Spotify").
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

const SCHEMA_VERSION: u32 = 2;

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
            default_destination_id: "google".into(),
            destinations: destination::builtins(),
            history: Vec::new(),
            launches: Vec::new(),
            app_overrides: Vec::new(),
            file_roots: None,
            file_excludes: Vec::new(),
            file_opens: Vec::new(),
            setting_opens: Vec::new(),
            ai: AiSettings::default(),
            clipboard: ClipboardSettings::default(),
            notes_shortcut: default_notes_shortcut(),
            typing_bests: Vec::new(),
            claude: crate::claude::ClaudeSettings::default(),
            resume_seconds: default_resume_seconds(),
            active_mode: String::new(),
            return_to_mode: true,
            route_cmd: String::new(),
            route_alt: String::new(),
            shell: crate::shell::ShellSettings::default(),
            bar_position: None,
            theme: default_theme(),
            theme_chosen: false,
            onboarding_done: false,
            onboarding_every_launch: false,
            power: None,
            bookmarks_disabled: Vec::new(),
            show_now_playing: true,
            steam_games: true,
            note_windows: Vec::new(),
            bug_capture: true,
            protected_folders: false,
            show_stats: true,
            ai_choice: String::new(),
            bindings: Vec::new(),
            launch_groups: Vec::new(),
        }
    }

    /// Adds built-ins a newer version introduced, at their built-in position, so a new
    /// primary destination lands in the pinned strip rather than after every custom one.
    pub fn merge_builtins(&mut self) {
        for (position, builtin) in destination::builtins().into_iter().enumerate() {
            if self
                .destinations
                .iter()
                .any(|destination| destination.id == builtin.id)
            {
                continue;
            }
            let at = position.min(self.destinations.len());
            self.destinations.insert(at, builtin);
        }
    }

    pub fn set_clipboard(&mut self, settings: ClipboardSettings) -> Result<(), String> {
        let settings = settings.normalized()?;
        // Compared as keys, not text, so "Alt+Shift+V" and "shift+alt+v" still clash.
        let shortcut = settings.shortcut.as_str();
        let clashes = |other: &str| !shortcut.is_empty() && crate::shortcut::same(other, shortcut);
        if clashes(&self.summon_shortcut) {
            return Err("Clipboard history needs a different shortcut from the bar".into());
        }
        // The rest only clash while history's shortcut is registered, which is when it's on.
        if settings.enabled {
            if clashes(&self.notes_shortcut) {
                return Err("That shortcut already opens notes".into());
            }
            if self.bug_capture
                && (clashes(crate::capture::SHORTCUT) || clashes(crate::capture::SNAPSHOT_SHORTCUT))
            {
                return Err("That shortcut already takes a bug capture".into());
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
        if !shortcut.is_empty()
            && (shortcut == self.summon_shortcut || shortcut == self.clipboard.shortcut)
        {
            return Err("Notes needs its own shortcut".into());
        }
        self.notes_shortcut = shortcut;
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
            capture: self.bug_capture,
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

    pub fn set_claude(&mut self, settings: crate::claude::ClaudeSettings) -> Result<(), String> {
        self.claude = settings.normalized()?;
        Ok(())
    }

    pub fn set_ai(&mut self, settings: AiSettings) -> Result<(), String> {
        self.ai = settings.normalized()?;
        Ok(())
    }

    pub fn record(&mut self, query: &str, destination_id: &str, now: i64) {
        history::record(&mut self.history, query, destination_id, now);
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

    pub fn record_app_override(&mut self, query: &str) {
        apps::record_override(&mut self.app_overrides, query);
    }

    pub fn set_general(
        &mut self,
        shortcut: String,
        launch_at_startup: bool,
        default_destination_id: String,
    ) -> Result<(), String> {
        if !self
            .destinations
            .iter()
            .any(|destination| destination.id == default_destination_id && !destination.disabled)
        {
            return Err("Choose an enabled destination as the default".into());
        }
        self.summon_shortcut = shortcut;
        self.launch_at_startup = launch_at_startup;
        self.default_destination_id = default_destination_id;
        Ok(())
    }

    pub fn upsert(&mut self, mut incoming: Destination) -> Result<(), String> {
        incoming.name = incoming.name.trim().to_string();
        incoming.triggers = normalize_triggers(&incoming.triggers)?;
        incoming.url_template = incoming.url_template.trim().to_string();

        let mut next = self.clone();
        if let Some(existing) = next
            .destinations
            .iter_mut()
            .find(|destination| destination.id == incoming.id)
        {
            let builtin = existing.builtin;
            existing.name = incoming.name;
            existing.triggers = incoming.triggers;
            existing.url_template = incoming.url_template;
            existing.suggest = incoming.suggest;
            existing.suggest_url = incoming.suggest_url.trim().to_string();
            existing.suggest_path = incoming.suggest_path.trim().to_string();
            existing.pinned = incoming.pinned;
            existing.disabled = incoming.disabled;
            existing.builtin = builtin;
            if existing.is_ai() {
                existing.url_template.clear();
            }
        } else {
            if incoming.builtin || !incoming.id.starts_with("custom-") {
                return Err("New destinations need a custom id".into());
            }
            incoming.builtin = false;
            incoming.kind = destination::DestinationKind::Web;
            incoming.suggest_url = incoming.suggest_url.trim().to_string();
            incoming.suggest_path = incoming.suggest_path.trim().to_string();
            next.destinations.push(incoming);
        }
        next.ensure_invariants()?;
        *self = next;
        Ok(())
    }

    /// Adds or updates every destination in an exported list, all or nothing. Entries whose id
    /// matches update that destination; the rest are added as custom ones.
    pub fn import(&mut self, incoming: Vec<ImportedDestination>) -> Result<usize, String> {
        if incoming.is_empty() {
            return Err("There are no destinations in that JSON".into());
        }
        let mut next = self.clone();
        let mut count = 0;
        for (index, item) in incoming.into_iter().enumerate() {
            let existing = item
                .id
                .as_deref()
                .and_then(|id| {
                    next.destinations
                        .iter()
                        .find(|destination| destination.id == id)
                })
                .cloned();
            let destination = match existing {
                Some(existing) if existing.is_ai() => continue,
                Some(existing) => Destination {
                    name: item.name,
                    triggers: item.triggers,
                    url_template: item.url_template,
                    suggest: item.suggest,
                    suggest_url: item.suggest_url,
                    suggest_path: item.suggest_path,
                    pinned: item.pinned.unwrap_or(existing.pinned),
                    disabled: item.disabled.unwrap_or(existing.disabled),
                    ..existing
                },
                None => Destination {
                    id: format!("custom-import-{}-{index}", crate::util::now_ms()),
                    name: item.name,
                    triggers: item.triggers,
                    url_template: item.url_template,
                    suggest: item.suggest,
                    suggest_url: item.suggest_url,
                    suggest_path: item.suggest_path,
                    pinned: item.pinned.unwrap_or(true),
                    builtin: false,
                    disabled: item.disabled.unwrap_or(false),
                    kind: destination::DestinationKind::Web,
                },
            };
            let name = destination.name.clone();
            next.upsert(destination)
                .map_err(|err| format!("{name}: {err}"))?;
            count += 1;
        }
        *self = next;
        Ok(count)
    }

    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        let Some(existing) = self
            .destinations
            .iter()
            .find(|destination| destination.id == id)
        else {
            return Err("That destination is missing.".into());
        };
        if existing.builtin {
            return Err("Built-in destinations can be turned off, not deleted".into());
        }
        let mut next = self.clone();
        next.destinations.retain(|destination| destination.id != id);
        next.history.retain(|entry| entry.destination_id != id);
        next.ensure_invariants()?;
        *self = next;
        Ok(())
    }

    pub fn move_destination(&mut self, id: &str, delta: i32) -> Result<(), String> {
        let Some(index) = self
            .destinations
            .iter()
            .position(|destination| destination.id == id)
        else {
            return Err("That destination is missing.".into());
        };
        let target = index as i32 + delta;
        if target < 0 || target >= self.destinations.len() as i32 {
            return Ok(());
        }
        self.destinations.swap(index, target as usize);
        Ok(())
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Moves installs that still have the old default summon shortcut onto the current one.
    fn migrate(&mut self) -> bool {
        if self.version >= SCHEMA_VERSION {
            return false;
        }
        if self.summon_shortcut == LEGACY_DEFAULT_SHORTCUT {
            self.summon_shortcut = DEFAULT_SHORTCUT.to_string();
        }
        self.version = SCHEMA_VERSION;
        true
    }

    fn ensure_invariants(&mut self) -> Result<(), String> {
        if self
            .destinations
            .iter()
            .all(|destination| destination.disabled)
        {
            return Err("At least one destination has to stay on".into());
        }

        let mut seen = HashSet::new();
        for destination in &self.destinations {
            if destination.name.trim().is_empty() {
                return Err("Destination name is required".into());
            }
            if destination.name.chars().count() > 40 {
                return Err("Destination name must be 40 characters or fewer".into());
            }
            if destination.triggers.is_empty() {
                return Err(format!("{} needs a trigger", destination.name));
            }
            if !destination.is_ai() {
                crate::template::validate(&destination.url_template)
                    .map_err(|err| format!("{}: {err}", destination.name))?;
            }
            if destination.suggest == destination::SuggestKind::Custom {
                destination::validate_suggest_source(
                    &destination.suggest_url,
                    &destination.suggest_path,
                )
                .map_err(|err| format!("{}: {err}", destination.name))?;
            }
            if destination.disabled {
                continue;
            }
            for trigger in &destination.triggers {
                if !seen.insert(trigger.clone()) {
                    return Err(format!("Trigger !{trigger} is already used"));
                }
            }
        }

        let default_ok = self.destinations.iter().any(|destination| {
            destination.id == self.default_destination_id && !destination.disabled
        });
        if !default_ok {
            self.default_destination_id = self
                .destinations
                .iter()
                .find(|destination| !destination.disabled)
                .map(|destination| destination.id.clone())
                .unwrap_or_else(|| "google".into());
        }
        Ok(())
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let data = serde_json::to_vec_pretty(self).map_err(|err| err.to_string())?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, &data).map_err(|err| err.to_string())?;
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
        // A save interrupted between writing and renaming leaves only the new copy behind.
        let Ok(bytes) = fs::read(path).or_else(|_| fs::read(path.with_extension("json.tmp")))
        else {
            return Self::fresh();
        };
        match serde_json::from_slice::<Self>(&bytes) {
            Ok(mut state) => {
                state.merge_builtins();
                // Violet was the default before Sidebar; anyone who never picked moves over.
                if !state.theme_chosen && state.theme == "violet" {
                    state.theme = default_theme();
                }
                if state.migrate()
                    && let Err(err) = state.save(path)
                {
                    log::error!("couldn't save migrated settings: {err}");
                }
                state
            }
            Err(err) => {
                log::error!("state file could not be read, starting fresh: {err}");
                let backup = path.with_extension("json.bak");
                let _ = fs::copy(path, backup);
                Self::fresh()
            }
        }
    }
}

/// One destination as exported; only the name, triggers and template are required.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedDestination {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub triggers: Vec<String>,
    pub url_template: String,
    #[serde(default = "no_suggestions")]
    pub suggest: destination::SuggestKind,
    #[serde(default)]
    pub suggest_url: String,
    #[serde(default)]
    pub suggest_path: String,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub disabled: Option<bool>,
}

fn no_suggestions() -> destination::SuggestKind {
    destination::SuggestKind::None
}

fn normalize_triggers(triggers: &[String]) -> Result<Vec<String>, String> {
    let mut normalized = Vec::new();
    for trigger in triggers {
        let Some(trigger) = destination::normalize_trigger(trigger) else {
            return Err(format!(
                "Trigger !{} can only use letters, numbers, and hyphens",
                trigger.trim().trim_start_matches('!')
            ));
        };
        if apps::is_scope(&trigger) {
            return Err(format!("!{trigger} is reserved for finding apps"));
        }
        if settings::is_scope(&trigger) {
            return Err(format!("!{trigger} is reserved for finding settings"));
        }
        if files::is_scope(&trigger) {
            return Err(format!("!{trigger} is reserved for finding files"));
        }
        if crate::notes::is_scope(&trigger) {
            return Err(format!("!{trigger} is reserved for notes"));
        }
        if trigger == "clip" {
            return Err("!clip is reserved for clipboard history".into());
        }
        if trigger == "claude" {
            return Err("!claude is reserved for Claude jobs".into());
        }
        if trigger == "type" {
            return Err("!type is reserved for the typing test".into());
        }
        if !normalized.contains(&trigger) {
            normalized.push(trigger);
        }
    }
    if normalized.is_empty() {
        return Err("A destination needs at least one trigger".into());
    }
    Ok(normalized)
}

pub struct AppState {
    inner: Mutex<Persisted>,
    path: PathBuf,
}

impl AppState {
    pub fn load(path: PathBuf) -> Self {
        let persisted = Persisted::load(&path);
        Self {
            inner: Mutex::new(persisted),
            path,
        }
    }

    pub fn snapshot(&self) -> Result<Persisted, String> {
        Ok(self.lock()?.clone())
    }

    pub fn update<F>(&self, mutate: F) -> Result<Persisted, String>
    where
        F: FnOnce(&mut Persisted) -> Result<(), String>,
    {
        let mut guard = self.lock()?;
        // Work on a copy so a failed mutation or save leaves memory matching disk.
        let mut next = guard.clone();
        mutate(&mut next)?;
        next.save(&self.path)?;
        *guard = next.clone();
        Ok(next)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Persisted>, String> {
        self.inner
            .lock()
            .map_err(|_| "Settings were left in a bad state. Restart Zephyr.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::destination::SuggestKind;
    use crate::shortcut::{DEFAULT_SHORTCUT, LEGACY_DEFAULT_SHORTCUT};

    fn custom(id: &str, trigger: &str) -> Destination {
        Destination {
            id: id.into(),
            name: "Example".into(),
            triggers: vec![trigger.into()],
            url_template: "https://example.com/?q={query}".into(),
            suggest: SuggestKind::None,
            suggest_url: String::new(),
            suggest_path: String::new(),
            pinned: true,
            builtin: false,
            disabled: false,
            kind: Default::default(),
        }
    }

    #[test]
    fn cannot_turn_off_every_destination() {
        let mut state = Persisted::fresh();
        for destination in &mut state.destinations {
            destination.disabled = true;
        }
        state.destinations[0].disabled = false;
        let mut last = state.destinations[0].clone();
        last.disabled = true;
        assert!(state.upsert(last).is_err());
    }

    #[test]
    fn duplicate_triggers_are_rejected_and_rolled_back() {
        let mut state = Persisted::fresh();
        let before = state.destinations.len();
        let result = state.upsert(custom("custom-1", "g"));
        assert!(result.is_err());
        assert_eq!(state.destinations.len(), before);
    }

    #[test]
    fn builtins_cannot_be_deleted() {
        let mut state = Persisted::fresh();
        assert!(state.remove("google").is_err());
    }

    #[test]
    fn legacy_default_shortcut_moves_to_the_platform_default() {
        let mut state = Persisted::fresh();
        state.version = 1;
        state.summon_shortcut = LEGACY_DEFAULT_SHORTCUT.into();
        assert!(state.migrate());
        assert_eq!(state.summon_shortcut, DEFAULT_SHORTCUT);
        assert_eq!(state.version, 2);
        assert!(!state.migrate());
    }

    #[test]
    fn a_custom_shortcut_survives_the_default_change() {
        let mut state = Persisted::fresh();
        state.version = 1;
        state.summon_shortcut = "ctrl+alt+k".into();
        assert!(state.migrate());
        assert_eq!(state.summon_shortcut, "ctrl+alt+k");
        assert_eq!(state.version, 2);
    }

    #[test]
    fn a_partial_file_keeps_the_fields_it_has() {
        let state: Persisted =
            serde_json::from_str(r#"{"summonShortcut":"ctrl+alt+k","launchAtStartup":true}"#)
                .unwrap();
        assert_eq!(state.summon_shortcut, "ctrl+alt+k");
        assert!(state.launch_at_startup);
        assert_eq!(state.version, 1);
        assert!(!state.destinations.is_empty());
        assert!(state.history.is_empty());
    }

    #[test]
    fn a_rejected_update_leaves_memory_untouched() {
        let dir = std::env::temp_dir().join(format!("zephyr-state-{}", std::process::id()));
        let app_state = AppState::load(dir.join("state.json"));
        let before = app_state.snapshot().unwrap();
        let result = app_state.update(|persisted| {
            persisted.history.clear();
            persisted.upsert(custom("custom-1", "g"))
        });
        assert!(result.is_err());
        assert_eq!(
            app_state.snapshot().unwrap().destinations,
            before.destinations
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_builtins_are_added_without_resetting_edits() {
        let mut state = Persisted::fresh();
        state.destinations[0].name = "Search".into();
        state
            .destinations
            .retain(|destination| destination.id != "youtube");
        state.merge_builtins();
        assert_eq!(state.destinations[0].name, "Search");
        assert!(
            state
                .destinations
                .iter()
                .any(|destination| destination.id == "youtube")
        );
    }

    #[test]
    fn imports_new_and_existing_destinations_all_or_nothing() {
        let mut state = Persisted::fresh();
        let json = r#"[
            {"id": "google", "name": "Google", "triggers": ["g"], "urlTemplate": "https://www.google.com/search?q={query}", "suggest": "google", "pinned": false},
            {"name": "Arxiv", "triggers": ["arxiv"], "urlTemplate": "https://arxiv.org/a/{query}"}
        ]"#;
        let incoming: Vec<ImportedDestination> = serde_json::from_str(json).unwrap();
        assert_eq!(state.import(incoming).unwrap(), 2);
        let google = state
            .destinations
            .iter()
            .find(|d| d.id == "google")
            .unwrap();
        assert_eq!(google.triggers, vec!["g"]);
        assert!(!google.pinned && google.builtin);
        assert!(
            state
                .destinations
                .iter()
                .any(|d| d.name == "Arxiv" && d.id.starts_with("custom-"))
        );

        let clash: Vec<ImportedDestination> = serde_json::from_str(
            r#"[{"name": "Again", "triggers": ["arxiv"], "urlTemplate": "https://x.dev/{query}"}]"#,
        )
        .unwrap();
        let before = state.destinations.len();
        assert!(state.import(clash).is_err());
        assert_eq!(state.destinations.len(), before);
    }
}
