use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use super::{blocking, publish};
use crate::shell::{self, ShellEvent, ShellInfo};
use crate::state::{AppState, Persisted};
use crate::window;

/// Runs a command typed in the bar after `>`; output streams through `on_event`.
#[tauri::command]
pub async fn shell_run(
    app: AppHandle,
    command: String,
    dir: Option<String>,
    on_event: Channel<ShellEvent>,
) -> Result<(), String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let settings = state.snapshot()?.shell.clone();
        shell::run(&settings, &command, dir.as_deref(), move |event| {
            let _ = on_event.send(event);
        })?;
        let snapshot = state.update(|persisted| {
            persisted.shell.record(&command);
            Ok(())
        })?;
        publish(&app, snapshot).map(|_| ())
    })
    .await
}

#[tauri::command]
pub fn shell_stop(id: u64) {
    shell::stop(id);
}

#[tauri::command]
pub async fn shell_open_terminal(
    app: AppHandle,
    command: String,
    dir: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let settings = state.snapshot()?.shell.clone();
        shell::open_in_terminal(&settings, &command, dir.as_deref())?;
        if !command.trim().is_empty() {
            let snapshot = state.update(|persisted| {
                persisted.shell.record(&command);
                Ok(())
            })?;
            publish(&app, snapshot)?;
        }
        window::hide_bar(&app);
        Ok(())
    })
    .await
    .map_err(|err| err.to_string())?
}

/// Async because finding WSL or Git Bash reads the registry and looks on disk on Windows,
/// which would otherwise hold up the main thread while Settings opens.
#[tauri::command]
pub async fn shell_info(state: State<'_, AppState>) -> Result<ShellInfo, String> {
    let settings = state.snapshot()?.shell.clone();
    tauri::async_runtime::spawn_blocking(move || shell::info(&settings))
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn save_shell_program(
    app: AppHandle,
    state: State<'_, AppState>,
    program: String,
) -> Result<Persisted, String> {
    let snapshot = state.update(|persisted| persisted.shell.set_program(&program))?;
    publish(&app, snapshot)
}
