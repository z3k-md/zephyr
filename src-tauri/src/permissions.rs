//! What macOS lets the app do, and asking for the rest in context: Settings › Permissions shows
//! each permission with why it's needed, and its Allow button either brings up the
//! system's own prompt or opens the exact System Settings pane. Nothing is asked at launch.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Granted,
    Denied,
    /// Never asked, so Allow brings up the system prompt.
    NotAsked,
    /// The system won't say (or can't be asked right now); Allow still works.
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct Permission {
    pub id: &'static str,
    pub status: Status,
}

/// The System Settings pane for a permission.
pub fn pane(id: &str) -> Option<&'static str> {
    Some(match id {
        "accessibility" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        }
        "automation" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation"
        }
        "files" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders"
        }
        _ => return None,
    })
}

/// Every permission the app can use here; empty on systems without these permissions.
pub fn statuses(protected_folders: bool) -> Vec<Permission> {
    if !cfg!(target_os = "macos") {
        return Vec::new();
    }
    vec![
        Permission {
            id: "accessibility",
            status: if crate::clipboard::can_paste() {
                Status::Granted
            } else {
                Status::NotAsked
            },
        },
        Permission {
            id: "automation",
            status: automation(false),
        },
        Permission {
            id: "files",
            status: files_status(protected_folders),
        },
    ]
}

/// Desktop, Documents and Downloads: not asked until file search may read them, then
/// whatever reading them says.
fn files_status(protected_folders: bool) -> Status {
    if !protected_folders {
        return Status::NotAsked;
    }
    let mut status = Status::Granted;
    for folder in crate::files::protected_folders() {
        if let Err(err) = std::fs::read_dir(&folder)
            && err.kind() == std::io::ErrorKind::PermissionDenied
        {
            status = Status::Denied;
        }
    }
    status
}

/// Reads the three folders, so macOS asks for any it hasn't been asked about yet. Blocks
/// while a prompt is up.
pub fn probe_files() -> Status {
    files_status(true)
}

/// Asks macOS for Accessibility. A grant from an earlier build can linger in the list,
/// switched on but no longer matching this copy of the app; it is cleared first so the
/// switch the user turns on is the one that counts.
pub fn request_accessibility(bundle_id: &str) {
    if crate::clipboard::can_paste() {
        return;
    }
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("tccutil")
        .args(["reset", "Accessibility", bundle_id])
        .status();
    #[cfg(not(target_os = "macos"))]
    let _ = bundle_id;
    crate::clipboard::request_paste_permission();
}

/// Whether the app may send System Events Apple Events (shut down, restart, log out). With
/// `ask`, macOS shows its prompt if it never has. Blocks, so it runs off the main thread.
#[cfg(target_os = "macos")]
pub fn automation(ask: bool) -> Status {
    use std::ffi::c_void;

    #[repr(C)]
    struct AEDesc {
        descriptor_type: u32,
        data_handle: *mut c_void,
    }

    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn AECreateDesc(kind: u32, data: *const c_void, size: isize, result: *mut AEDesc) -> i16;
        fn AEDisposeDesc(desc: *mut AEDesc) -> i16;
        fn AEDeterminePermissionToAutomateTarget(
            target: *const AEDesc,
            event_class: u32,
            event_id: u32,
            ask_user_if_needed: u8,
        ) -> i32;
    }

    const TYPE_APPLICATION_BUNDLE_ID: u32 = u32::from_be_bytes(*b"bund");
    const TYPE_WILD_CARD: u32 = u32::from_be_bytes(*b"****");
    const NO_ERR: i32 = 0;
    const NOT_PERMITTED: i32 = -1743;
    const WOULD_REQUIRE_CONSENT: i32 = -1744;
    const PROC_NOT_FOUND: i32 = -600;
    const SYSTEM_EVENTS: &str = "com.apple.systemevents";

    let check = |ask: bool| {
        let mut target = AEDesc {
            descriptor_type: 0,
            data_handle: std::ptr::null_mut(),
        };
        // SAFETY: the bundle id bytes outlive the call, and the descriptor is disposed.
        unsafe {
            if AECreateDesc(
                TYPE_APPLICATION_BUNDLE_ID,
                SYSTEM_EVENTS.as_ptr().cast(),
                SYSTEM_EVENTS.len() as isize,
                &mut target,
            ) != 0
            {
                return None;
            }
            let status = AEDeterminePermissionToAutomateTarget(
                &target,
                TYPE_WILD_CARD,
                TYPE_WILD_CARD,
                u8::from(ask),
            );
            AEDisposeDesc(&mut target);
            Some(status)
        }
    };

    let mut status = check(ask);
    // macOS can only answer for an app that's running; System Events starts in a moment.
    if ask && status == Some(PROC_NOT_FOUND) {
        let _ = std::process::Command::new("open")
            .args(["-g", "-b", SYSTEM_EVENTS])
            .status();
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(150));
            status = check(true);
            if status != Some(PROC_NOT_FOUND) {
                break;
            }
        }
    }
    match status {
        Some(NO_ERR) => Status::Granted,
        Some(NOT_PERMITTED) => Status::Denied,
        Some(WOULD_REQUIRE_CONSENT) => Status::NotAsked,
        _ => Status::Unknown,
    }
}

#[cfg(not(target_os = "macos"))]
pub fn automation(_ask: bool) -> Status {
    Status::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_permission_has_a_settings_pane() {
        for permission in statuses(false) {
            assert!(pane(permission.id).is_some(), "{}", permission.id);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn automation_status_is_read_without_asking() {
        // Never asks in a test; any answer but a crash is fine.
        let _ = automation(false);
    }
}
