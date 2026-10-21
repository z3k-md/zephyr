//! The windows open right now, so an app's row switches to it when it's already running, and
//! typing part of a window's title (a document, a browser tab) finds that window. They're
//! listed each time the bar is summoned, never on a timer.
//!
//! Windows only for now; on macOS the list stays empty, so app rows launch as before.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, RwLock};

use serde::Serialize;

use crate::apps::App;
use crate::suggest::{SuggestResponse, Suggestion};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpenWindow {
    /// The window handle, as a number the page can hand back.
    pub handle: i64,
    pub title: String,
    /// The full path of the program that owns it.
    pub exe: String,
    /// The window's AppUserModelID when it has one (packaged apps, Chrome, Terminal).
    pub app_id: String,
}

/// A window listed under its app's row, for the action panel.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WindowRef {
    pub handle: i64,
    pub title: String,
}

fn open() -> &'static RwLock<Vec<OpenWindow>> {
    static OPEN: OnceLock<RwLock<Vec<OpenWindow>>> = OnceLock::new();
    OPEN.get_or_init(Default::default)
}

/// Lists the open windows again, off the calling thread. Called as the bar is summoned, so the
/// list is in front-to-back order from before the bar took the front.
pub fn refresh() {
    std::thread::spawn(|| {
        let found = platform::list();
        if let Ok(mut windows) = open().write() {
            *windows = found;
        }
    });
}

pub fn all() -> Vec<OpenWindow> {
    open()
        .read()
        .map(|windows| windows.clone())
        .unwrap_or_default()
}

/// Brings a window to the front (restoring it when minimized).
pub fn focus(handle: i64) -> bool {
    platform::focus(handle)
}

/// Brings a process's first window to the front (Windows; elsewhere apps are activated).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn focus_process(pid: u32) -> bool {
    platform::of_process(pid)
        .first()
        .is_some_and(|handle| platform::focus(*handle))
}

/// Asks each of a process's windows to close, as its close button would.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn close_process_windows(pid: u32) -> bool {
    let handles = platform::of_process(pid);
    !handles.is_empty() && handles.into_iter().all(platform::close)
}

/// Asks a window to close, as its own close button would; the app may still ask to save.
pub fn close(handle: i64) -> bool {
    platform::close(handle)
}

/// The windows that belong to `app`, front to back.
pub fn of_app<'a>(app: &App, windows: &'a [OpenWindow]) -> Vec<&'a OpenWindow> {
    let exe = program_of(&app.id);
    windows
        .iter()
        .filter(|window| belongs(app, exe.as_deref(), window))
        .collect()
}

/// Whether a window is one of `app`'s: by the AppUserModelID it declares, by the program the
/// app launches (or another program in that program's folder: Steam's windows belong to
/// steamwebhelper.exe beside Steam.exe), or by a program named like the app (GitHub Desktop's
/// GitHubDesktop.exe, whose Start entry is only an id).
pub fn belongs(app: &App, app_exe: Option<&str>, window: &OpenWindow) -> bool {
    let id = window.app_id.as_str();
    let app_id = app.id.as_str();
    if !id.is_empty()
        && (id.eq_ignore_ascii_case(app_id)
            // Chrome tags a non-default profile's windows `Chrome.<profile>`.
            || id
                .to_ascii_lowercase()
                .starts_with(&format!("{}.", app_id.to_ascii_lowercase())))
    {
        return true;
    }
    if window.exe.is_empty() {
        return false;
    }
    let exe = window.exe.to_lowercase().replace('/', "\\");
    if let Some(app_exe) = app_exe {
        let app_exe = app_exe.to_lowercase().replace('/', "\\");
        if app_exe == exe {
            return true;
        }
        if let Some((folder, _)) = app_exe.rsplit_once('\\')
            && own_folder(folder)
            && exe.starts_with(&format!("{folder}\\"))
        {
            return true;
        }
    }
    let squash = |text: &str| -> String {
        text.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    };
    let stem = exe
        .rsplit('\\')
        .next()
        .unwrap_or_default()
        .trim_end_matches(".exe");
    let name = squash(&app.name);
    name.len() >= 3 && name == squash(stem)
}

/// A folder that is one app's own, not one many programs share.
fn own_folder(folder: &str) -> bool {
    let depth = folder.matches('\\').count();
    let shared = [
        "\\windows",
        "\\program files",
        "\\program files (x86)",
        "\\appdata\\local",
        "\\appdata\\roaming",
    ];
    depth >= 2
        && !shared.iter().any(|shared| folder.ends_with(shared))
        && !folder.contains("\\windows\\")
        && !folder.ends_with("\\windows")
}

/// The program an app id launches, when the id names one: a path, or a known folder's id and
/// the path inside it (`{6D809377-…}\Notepad++\notepad++.exe`). Packaged apps have none.
fn program_of(app_id: &str) -> Option<String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(found) = cache
        .lock()
        .ok()
        .and_then(|cache| cache.get(app_id).cloned())
    {
        return found;
    }
    let found = if looks_like_path(app_id) {
        Some(app_id.to_string())
    } else {
        known_folder_parts(app_id).and_then(|(folder, rest)| {
            platform::known_folder(folder).map(|root| format!("{root}\\{rest}"))
        })
    };
    if let Ok(mut cache) = cache.lock() {
        cache.insert(app_id.to_string(), found.clone());
    }
    found
}

fn looks_like_path(id: &str) -> bool {
    id.len() > 3
        && id.as_bytes()[1] == b':'
        && (id.as_bytes()[2] == b'\\' || id.as_bytes()[2] == b'/')
}

/// `{GUID}\rest` split into the GUID (without braces) and the rest.
fn known_folder_parts(id: &str) -> Option<(&str, &str)> {
    let rest = id.strip_prefix('{')?;
    let (guid, path) = rest.split_once("}\\")?;
    (guid.len() == 36 && !path.is_empty()).then_some((guid, path))
}

/// Windows whose title has the typed text, best first: titles starting with it, then a word
/// starting with it, then anywhere.
pub fn by_title<'a>(query: &str, windows: &'a [OpenWindow], limit: usize) -> Vec<&'a OpenWindow> {
    let needle = query.trim().to_lowercase();
    if needle.chars().count() < 2 {
        return Vec::new();
    }
    let mut found: Vec<(u8, usize, &OpenWindow)> = windows
        .iter()
        .enumerate()
        .filter_map(|(order, window)| {
            let title = window.title.to_lowercase();
            let rank = if title.starts_with(&needle) {
                0
            } else if title
                .split(|c: char| !c.is_alphanumeric())
                .any(|word| word.starts_with(&needle))
            {
                1
            } else if title.contains(&needle) {
                2
            } else {
                return None;
            };
            Some((rank, order, window))
        })
        .collect();
    found.sort_by_key(|(rank, order, _)| (*rank, *order));
    found
        .into_iter()
        .take(limit)
        .map(|(_, _, window)| window)
        .collect()
}

/// Adds what's running to a suggestion list: app rows that are open say "Running" and carry
/// their windows; windows whose title matches the text get a section below the apps.
pub fn annotate(response: &mut SuggestResponse, installed: &[App], query: &str) {
    let windows = all();
    if windows.is_empty() {
        return;
    }
    for item in response.items.iter_mut().filter(|item| item.kind == "app") {
        let Some(app) = item
            .app_id
            .as_deref()
            .and_then(|id| installed.iter().find(|app| app.id == id))
        else {
            continue;
        };
        let open = of_app(app, &windows);
        if !open.is_empty() {
            item.hint = "Running".into();
            item.windows = open
                .iter()
                .map(|window| WindowRef {
                    handle: window.handle,
                    title: window.title.clone(),
                })
                .collect();
        }
    }

    // Only for free text: a bang or a scope (!app, !f) is about something else.
    if response.mode != "search" || query.contains('!') || response.text.trim().is_empty() {
        return;
    }
    let shown: Vec<&str> = response
        .items
        .iter()
        .map(|item| item.label.as_str())
        .collect();
    let rows: Vec<Suggestion> = by_title(&response.text, &windows, 3)
        .into_iter()
        .filter(|window| {
            !shown
                .iter()
                .any(|label| label.eq_ignore_ascii_case(&window.title))
        })
        .map(|window| window_item(window, installed))
        .collect();
    if rows.is_empty() {
        return;
    }
    // Below the apps, and never ahead of the row Enter takes.
    let after_apps = response
        .items
        .iter()
        .rposition(|item| item.section == "apps")
        .map(|index| index + 1);
    let after_preselect = response.preselect.map(|index| index + 1).unwrap_or(0);
    let at = after_apps
        .unwrap_or(0)
        .max(after_preselect)
        .min(response.items.len());
    response.items.splice(at..at, rows);
    response.items.truncate(10);
}

fn window_item(window: &OpenWindow, installed: &[App]) -> Suggestion {
    let app = installed.iter().find(|app| {
        let exe = program_of(&app.id);
        belongs(app, exe.as_deref(), window)
    });
    let program = window
        .exe
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or_default()
        .trim_end_matches(".exe")
        .to_string();
    Suggestion {
        label: window.title.clone(),
        kind: "window".into(),
        hint: app.map(|app| app.name.clone()).unwrap_or(program),
        section: "windows".into(),
        icon: match app {
            Some(app) => format!("app:{}", app.id),
            None if !window.exe.is_empty() => format!("app:exe:{}", window.exe),
            None => "glyph:grid".into(),
        },
        app_id: app.map(|app| app.id.clone()),
        window: Some(window.handle),
        ..Default::default()
    }
}

#[cfg(windows)]
mod platform {
    use super::OpenWindow;
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
    use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
    use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
    use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
    use windows::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::{
        IPropertyStore, SHGetPropertyStoreForWindow,
    };
    use windows::Win32::UI::Shell::{KNOWN_FOLDER_FLAG, SHGetKnownFolderPath};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GWL_EXSTYLE, GetWindow, GetWindowLongW, GetWindowTextLengthW,
        GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible,
        PostMessageW, SW_RESTORE, SetForegroundWindow, ShowWindow, WM_CLOSE, WS_EX_TOOLWINDOW,
    };
    use windows::core::{BOOL, GUID, PWSTR};

    pub fn list() -> Vec<OpenWindow> {
        unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
            let found = unsafe { &mut *(data.0 as *mut Vec<OpenWindow>) };
            if let Some(window) = unsafe { describe(hwnd) } {
                found.push(window);
            }
            true.into()
        }
        // The shell's window properties are COM objects.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let mut found: Vec<OpenWindow> = Vec::new();
        unsafe {
            let _ = EnumWindows(
                Some(visit),
                LPARAM(&mut found as *mut Vec<OpenWindow> as isize),
            );
        }
        found
    }

    /// A window someone would switch to: shown, titled, top-level, not a tool palette, not
    /// cloaked (hidden UWP frames, windows on another desktop), and not the app's own or the desktop.
    unsafe fn describe(hwnd: HWND) -> Option<OpenWindow> {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) == 0 {
                return None;
            }
            if GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid()) {
                return None;
            }
            if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
                return None;
            }
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                &mut cloaked as *mut u32 as *mut _,
                std::mem::size_of::<u32>() as u32,
            );
            if cloaked != 0 {
                return None;
            }
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == GetCurrentProcessId() {
                return None;
            }
            let mut text = [0u16; 512];
            let length = GetWindowTextW(hwnd, &mut text);
            let title = String::from_utf16_lossy(&text[..length.max(0) as usize])
                .trim()
                .to_string();
            let (exe, packaged) = program(pid);
            if title.is_empty()
                || (title == "Program Manager" && exe.to_lowercase().ends_with("explorer.exe"))
            {
                return None;
            }
            let app_id = window_app_id(hwnd).or(packaged).unwrap_or_default();
            Some(OpenWindow {
                handle: hwnd.0 as i64,
                title,
                exe,
                app_id,
            })
        }
    }

    /// The process's program path, and its AppUserModelID when it's a packaged app.
    fn program(pid: u32) -> (String, Option<String>) {
        unsafe {
            let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return (String::new(), None);
            };
            let mut path = [0u16; 1024];
            let mut length = path.len() as u32;
            let named = QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut length,
            );
            let exe = if named.is_ok() {
                String::from_utf16_lossy(&path[..length as usize])
            } else {
                String::new()
            };
            let mut id = [0u16; 256];
            let mut id_length = id.len() as u32;
            let packaged =
                GetApplicationUserModelId(process, &mut id_length, Some(PWSTR(id.as_mut_ptr())))
                    .is_ok()
                    .then(|| String::from_utf16_lossy(&id[..id_length.saturating_sub(1) as usize]));
            let _ = CloseHandle(process);
            (exe, packaged.filter(|id| !id.is_empty()))
        }
    }

    /// The AppUserModelID the window itself declares (Chrome, Terminal, UWP frames do).
    fn window_app_id(hwnd: HWND) -> Option<String> {
        unsafe {
            let store: IPropertyStore = SHGetPropertyStoreForWindow(hwnd).ok()?;
            let value = store.GetValue(&PKEY_AppUserModel_ID).ok()?;
            let text = PropVariantToStringAlloc(&value).ok()?;
            let id = text.to_string().ok();
            CoTaskMemFree(Some(text.0 as *const _));
            id.filter(|id| !id.is_empty())
        }
    }

    /// The shown windows a process owns.
    pub fn of_process(pid: u32) -> Vec<i64> {
        struct Search {
            pid: u32,
            found: Vec<i64>,
        }
        unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
            let search = unsafe { &mut *(data.0 as *mut Search) };
            let mut owner = 0;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut owner)) };
            if owner == search.pid
                && unsafe { IsWindowVisible(hwnd) }.as_bool()
                && unsafe { GetWindowTextLengthW(hwnd) } > 0
            {
                search.found.push(hwnd.0 as i64);
            }
            true.into()
        }
        let mut search = Search {
            pid,
            found: Vec::new(),
        };
        unsafe {
            let _ = EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize));
        }
        search.found
    }

    pub fn known_folder(guid: &str) -> Option<String> {
        let id = GUID::try_from(guid).ok()?;
        unsafe {
            let path = SHGetKnownFolderPath(&id, KNOWN_FOLDER_FLAG(0), None).ok()?;
            let text = path.to_string().ok();
            CoTaskMemFree(Some(path.0 as *const _));
            text
        }
    }

    pub fn focus(handle: i64) -> bool {
        let hwnd = HWND(handle as isize as *mut _);
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool() {
                return false;
            }
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd).as_bool()
        }
    }

    pub fn close(handle: i64) -> bool {
        let hwnd = HWND(handle as isize as *mut _);
        unsafe {
            IsWindow(Some(hwnd)).as_bool()
                && PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)).is_ok()
        }
    }
}

// The Mac side (NSWorkspace running apps, AX window titles) is still to come; until then
// nothing is listed and app rows launch as before.
#[cfg(not(windows))]
mod platform {
    use super::OpenWindow;

    pub fn list() -> Vec<OpenWindow> {
        Vec::new()
    }

    pub fn of_process(_pid: u32) -> Vec<i64> {
        Vec::new()
    }

    pub fn known_folder(_guid: &str) -> Option<String> {
        None
    }

    pub fn focus(_handle: i64) -> bool {
        false
    }

    pub fn close(_handle: i64) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(title: &str, exe: &str, app_id: &str) -> OpenWindow {
        OpenWindow {
            handle: 1,
            title: title.into(),
            exe: exe.into(),
            app_id: app_id.into(),
        }
    }

    #[test]
    fn windows_belong_to_apps_by_id_or_program() {
        let chrome = window(
            "Inbox - Gmail",
            r"C:\Program Files\Google\Chrome\chrome.exe",
            "Chrome",
        );
        let app = |id: &str, name: &str| App {
            id: id.into(),
            name: name.into(),
        };
        assert!(belongs(&app("Chrome", "Google Chrome"), None, &chrome));
        let profile = window("Docs", r"C:\x\chrome.exe", "Chrome.ABCDEF");
        assert!(belongs(&app("Chrome", "Google Chrome"), None, &profile));
        assert!(!belongs(&app("ChromeBeta", "Chrome Beta"), None, &profile));
        let notepad = window(
            "notes.txt - Notepad++",
            r"C:\Program Files\Notepad++\notepad++.exe",
            "",
        );
        let notepad_app = app(
            "{6D809377-6AF0-444B-8957-A3773F02200E}\\Notepad++\\notepad++.exe",
            "Notepad++",
        );
        assert!(belongs(
            &notepad_app,
            Some(r"c:\program files\notepad++\notepad++.exe"),
            &notepad
        ));
        assert!(!belongs(
            &app("Chrome", "Google Chrome"),
            Some(r"C:\other.exe"),
            &notepad
        ));
    }

    #[test]
    fn helper_programs_in_an_apps_folder_and_programs_named_like_the_app_count() {
        let app = |id: &str, name: &str| App {
            id: id.into(),
            name: name.into(),
        };
        let steam = window(
            "Steam",
            r"C:\Program Files (x86)\Steam\bin\cef\cef.win64\steamwebhelper.exe",
            "Valve.Steam.Client",
        );
        let steam_app = app(r"{7C5A40EF}\Steam\Steam.exe", "Steam");
        assert!(belongs(
            &steam_app,
            Some(r"C:\Program Files (x86)\Steam\Steam.exe"),
            &steam
        ));
        let github = window(
            "GitHub Desktop",
            r"C:\Users\Zach\AppData\Local\GitHubDesktop\app-3.6.6\GitHubDesktop.exe",
            "",
        );
        assert!(belongs(
            &app("com.squirrel.GitHubDesktop.GitHubDesktop", "GitHub Desktop"),
            None,
            &github
        ));
        // A system tool's folder is shared: Notepad isn't Registry Editor's window.
        let notepad = window("notes - Notepad", r"C:\Windows\System32\notepad.exe", "");
        assert!(!belongs(
            &app(r"C:\Windows\regedit.exe", "Registry Editor"),
            Some(r"C:\Windows\regedit.exe"),
            &notepad
        ));
    }

    #[test]
    fn known_folder_ids_split_into_folder_and_path() {
        assert_eq!(
            known_folder_parts("{6D809377-6AF0-444B-8957-A3773F02200E}\\Notepad++\\notepad++.exe"),
            Some((
                "6D809377-6AF0-444B-8957-A3773F02200E",
                "Notepad++\\notepad++.exe"
            ))
        );
        assert_eq!(known_folder_parts("Chrome"), None);
        assert!(looks_like_path(r"C:\Tools\app.exe"));
        assert!(!looks_like_path(
            "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"
        ));
    }

    /// Lists this computer's open windows: `cargo test lists_open_windows -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn lists_open_windows() {
        for window in platform::list() {
            println!("{:?} | {} | {}", window.title, window.exe, window.app_id);
        }
    }

    #[test]
    fn titles_match_by_start_then_word_then_anywhere() {
        let windows = [
            window("Budget 2026.xlsx - Excel", "", ""),
            window("Q3 budget review - Google Docs", "", ""),
            window("Rebudgeting notes", "", ""),
            window("Inbox", "", ""),
        ];
        let titles: Vec<_> = by_title("budget", &windows, 3)
            .into_iter()
            .map(|window| window.title.as_str())
            .collect();
        assert_eq!(
            titles,
            [
                "Budget 2026.xlsx - Excel",
                "Q3 budget review - Google Docs",
                "Rebudgeting notes"
            ]
        );
        assert!(by_title("b", &windows, 3).is_empty());
    }
}
