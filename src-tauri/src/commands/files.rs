use std::time::UNIX_EPOCH;

use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use super::{blocking, record, save};
use crate::clipboard;
use crate::files;
use crate::query::DispatchOutcome;
use crate::state::{AppState, Persisted};
use crate::util::now_secs;
use crate::window;

#[tauri::command]
pub async fn open_file(app: AppHandle, path: String) -> Result<DispatchOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        open_file_path(&app, &app.state::<AppState>(), &path)
    })
    .await
    .map_err(|err| err.to_string())?
}

/// Only paths the bar could have shown are opened: indexed ones and ones opened before.
fn known_file(state: &AppState, path: &str) -> Result<(), String> {
    let opened_before = state
        .snapshot()?
        .file_opens
        .iter()
        .any(|open| open.app_id == path);
    if opened_before || files::index().contains(path) {
        Ok(())
    } else {
        Err("That file isn't in Zephyr's file index.".into())
    }
}

pub(super) fn open_file_path(
    app: &AppHandle,
    state: &AppState,
    path: &str,
) -> Result<DispatchOutcome, String> {
    known_file(state, path)?;
    if !std::path::Path::new(path).exists() {
        return Err("That file was moved or deleted.".into());
    }
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|err| format!("Couldn't open it: {err}"))?;
    window::hide_bar(app);
    record(app, state, "the open", |persisted| {
        persisted.record_file_open(path, now_secs())
    });
    Ok(DispatchOutcome::FileOpened {
        path: path.to_string(),
    })
}

#[tauri::command]
pub async fn reveal_file(app: AppHandle, path: String) -> Result<(), String> {
    blocking(move || {
        known_file(&app.state::<AppState>(), &path)?;
        app.opener()
            .reveal_item_in_dir(&path)
            .map_err(|err| format!("Couldn't show it: {err}"))?;
        window::hide_bar(&app);
        Ok(())
    })
    .await
}

#[tauri::command]
pub fn file_index_status() -> files::Status {
    files::index().status()
}

#[tauri::command]
pub fn rebuild_file_index() {
    files::index().rescan();
}

#[tauri::command]
pub async fn save_file_folders(
    app: AppHandle,
    roots: Option<Vec<String>>,
    excludes: Vec<String>,
) -> Result<Persisted, String> {
    let snapshot = save(app.clone(), |persisted| {
        persisted.set_file_folders(roots, excludes)
    })
    .await?;
    files::index().configure(snapshot.file_config(), file_cache_path(&app));
    Ok(snapshot)
}

/// Lets file search into Desktop, Documents and Downloads; macOS asks for each as the index
/// first reads it. Called when file search is first used, or from Settings › Permissions.
#[tauri::command]
pub async fn allow_protected_folders(app: AppHandle) -> Result<Persisted, String> {
    let snapshot = save(app.clone(), |persisted| {
        persisted.protected_folders = true;
        Ok(())
    })
    .await?;
    files::index().configure(snapshot.file_config(), file_cache_path(&app));
    Ok(snapshot)
}

pub fn file_cache_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_cache_dir()
        .ok()
        .map(|dir| dir.join("files.idx"))
}

/// Shows the system folder picker; `None` when cancelled.
#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Choose a folder")
            .blocking_pick_folder()
    })
    .await
    .map_err(|err| err.to_string())?;
    Ok(picked
        .and_then(|path| path.into_path().ok())
        .map(|path| path.to_string_lossy().into_owned()))
}

/// What a drag out of the bar carries: the files, and a picture for the pointer.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DragOut {
    pub files: Vec<String>,
    pub icon: String,
}

/// Gets a file result (`path`, which must be in the file index) or a clipboard history item
/// (`clip`) ready to drag into another app, and keeps the bar up until the drag ends.
// Async: drawing the file's icon and writing the copies takes a moment.
#[tauri::command]
pub async fn drag_prepare(
    app: AppHandle,
    path: Option<String>,
    clip: Option<u64>,
) -> Result<DragOut, String> {
    blocking(move || {
        let dir = app
            .path()
            .app_cache_dir()
            .map_err(|err| err.to_string())?
            .join("drag");
        let files = match (path, clip) {
            (Some(path), _) => {
                known_file(&app.state::<AppState>(), &path)?;
                vec![path]
            }
            // Earlier drags' copies are done with: whatever they were dropped on has read them.
            (None, Some(id)) => {
                let _ = std::fs::remove_dir_all(&dir);
                clipboard::drag_files(id, &dir)?
            }
            (None, None) => return Err("Nothing to drag".into()),
        };
        let first = files.first().ok_or("Nothing to drag")?;
        let png = crate::icons::file_icon_png(first, 64).ok_or("Couldn't draw the drag picture")?;
        std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
        let icon = dir.join("pointer.png");
        std::fs::write(&icon, png).map_err(|err| err.to_string())?;
        window::begin_drag_out();
        Ok(DragOut {
            files,
            icon: icon.to_string_lossy().into_owned(),
        })
    })
    .await
}

/// The drag ended: dropped somewhere hides the bar, cancelled keeps it.
#[tauri::command]
pub fn drag_finished(app: AppHandle, dropped: bool) {
    window::end_drag_out(&app, dropped);
}

/// What the bar's preview pane shows for a file result.
#[derive(serde::Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FilePreview {
    Image {
        url: String,
        size: u64,
        modified: i64,
    },
    Text {
        text: String,
        /// More lines follow than are shown.
        truncated: bool,
        size: u64,
        modified: i64,
    },
    Info {
        size: u64,
        modified: i64,
        folder: bool,
    },
}

/// A look at a file result without opening it: a picture, the first lines of a text file,
/// or its size and date. Only for files in the file index.
#[tauri::command]
pub async fn file_preview(state: State<'_, AppState>, path: String) -> Result<FilePreview, String> {
    known_file(&state, &path)?;
    tauri::async_runtime::spawn_blocking(move || preview_of(std::path::Path::new(&path)))
        .await
        .map_err(|err| err.to_string())?
}

fn preview_of(path: &std::path::Path) -> Result<FilePreview, String> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use std::io::Read;

    const IMAGE_LIMIT: u64 = 15 * 1024 * 1024;
    const TEXT_PEEK: u64 = 256 * 1024;
    const LINES: usize = 200;

    let meta = std::fs::metadata(path).map_err(|err| err.to_string())?;
    let size = meta.len();
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    if meta.is_dir() {
        return Ok(FilePreview::Info {
            size: 0,
            modified,
            folder: true,
        });
    }
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let image_type = match extension.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "ico" => Some("image/x-icon"),
        "svg" => Some("image/svg+xml"),
        _ => None,
    };
    if let Some(mime) = image_type
        && size <= IMAGE_LIMIT
    {
        let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
        return Ok(FilePreview::Image {
            url: format!("data:{mime};base64,{}", STANDARD.encode(bytes)),
            size,
            modified,
        });
    }

    // Anything that reads as text: no NUL bytes in its first part.
    let mut peek = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(TEXT_PEEK).read_to_end(&mut peek))
        .map_err(|err| err.to_string())?;
    if !peek.is_empty() && !peek.contains(&0) {
        let text = String::from_utf8_lossy(&peek);
        let mut lines: Vec<&str> = text.lines().take(LINES + 1).collect();
        let truncated = lines.len() > LINES || size > TEXT_PEEK;
        lines.truncate(LINES);
        return Ok(FilePreview::Text {
            text: lines.join("\n"),
            truncated,
            size,
            modified,
        });
    }
    Ok(FilePreview::Info {
        size,
        modified,
        folder: false,
    })
}

/// macOS: opens Quick Look on a file result (Space-style preview), from the file index only.
#[tauri::command]
pub fn quick_look(state: State<'_, AppState>, path: String) -> Result<(), String> {
    known_file(&state, &path)?;
    #[cfg(target_os = "macos")]
    {
        crate::util::spawn_detached(
            std::process::Command::new("qlmanage")
                .args(["-p", &path])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null()),
        )
        .map_err(|err| err.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Quick Look is only on macOS".into())
    }
}
