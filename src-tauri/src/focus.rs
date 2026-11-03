//! What a focus session puts aside: its default length, the break that follows it, apps
//! whose windows leave the screen, and sites the browser extension blocks. The session
//! itself is a countdown timer (countdown.rs). Nothing here quits an app.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::countdown::MAX_SECONDS;

/// Seconds for "focus deep work" when no length is typed.
pub fn default_seconds() -> i64 {
    25 * 60
}

/// A plain "Break" timer this long starts when a session ends, unless it's set to 0.
pub fn default_break_seconds() -> i64 {
    5 * 60
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FocusSettings {
    /// Seconds for "focus deep work" when no length is typed.
    #[serde(default = "default_seconds")]
    pub seconds: i64,
    /// A plain "Break" timer this long starts when a session ends; 0 starts none.
    #[serde(default = "default_break_seconds")]
    pub break_seconds: i64,
    /// Apps minimized (Windows) or hidden (macOS) when a session starts.
    pub apps: Vec<FocusApp>,
    /// Hosts the browser extension blocks while a session runs ("x.com", "youtube.com").
    pub sites: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct FocusApp {
    pub id: String,
    pub name: String,
}

impl Default for FocusSettings {
    fn default() -> Self {
        Self {
            seconds: default_seconds(),
            break_seconds: default_break_seconds(),
            apps: Vec::new(),
            sites: Vec::new(),
        }
    }
}

impl FocusSettings {
    /// Drops blank apps and sites that aren't hosts, and rejects a length the timer won't run.
    pub fn normalized(mut self) -> Result<Self, String> {
        if !(1..=MAX_SECONDS).contains(&self.seconds) {
            return Err("A focus session runs from a second to a week".into());
        }
        if !(0..=MAX_SECONDS).contains(&self.break_seconds) {
            return Err("A break runs up to a week".into());
        }
        let mut apps = Vec::new();
        for app in self.apps {
            let id = app.id.trim().to_string();
            if id.is_empty() || apps.iter().any(|kept: &FocusApp| kept.id == id) {
                continue;
            }
            let name: String = app.name.trim().chars().take(80).collect();
            apps.push(FocusApp { id, name });
            if apps.len() == 40 {
                break;
            }
        }
        self.apps = apps;
        let mut sites = Vec::new();
        for site in self.sites {
            let Some(host) = host_of(&site) else {
                continue;
            };
            if !sites.contains(&host) {
                sites.push(host);
            }
            if sites.len() == 100 {
                break;
            }
        }
        self.sites = sites;
        Ok(self)
    }
}

/// "https://www.YouTube.com/feed" and "youtube.com" are "youtube.com", so a block covers
/// the site and its subdomains. None when it isn't a host.
pub fn host_of(text: &str) -> Option<String> {
    let text = text.trim().to_lowercase();
    if text.is_empty() || text.chars().any(char::is_whitespace) {
        return None;
    }
    let with_scheme = if text.contains("://") {
        text
    } else {
        format!("https://{text}")
    };
    let host = url::Url::parse(&with_scheme)
        .ok()?
        .host_str()?
        .trim_end_matches('.')
        .to_string();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    if host.is_empty() || (!host.contains('.') && host != "localhost") {
        return None;
    }
    Some(host)
}

fn settings_of(app: &AppHandle) -> FocusSettings {
    app.state::<crate::state::AppState>()
        .snapshot()
        .map(|snapshot| snapshot.focus.clone())
        .unwrap_or_default()
}

/// The break length saved in settings; 0 starts none.
pub fn break_seconds(app: &AppHandle) -> i64 {
    settings_of(app).break_seconds.max(0)
}

/// Hides the chosen apps and tells the extension how long to block sites. Called when a
/// session starts, after the timer is saved.
pub fn begin(app: &AppHandle) {
    let settings = settings_of(app);
    for listed in &settings.apps {
        crate::running::tuck(&crate::apps::App {
            id: listed.id.clone(),
            name: listed.name.clone(),
        });
    }
    publish(app);
}

/// Sends the extension the sites of the focus session that's running, or none when there
/// isn't one. A session that ended and one that replaced it both land here, so a newer
/// session isn't cleared by the one it replaced.
pub fn publish(app: &AppHandle) {
    let now = chrono::Local::now().timestamp();
    let Some(snapshot) = app.state::<crate::state::AppState>().snapshot().ok() else {
        crate::browser::set_focus_sites(Vec::new(), 0);
        return;
    };
    if let Some(timer) = snapshot
        .timers
        .iter()
        .find(|timer| timer.focus && timer.at > now)
    {
        crate::browser::set_focus_sites(snapshot.focus.sites.clone(), timer.at);
    } else {
        crate::browser::set_focus_sites(Vec::new(), 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_are_stored_as_hosts() {
        assert_eq!(host_of("youtube.com"), Some("youtube.com".into()));
        assert_eq!(
            host_of("https://www.YouTube.com/feed"),
            Some("youtube.com".into())
        );
        assert_eq!(host_of("x.com"), Some("x.com".into()));
        assert_eq!(host_of("localhost"), Some("localhost".into()));
        assert_eq!(host_of("not a site"), None);
        assert_eq!(host_of("youtube"), None);
    }

    #[test]
    fn lengths_have_to_be_ones_a_timer_can_run() {
        let settings = FocusSettings {
            sites: vec!["https://www.x.com/home".into(), "x.com".into()],
            apps: vec![
                FocusApp {
                    id: " Chrome ".into(),
                    name: " Google Chrome ".into(),
                },
                FocusApp {
                    id: "Chrome".into(),
                    name: "again".into(),
                },
            ],
            ..FocusSettings::default()
        };
        let saved = settings.normalized().unwrap();
        assert_eq!(saved.sites, vec!["x.com".to_string()]);
        assert_eq!(saved.apps.len(), 1);
        assert_eq!(saved.apps[0].id, "Chrome");
        assert_eq!(saved.apps[0].name, "Google Chrome");

        let too_long = FocusSettings {
            seconds: MAX_SECONDS + 1,
            ..FocusSettings::default()
        };
        assert!(too_long.normalized().is_err());
        let no_break = FocusSettings {
            break_seconds: 0,
            ..FocusSettings::default()
        };
        assert!(no_break.normalized().is_ok());
    }
}
