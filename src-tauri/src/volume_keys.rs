//! The volume keys, taken over (Settings › Sound): Zephyr changes the level itself and shows
//! its own overlay instead of Windows' flyout, or a drawer in the bar when the bar is open.
//! Plain keys change master. Alt with Volume Up or Down steps through the apps with audio, and
//! the app picked stays the keys' target until the overlay fades.

#[cfg(windows)]
use serde::Serialize;

#[cfg(windows)]
use crate::audio::Level;

/// What the overlay and the bar's drawer show after a press.
#[cfg(windows)]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shown {
    /// The keys' target: `master` or an app's key.
    pub key: String,
    /// Master first, then the apps, as Alt steps through them.
    pub levels: Vec<Level>,
}

#[cfg(windows)]
pub use win::{open_sound, set_enabled, set_position, wheel};

#[cfg(not(windows))]
pub fn set_enabled(_app: &tauri::AppHandle, _on: bool) {}
#[cfg(not(windows))]
pub fn set_position(_position: &str) {}
#[cfg(not(windows))]
pub fn wheel(_delta: f32) {}
#[cfg(not(windows))]
pub fn open_sound(_app: &tauri::AppHandle) {}

#[cfg(windows)]
mod win {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, Sender};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY,
        VK_VOLUME_DOWN, VK_VOLUME_MUTE, VK_VOLUME_UP,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GWL_EXSTYLE, GWL_STYLE, GetMessageW, GetWindowLongPtrW,
        HC_ACTION, HWND_TOPMOST, KBDLLHOOKSTRUCT, LLKHF_ALTDOWN, MSG, SW_HIDE, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SetWindowLongPtrW,
        SetWindowPos, SetWindowsHookExW, ShowWindow, TranslateMessage, WH_KEYBOARD_LL, WM_KEYDOWN,
        WM_SYSKEYDOWN, WS_CAPTION, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_MAXIMIZEBOX,
        WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
    };

    use super::Shown;
    use crate::audio::{self, MASTER_KEY, Target};

    /// A notch of the volume keys, as Windows steps them.
    const STEP: f32 = 0.02;
    /// The overlay stays this long after the last press; an app picked with Alt stays the
    /// target as long.
    const SHOWN_FOR: Duration = Duration::from_millis(1600);
    /// The overlay's size, in logical pixels, and its gap from the screen's edge: more at the
    /// bottom, so it sits above the taskbar where Windows' own flyout does.
    const WIDTH: f64 = 300.0;
    const HEIGHT: f64 = 104.0;
    const MARGIN: f64 = 16.0;
    const BOTTOM_MARGIN: f64 = 48.0;
    /// An unassigned key, pressed after an Alt+volume key is taken so the app in front
    /// doesn't read the lone Alt as a tap that opens its menu bar.
    const MENU_MASK: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);

    static ENABLED: AtomicBool = AtomicBool::new(false);
    static PRESSES: OnceLock<Sender<Press>> = OnceLock::new();
    static HIDE: crate::timer::Debounce = crate::timer::Debounce::new();
    /// Where the overlay shows (Settings › Sound): `bottom` or `top`, centered, or
    /// `bottom-right`.
    static POSITION: Mutex<String> = Mutex::new(String::new());

    #[derive(Debug, Clone, Copy)]
    enum Press {
        Up {
            alt: bool,
        },
        Down {
            alt: bool,
        },
        Mute {
            alt: bool,
        },
        /// The wheel over the overlay: steps the target.
        Wheel(f32),
        /// The bar took over showing levels; the target goes back to master.
        Reset,
    }

    /// Turns the takeover on or off. The hook and its worker start the first time it's on
    /// and then stay, passing every key through while it's off.
    pub fn set_enabled(app: &AppHandle, on: bool) {
        ENABLED.store(on, Ordering::SeqCst);
        if !on {
            return;
        }
        if PRESSES.get().is_none() {
            let (sender, presses) = mpsc::channel();
            if PRESSES.set(sender).is_ok() {
                let app = app.clone();
                std::thread::spawn(move || work(app, presses));
                std::thread::spawn(hook);
            }
        }
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(err) = prepare_overlay(&app) {
                log::error!("couldn't make the volume overlay: {err}");
            }
        });
    }

    pub fn set_position(position: &str) {
        *POSITION.lock().unwrap_or_else(|poison| poison.into_inner()) = position.to_string();
    }

    pub fn wheel(delta: f32) {
        send(Press::Wheel(delta));
    }

    /// A click on the overlay: the Sound view, in the bar.
    pub fn open_sound(app: &AppHandle) {
        HIDE.cancel();
        if let Some(overlay) = app.get_webview_window("osd") {
            hide(&overlay);
        }
        send(Press::Reset);
        crate::window::show_bar(app);
        let _ = app.emit_to("main", "sound-open", ());
    }

    fn send(press: Press) {
        if let Some(presses) = PRESSES.get() {
            let _ = presses.send(press);
        }
    }

    /// The low-level keyboard hook, on its own thread with the message loop it needs. It
    /// only queues the press: a hook that's slow to return gets dropped by Windows.
    fn hook() {
        // SAFETY: a hook procedure installed for this thread's message loop, which runs
        // until the process ends.
        unsafe {
            let module = GetModuleHandleW(None).ok();
            if let Err(err) =
                SetWindowsHookExW(WH_KEYBOARD_LL, Some(on_key), module.map(Into::into), 0)
            {
                log::error!("couldn't watch the volume keys: {err}");
                return;
            }
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    unsafe extern "system" fn on_key(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 && ENABLED.load(Ordering::Relaxed) {
            // SAFETY: for HC_ACTION, lparam points at the key's KBDLLHOOKSTRUCT.
            let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
            let key = VIRTUAL_KEY(info.vkCode as u16);
            if key == VK_VOLUME_UP || key == VK_VOLUME_DOWN || key == VK_VOLUME_MUTE {
                let message = wparam.0 as u32;
                if message == WM_KEYDOWN || message == WM_SYSKEYDOWN {
                    let alt = info.flags.0 & LLKHF_ALTDOWN.0 != 0;
                    send(match key {
                        VK_VOLUME_UP => Press::Up { alt },
                        VK_VOLUME_DOWN => Press::Down { alt },
                        _ => Press::Mute { alt },
                    });
                }
                // Taken, down and up, so Windows neither changes the level nor shows its flyout.
                return LRESULT(1);
            }
        }
        // SAFETY: passing the key on, as every hook must.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    /// Where the keys land, and until when, after an Alt press picked an app.
    struct Picked {
        key: String,
        until: Instant,
    }

    fn work(app: AppHandle, presses: mpsc::Receiver<Press>) {
        let mut picked: Option<Picked> = None;
        for press in presses {
            if let Press::Reset = press {
                picked = None;
                continue;
            }
            if matches!(
                press,
                Press::Up { alt: true } | Press::Down { alt: true } | Press::Mute { alt: true }
            ) {
                mask_alt();
            }
            let now = Instant::now();
            let mut levels = audio::levels();
            if levels.is_empty() {
                continue;
            }
            let held = picked
                .as_ref()
                .filter(|picked| picked.until > now)
                .map(|picked| picked.key.clone())
                .filter(|key| levels.iter().any(|level| &level.key == key));
            let mut key = held.unwrap_or_else(|| MASTER_KEY.to_string());
            let at = levels
                .iter()
                .position(|level| level.key == key)
                .unwrap_or(0);
            let changed = match press {
                Press::Up { alt: true } | Press::Down { alt: true } => {
                    let forward = matches!(press, Press::Up { .. });
                    let count = levels.len();
                    let next = if forward {
                        (at + 1) % count
                    } else {
                        (at + count - 1) % count
                    };
                    key = levels[next].key.clone();
                    None
                }
                Press::Up { alt: false } => audio::step(&Target::from_key(&key), STEP),
                Press::Down { alt: false } => audio::step(&Target::from_key(&key), -STEP),
                Press::Wheel(delta) => audio::step(&Target::from_key(&key), delta),
                Press::Mute { .. } => audio::toggle_mute(&Target::from_key(&key)),
                Press::Reset => None,
            };
            if let Some(level) = changed
                && let Some(slot) = levels.iter_mut().find(|slot| slot.key == level.key)
            {
                *slot = level;
            }
            picked = Some(Picked {
                key: key.clone(),
                until: now + SHOWN_FOR,
            });
            show(&app, &Shown { key, levels });
        }
    }

    /// The bar's drawer when the bar is open, the overlay otherwise.
    fn show(app: &AppHandle, shown: &Shown) {
        if crate::stats::bar_visible() {
            let _ = app.emit_to("main", "sound-key", shown);
            return;
        }
        let Some(overlay) = app.get_webview_window("osd") else {
            return;
        };
        let _ = app.emit_to("osd", "osd-show", shown);
        place(app, &overlay);
        let hidden = overlay.clone();
        HIDE.schedule(SHOWN_FOR, move || {
            hide(&hidden);
        });
    }

    /// On the screen the pointer is on, where Settings › Sound puts it, without taking focus
    /// from the app in front.
    fn place(app: &AppHandle, overlay: &tauri::WebviewWindow) {
        let monitor = app
            .cursor_position()
            .ok()
            .and_then(|point| app.monitor_from_point(point.x, point.y).ok().flatten())
            .or_else(|| app.primary_monitor().ok().flatten());
        let Some(monitor) = monitor else {
            return;
        };
        let scale = monitor.scale_factor();
        let area = monitor.work_area();
        let (width, height) = ((WIDTH * scale) as i32, (HEIGHT * scale) as i32);
        let margin = (MARGIN * scale) as i32;
        let (left, top) = (area.position.x, area.position.y);
        let (across, down) = (area.size.width as i32, area.size.height as i32);
        let centered = left + (across - width) / 2;
        let position = POSITION
            .lock()
            .map(|position| position.clone())
            .unwrap_or_default();
        let (x, y) = match position.as_str() {
            "top" => (centered, top + margin),
            "bottom-right" => (left + across - width - margin, top + down - height - margin),
            _ => (
                centered,
                top + down - height - (BOTTOM_MARGIN * scale) as i32,
            ),
        };
        let Ok(hwnd) = overlay.hwnd() else {
            return;
        };
        // SAFETY: the overlay's own window handle; a stale one only makes the calls fail.
        unsafe {
            let _ = SetWindowPos(
                HWND(hwnd.0),
                Some(HWND_TOPMOST),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = SetWindowPos(
                HWND(hwnd.0),
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    /// Presses and releases an unassigned key, so a held Alt isn't read as a lone tap.
    fn mask_alt() {
        let key = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: MENU_MASK,
                    dwFlags: flags,
                    ..Default::default()
                },
            },
        };
        let inputs = [key(Default::default()), key(KEYEVENTF_KEYUP)];
        // SAFETY: two keyboard inputs, sized as SendInput expects.
        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    }

    /// Makes the overlay's window ahead of the first press: hidden, transparent, on top,
    /// and never taking focus.
    fn prepare_overlay(app: &AppHandle) -> Result<(), String> {
        if app.get_webview_window("osd").is_some() {
            return Ok(());
        }
        let overlay =
            WebviewWindowBuilder::new(app, "osd", WebviewUrl::App("index.html?view=osd".into()))
                .title("Zephyr volume")
                .background_color(crate::window::WINDOW_BACKGROUND)
                .transparent(true)
                .decorations(false)
                .shadow(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .focusable(false)
                .focused(false)
                .visible(false)
                .inner_size(WIDTH, HEIGHT)
                .build()
                .map_err(|err| err.to_string())?;
        if let Ok(hwnd) = overlay.hwnd() {
            popup(HWND(hwnd.0));
        }
        Ok(())
    }

    /// A bare popup: no title bar or frame (which showed despite `decorations(false)`), out
    /// of the taskbar and Alt+Tab, and never activated by a click.
    fn popup(hwnd: HWND) {
        let frame = (WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX).0;
        // SAFETY: the overlay's own window handle, its styles read and written back.
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            SetWindowLongPtrW(hwnd, GWL_STYLE, ((style & !frame) | WS_POPUP.0) as isize);
            let extended = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            let extended = extended | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0;
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, extended as isize);
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    /// Hides it straight through Windows. It's shown that way, so Tauri still thinks it's
    /// hidden and its own hide does nothing.
    fn hide(overlay: &tauri::WebviewWindow) {
        if let Ok(hwnd) = overlay.hwnd() {
            // SAFETY: the overlay's own window handle; a stale one only makes the call fail.
            unsafe {
                let _ = ShowWindow(HWND(hwnd.0), SW_HIDE);
            }
        }
    }
}
