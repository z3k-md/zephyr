use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::apps::{self, LaunchEntry};
use crate::destination::{self, Destination};
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
    pub default_destination_id: String,
    pub destinations: Vec<Destination>,
    pub history: Vec<HistoryEntry>,
    pub launches: Vec<LaunchEntry>,
    /// Lowercased texts the user sent to the web even though they matched an app.
    pub app_overrides: Vec<String>,
}

const SCHEMA_VERSION: u32 = 2;

fn schema_version() -> u32 {
    SCHEMA_VERSION
}

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
            version: schema_version(),
            summon_shortcut: DEFAULT_SHORTCUT.to_string(),
            launch_at_startup: false,
            default_destination_id: "google".into(),
            destinations: destination::builtins(),
            history: Vec::new(),
            launches: Vec::new(),
            app_overrides: Vec::new(),
        }
    }

    pub fn merge_builtins(&mut self) {
        for builtin in destination::builtins() {
            if self
                .destinations
                .iter()
                .any(|destination| destination.id == builtin.id)
            {
                continue;
            }
            self.destinations.push(builtin);
        }
    }

    pub fn record(&mut self, query: &str, destination_id: &str, now: i64) {
        history::record(&mut self.history, query, destination_id, now);
    }

    pub fn record_launch(&mut self, app_id: &str, now: i64) {
        apps::record_launch(&mut self.launches, app_id, now);
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
            existing.pinned = incoming.pinned;
            existing.disabled = incoming.disabled;
            existing.builtin = builtin;
        } else {
            if incoming.builtin || !incoming.id.starts_with("custom-") {
                return Err("New destinations need a custom id".into());
            }
            incoming.builtin = false;
            next.destinations.push(incoming);
        }
        next.ensure_invariants()?;
        *self = next;
        Ok(())
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
            destination::build_url(&destination.url_template, "probe")?;
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
        if fs::rename(&tmp, path).is_err() {
            let _ = fs::remove_file(path);
            fs::rename(&tmp, path).map_err(|err| err.to_string())?;
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Self {
        let Ok(bytes) = fs::read(path) else {
            return Self::fresh();
        };
        match serde_json::from_slice::<Self>(&bytes) {
            Ok(mut state) => {
                state.merge_builtins();
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
    pub inner: Mutex<Persisted>,
    pub path: PathBuf,
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
            pinned: true,
            builtin: false,
            disabled: false,
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
}
