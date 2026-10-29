use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use super::{blocking, save};
use crate::state::Persisted;
use crate::window;

/// CPU, memory, disk and network for the bar's footer; the bar asks about once a second
/// while it is showing.
#[tauri::command]
pub async fn system_stats() -> Result<crate::stats::Stats, String> {
    tauri::async_runtime::spawn_blocking(crate::stats::sample)
        .await
        .map_err(|err| err.to_string())
}

/// One tick of System Resources: totals, the last minute, and the rows that can show.
#[tauri::command]
pub async fn resources_snapshot(
    sort: String,
    filter: String,
    limit: usize,
) -> Result<crate::resources::Snapshot, String> {
    crate::stats::set_resources_open(true);
    tauri::async_runtime::spawn_blocking(move || {
        crate::resources::snapshot(&sort, &filter, limit.clamp(1, 200))
    })
    .await
    .map_err(|err| err.to_string())
}

/// The view closed; its process table goes after half a minute unless it reopens.
#[tauri::command]
pub fn resources_closed() {
    crate::stats::set_resources_open(false);
    crate::timer::after(std::time::Duration::from_secs(31), crate::resources::idle);
}

#[tauri::command]
pub async fn resources_quit(name: String, pids: Vec<u32>, force: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::resources::quit(&name, &pids, force))
        .await
        .map_err(|err| err.to_string())?
}

/// Quits Finder, the Dock or Explorer so they start fresh.
#[tauri::command]
pub async fn resources_relaunch(name: String, pids: Vec<u32>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::resources::relaunch(&name, &pids))
        .await
        .map_err(|err| err.to_string())?
}

/// Brings an app forward and hides the bar; false when it has no windows to show.
#[tauri::command]
pub fn resources_switch(app: AppHandle, pids: Vec<u32>) -> bool {
    let switched = crate::resources::switch_to(&pids);
    if switched {
        window::hide_bar(&app);
    }
    switched
}

#[tauri::command]
pub async fn resources_reveal(app: AppHandle, path: String) -> Result<(), String> {
    blocking(move || {
        if path.is_empty() || !std::path::Path::new(&path).exists() {
            return Err("Its file isn't there anymore".into());
        }
        app.opener()
            .reveal_item_in_dir(&path)
            .map_err(|err| format!("Couldn't show it: {err}"))?;
        window::hide_bar(&app);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn save_show_stats(app: AppHandle, show: bool) -> Result<Persisted, String> {
    save(app, move |persisted| {
        persisted.show_stats = show;
        Ok(())
    })
    .await
}

/// Show info for one Resources row.
#[tauri::command]
pub async fn resources_detail(key: String, pids: Vec<u32>) -> Option<crate::resources::Detail> {
    tauri::async_runtime::spawn_blocking(move || crate::resources::detail(&key, &pids))
        .await
        .ok()
        .flatten()
}

/// The CPU drill-in: each logical processor over the last 15 minutes.
#[tauri::command]
pub async fn resources_cores() -> Result<crate::stats::CoreHeat, String> {
    tauri::async_runtime::spawn_blocking(crate::stats::core_heat)
        .await
        .map_err(|err| err.to_string())
}

/// The Memory drill-in.
#[tauri::command]
pub async fn resources_memory() -> Option<crate::resources::MemoryBreakdown> {
    tauri::async_runtime::spawn_blocking(crate::resources::memory_breakdown)
        .await
        .ok()
        .flatten()
}

/// Turns the Windows CPU temperature reading on or off. Either way Windows asks for
/// permission once (cputemp.rs).
#[tauri::command]
pub async fn save_cpu_temp(app: AppHandle, enabled: bool) -> Result<Persisted, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if enabled {
            crate::cputemp::enable()
        } else {
            crate::cputemp::disable()
        }
    })
    .await
    .map_err(|err| err.to_string())??;
    save(app, move |persisted| {
        persisted.cpu_temp = enabled;
        Ok(())
    })
    .await
}

/// Whether PawnIO is installed, the helper's latest reading, and whether its copy is missing
/// or outdated, so it needs setting up again.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuTempStatus {
    pawnio: bool,
    celsius: Option<f32>,
    needs_setup: bool,
}

#[tauri::command]
pub fn cpu_temp_status() -> CpuTempStatus {
    CpuTempStatus {
        pawnio: crate::cputemp::pawnio_installed(),
        celsius: crate::cputemp::read(),
        needs_setup: crate::cputemp::needs_setup(),
    }
}
