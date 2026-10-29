use tauri::AppHandle;

use super::save;
use crate::state::Persisted;

/// The connection's grade, numbers and sparkline, for the bar's network card and footer.
#[tauri::command]
pub async fn network_summary() -> Option<crate::net::Summary> {
    tauri::async_runtime::spawn_blocking(crate::net::summary)
        .await
        .ok()
        .flatten()
}

/// The last 15 minutes in detail, for the Network view.
#[tauri::command]
pub async fn network_history() -> Option<crate::net::History> {
    tauri::async_runtime::spawn_blocking(crate::net::history)
        .await
        .ok()
        .flatten()
}

/// The address the internet sees, fetched while the IP card or Network view shows.
#[tauri::command]
pub async fn network_public_ip() -> Option<String> {
    crate::net::ip::public().await
}

#[tauri::command]
pub async fn save_watch_connection(app: AppHandle, watch: bool) -> Result<Persisted, String> {
    let snapshot = save(app, move |persisted| {
        persisted.watch_connection = watch;
        Ok(())
    })
    .await?;
    crate::net::set_enabled(watch);
    Ok(snapshot)
}

/// Runs a speed test, sending "speed-test" progress events, and keeps the result.
#[tauri::command]
pub async fn speed_test(app: AppHandle) -> Result<crate::net::speed::SpeedResult, String> {
    let result = crate::net::speed::run(&app).await?;
    let saved = result.clone();
    save(app, move |persisted| {
        persisted.last_speed = Some(saved);
        Ok(())
    })
    .await?;
    Ok(result)
}

#[tauri::command]
pub fn speed_test_cancel() {
    crate::net::speed::cancel();
}

/// One of this computer's addresses, for the Network view's chips and the IP card.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkAddress {
    value: String,
    /// "Wi-Fi · IPv4".
    detail: String,
    icon: &'static str,
}

/// This computer's addresses, the one the internet is reached through first.
#[tauri::command]
pub fn network_addresses() -> Vec<NetworkAddress> {
    crate::net::ip::local()
        .into_iter()
        .map(|address| NetworkAddress {
            icon: address.link.glyph(),
            value: address.value,
            detail: address.detail,
        })
        .collect()
}
