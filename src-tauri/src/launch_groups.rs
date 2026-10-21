//! Launch groups: several apps, links and folders opened in one go ("Coding", "Gaming"),
//! from their row in the bar or a hotkey. Apps already running come forward rather than
//! starting again; opens are spaced a little so nothing stalls, and a notification says
//! how it went.

use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::state::{AppState, LaunchGroup};

/// Checks groups before they're saved: a name, and items of a known kind.
pub fn validate(groups: &[LaunchGroup]) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    for group in groups {
        if group.id.trim().is_empty() || !ids.insert(group.id.as_str()) {
            return Err("Each group needs its own id".into());
        }
        if group.name.trim().is_empty() {
            return Err("Give every group a name".into());
        }
        if group.name.chars().count() > 40 {
            return Err(format!("{} is too long a name", group.name));
        }
        for item in &group.items {
            if !matches!(item.kind.as_str(), "app" | "url" | "folder") || item.value.is_empty() {
                return Err(format!("{} has an item Zephyr can't open", group.name));
            }
            if item.kind == "url"
                && !(item.value.starts_with("http://") || item.value.starts_with("https://"))
            {
                return Err(format!(
                    "{} must start with http:// or https://",
                    item.value
                ));
            }
        }
    }
    Ok(())
}

/// Opens everything in the group, in order, off the main thread, then reports.
pub fn launch(app: &AppHandle, id: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let group = state
        .snapshot()?
        .launch_groups
        .into_iter()
        .find(|group| group.id == id)
        .ok_or("That group no longer exists")?;
    crate::window::hide_bar(app);
    let app = app.clone();
    std::thread::spawn(move || {
        let total = group.items.len();
        let mut failed: Vec<String> = Vec::new();
        for (index, item) in group.items.iter().enumerate() {
            if index > 0 {
                std::thread::sleep(Duration::from_millis(150));
            }
            let result = match item.kind.as_str() {
                "app" => crate::commands::launch_quietly(&app, &item.value),
                "url" => app
                    .opener()
                    .open_url(&item.value, None::<&str>)
                    .map_err(|err| err.to_string()),
                _ => app
                    .opener()
                    .open_path(&item.value, None::<&str>)
                    .map_err(|err| err.to_string()),
            };
            if let Err(err) = result {
                log::error!(
                    "launch group {}: couldn't open {}: {err}",
                    group.name,
                    item.label
                );
                failed.push(item.label.clone());
            }
        }
        let opened = total - failed.len();
        let body = if failed.is_empty() {
            format!("Opened {opened} of {total}")
        } else {
            format!(
                "Opened {opened} of {total}. Couldn't open {}.",
                failed.join(", ")
            )
        };
        notify(&app, &group.name, &body);
        log::info!("launched group {}: {body}", group.name);
    });
    Ok(())
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    if let Err(err) = app.notification().builder().title(title).body(body).show() {
        log::error!("couldn't show a notification: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::GroupItem;

    fn group(id: &str, name: &str, items: Vec<GroupItem>) -> LaunchGroup {
        LaunchGroup {
            id: id.into(),
            name: name.into(),
            icon: String::new(),
            items,
        }
    }

    #[test]
    fn groups_need_names_and_openable_items() {
        let app = GroupItem {
            kind: "app".into(),
            value: "/Applications/Steam.app".into(),
            label: "Steam".into(),
        };
        assert!(validate(&[group("a", "Gaming", vec![app.clone()])]).is_ok());
        assert!(validate(&[group("a", " ", vec![])]).is_err());
        assert!(validate(&[group("a", "A", vec![]), group("a", "B", vec![])]).is_err());
        let bad = GroupItem {
            kind: "url".into(),
            value: "javascript:alert(1)".into(),
            label: "x".into(),
        };
        assert!(validate(&[group("a", "Web", vec![bad])]).is_err());
    }
}
