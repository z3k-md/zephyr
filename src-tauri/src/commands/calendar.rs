use tauri::AppHandle;

use super::{blocking, save, save_now};
use crate::calendar::{self, Feed};
use crate::state::Persisted;

/// Events in memory, and each feed's last update or error.
#[tauri::command]
pub fn calendar_agenda() -> calendar::Agenda {
    calendar::agenda()
}

/// Adds a feed. The link goes in the keychain; settings keep the name, color and order.
#[tauri::command]
pub async fn calendar_add_feed(
    app: AppHandle,
    name: String,
    url: String,
) -> Result<Persisted, String> {
    let (name, url) = calendar::validate_link(&name, &url)?;
    let snapshot = blocking(move || {
        let id = uuid::Uuid::new_v4().to_string();
        crate::secrets::set(calendar::SECRET_SERVICE, &id, Some(&url))?;
        let feed_id = id.clone();
        match save_now(&app, move |persisted| {
            let color = calendar::next_color(&persisted.calendar_feeds);
            let order = persisted
                .calendar_feeds
                .iter()
                .map(|feed| feed.order)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            persisted.calendar_feeds.push(Feed {
                id: feed_id,
                name,
                color,
                order,
            });
            Ok(())
        }) {
            Ok(snapshot) => Ok(snapshot),
            Err(err) => {
                let _ = crate::secrets::set(calendar::SECRET_SERVICE, &id, None);
                Err(err)
            }
        }
    })
    .await?;
    calendar::refresh();
    Ok(snapshot)
}

#[tauri::command]
pub async fn calendar_rename_feed(
    app: AppHandle,
    id: String,
    name: String,
) -> Result<Persisted, String> {
    let name = calendar::validate_name(&name)?;
    let snapshot = save(app, move |persisted| {
        let feed = persisted
            .calendar_feeds
            .iter_mut()
            .find(|feed| feed.id == id)
            .ok_or("That calendar is gone")?;
        feed.name = name;
        Ok(())
    })
    .await?;
    calendar::refresh();
    Ok(snapshot)
}

#[tauri::command]
pub async fn calendar_remove_feed(app: AppHandle, id: String) -> Result<Persisted, String> {
    let snapshot = save(app.clone(), {
        let id = id.clone();
        move |persisted| {
            let before = persisted.calendar_feeds.len();
            persisted.calendar_feeds.retain(|feed| feed.id != id);
            if persisted.calendar_feeds.len() == before {
                return Err("That calendar is gone".into());
            }
            Ok(())
        }
    })
    .await?;
    let _ = blocking(move || crate::secrets::set(calendar::SECRET_SERVICE, &id, None)).await;
    calendar::refresh();
    Ok(snapshot)
}

/// Moves a feed up or down in the list.
#[tauri::command]
pub async fn calendar_move_feed(app: AppHandle, id: String, up: bool) -> Result<Persisted, String> {
    let snapshot = save(app, move |persisted| {
        calendar::move_feed(&mut persisted.calendar_feeds, &id, up)
    })
    .await?;
    calendar::refresh();
    Ok(snapshot)
}
