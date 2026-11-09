//! The process's one low-level keyboard hook.
//!
//! Windows removes a hook that takes too long to return, and a second hook only
//! adds another call on the way to the next one. The volume keys and the
//! clipboard cycler share this hook; each says whether it took the key.

use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG,
    SetWindowsHookExW, TranslateMessage, WH_KEYBOARD_LL,
};

static STARTED: AtomicBool = AtomicBool::new(false);

/// Starts the hook thread once. Called when the volume keys or the clipboard
/// cycler is turned on; while both are off, nothing is installed.
pub fn install() {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Err(err) = std::thread::Builder::new()
        .name("keyhook".into())
        .spawn(hook)
    {
        STARTED.store(false, Ordering::SeqCst);
        log::error!("couldn't watch the keyboard: {err}");
    }
}

fn hook() {
    // SAFETY: a hook procedure installed for this thread's message loop, which runs
    // until the process ends.
    unsafe {
        let module = GetModuleHandleW(None).ok();
        if let Err(err) = SetWindowsHookExW(WH_KEYBOARD_LL, Some(proc), module.map(Into::into), 0) {
            log::error!("couldn't watch the keyboard: {err}");
            STARTED.store(false, Ordering::SeqCst);
            return;
        }
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

unsafe extern "system" fn proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for HC_ACTION, lparam points at the key's KBDLLHOOKSTRUCT.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let message = wparam.0 as u32;
        if crate::volume_keys::on_key(info, message) || crate::clip_cycle::on_key(info, message) {
            return LRESULT(1);
        }
    }
    // SAFETY: passing the key on, as every hook must.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
