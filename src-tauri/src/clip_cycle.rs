//! Ctrl+C, then Up, steps through clipboard history while Ctrl stays held.
//!
//! The copy itself is never taken. Only Up, Down, V and Esc are, and only after
//! C in that same hold of Ctrl, so a plain Ctrl+Up still reaches the app in
//! front. Releasing Ctrl copies the selected item; V pastes it; Esc leaves the
//! clipboard alone.

#[cfg(any(windows, test))]
#[cfg_attr(not(windows), allow(dead_code))]
mod gesture {
    /// Virtual keys the hook reports. Left and right arrive on their own; the
    /// generic value shows up from older paths, so both count.
    pub(super) const VK_SHIFT: u16 = 0x10;
    pub(super) const VK_CONTROL: u16 = 0x11;
    pub(super) const VK_MENU: u16 = 0x12;
    pub(super) const VK_ESCAPE: u16 = 0x1B;
    pub(super) const VK_UP: u16 = 0x26;
    pub(super) const VK_DOWN: u16 = 0x28;
    pub(super) const VK_C: u16 = 0x43;
    pub(super) const VK_V: u16 = 0x56;
    pub(super) const VK_LSHIFT: u16 = 0xA0;
    pub(super) const VK_RSHIFT: u16 = 0xA1;
    pub(super) const VK_LCONTROL: u16 = 0xA2;
    pub(super) const VK_RCONTROL: u16 = 0xA3;
    pub(super) const VK_LMENU: u16 = 0xA4;
    pub(super) const VK_RMENU: u16 = 0xA5;
    pub(super) const VK_LWIN: u16 = 0x5B;
    pub(super) const VK_RWIN: u16 = 0x5C;

    /// Which way a step moves. Items are newest first, so older is a higher index.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum Direction {
        Older,
        Newer,
    }

    /// What the worker should do with a key. Wrapping the selection is its job.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum Command {
        Step(Direction),
        Commit,
        Paste,
        Cancel,
    }

    #[derive(Clone, Copy)]
    pub(super) struct Event {
        pub vk: u16,
        pub down: bool,
        pub injected: bool,
    }

    pub(super) struct Machine {
        ctrl: u8,
        shift: u8,
        alt: u8,
        win: u8,
        armed: bool,
        open: bool,
        /// Keys whose down we took, so the matching up is taken too.
        swallowed: Vec<u16>,
    }

    impl Machine {
        pub(super) const fn new() -> Self {
            Self {
                ctrl: 0,
                shift: 0,
                alt: 0,
                win: 0,
                armed: false,
                open: false,
                swallowed: Vec::new(),
            }
        }

        /// Drops the gesture and any remembered modifiers. Returns whether the
        /// popup was open.
        pub(super) fn reset(&mut self) -> bool {
            let open = self.open;
            *self = Self::new();
            open
        }

        pub(super) fn ctrl_held(&self) -> bool {
            self.ctrl != 0
        }

        pub(super) fn is_ctrl(vk: u16) -> bool {
            ctrl_bit(vk).is_some()
        }

        pub(super) fn handle(&mut self, event: Event, active: bool) -> (bool, Option<Command>) {
            // Our own paste is a V we injected; it must not be a second paste.
            if event.injected {
                return (false, None);
            }
            if !active {
                let command = self.open.then_some(Command::Cancel);
                self.armed = false;
                self.open = false;
                self.swallowed.clear();
                self.track_modifier(event.vk, event.down);
                return (false, command);
            }
            if !event.down && self.take_swallowed(event.vk) {
                return (true, None);
            }
            if self.track_modifier(event.vk, event.down) {
                if ctrl_bit(event.vk).is_some() && !event.down && self.ctrl == 0 {
                    let command = self.open.then_some(Command::Commit);
                    self.armed = false;
                    self.open = false;
                    self.swallowed.clear();
                    return (false, command);
                }
                return (false, None);
            }
            if !event.down {
                return (false, None);
            }
            // A held V or Esc repeats; those repeats stay taken, without a second command.
            if self.swallowed.contains(&event.vk) && event.vk != VK_UP && event.vk != VK_DOWN {
                return (true, None);
            }
            let ctrl = self.ctrl != 0;
            let plain = self.shift == 0 && self.alt == 0 && self.win == 0;
            if event.vk == VK_C && ctrl && plain {
                self.armed = true;
                return (false, None);
            }
            if ctrl && (self.armed || self.open) && (event.vk == VK_UP || event.vk == VK_DOWN) {
                self.open = true;
                self.swallow(event.vk);
                let direction = if event.vk == VK_UP {
                    Direction::Older
                } else {
                    Direction::Newer
                };
                return (true, Some(Command::Step(direction)));
            }
            if event.vk == VK_V && self.open {
                self.close();
                self.swallow(event.vk);
                return (true, Some(Command::Paste));
            }
            if event.vk == VK_ESCAPE && self.open {
                self.close();
                self.swallow(event.vk);
                return (true, Some(Command::Cancel));
            }
            let command = self.open.then_some(Command::Cancel);
            self.armed = false;
            self.open = false;
            (false, command)
        }

        fn close(&mut self) {
            self.armed = false;
            self.open = false;
        }

        fn track_modifier(&mut self, vk: u16, down: bool) -> bool {
            if let Some(bit) = ctrl_bit(vk) {
                set_bit(&mut self.ctrl, bit, down);
                return true;
            }
            if let Some(bit) = shift_bit(vk) {
                set_bit(&mut self.shift, bit, down);
                return true;
            }
            if let Some(bit) = alt_bit(vk) {
                set_bit(&mut self.alt, bit, down);
                return true;
            }
            if let Some(bit) = win_bit(vk) {
                set_bit(&mut self.win, bit, down);
                return true;
            }
            false
        }

        fn swallow(&mut self, vk: u16) {
            if !self.swallowed.contains(&vk) {
                self.swallowed.push(vk);
            }
        }

        fn take_swallowed(&mut self, vk: u16) -> bool {
            let Some(index) = self.swallowed.iter().position(|key| *key == vk) else {
                return false;
            };
            self.swallowed.swap_remove(index);
            true
        }
    }

    fn set_bit(slot: &mut u8, bit: u8, down: bool) {
        if down {
            *slot |= bit;
        } else {
            *slot &= !bit;
        }
    }

    fn ctrl_bit(vk: u16) -> Option<u8> {
        match vk {
            VK_LCONTROL => Some(0b001),
            VK_RCONTROL => Some(0b010),
            VK_CONTROL => Some(0b100),
            _ => None,
        }
    }

    fn shift_bit(vk: u16) -> Option<u8> {
        match vk {
            VK_LSHIFT => Some(0b001),
            VK_RSHIFT => Some(0b010),
            VK_SHIFT => Some(0b100),
            _ => None,
        }
    }

    fn alt_bit(vk: u16) -> Option<u8> {
        match vk {
            VK_LMENU => Some(0b001),
            VK_RMENU => Some(0b010),
            VK_MENU => Some(0b100),
            _ => None,
        }
    }

    fn win_bit(vk: u16) -> Option<u8> {
        match vk {
            VK_LWIN => Some(0b001),
            VK_RWIN => Some(0b010),
            _ => None,
        }
    }

    /// Where a gesture opens. `len` is at least 2. Items are newest first, so
    /// Older starts on the previous copy and Newer wraps to the oldest.
    pub(super) fn first_at(len: usize, direction: Direction) -> usize {
        match direction {
            Direction::Older => 1,
            Direction::Newer => len - 1,
        }
    }

    /// The index after one step. Both ends wrap; the state machine doesn't.
    pub(super) fn step_at(at: usize, len: usize, direction: Direction) -> usize {
        if len == 0 {
            return 0;
        }
        match direction {
            Direction::Older => (at + 1) % len,
            Direction::Newer => (at + len - 1) % len,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const VK_A: u16 = 0x41;

        fn key(machine: &mut Machine, vk: u16, down: bool) -> (bool, Option<Command>) {
            machine.handle(
                Event {
                    vk,
                    down,
                    injected: false,
                },
                true,
            )
        }

        fn down(machine: &mut Machine, vk: u16) -> (bool, Option<Command>) {
            key(machine, vk, true)
        }

        fn up(machine: &mut Machine, vk: u16) -> (bool, Option<Command>) {
            key(machine, vk, false)
        }

        fn arm(machine: &mut Machine) {
            down(machine, VK_LCONTROL);
            down(machine, VK_C);
        }

        fn open(machine: &mut Machine) -> (bool, Option<Command>) {
            arm(machine);
            down(machine, VK_UP)
        }

        #[test]
        fn arming_then_up_steps_older_and_down_steps_newer() {
            let mut machine = Machine::new();
            assert_eq!(down(&mut machine, VK_LCONTROL), (false, None));
            assert_eq!(down(&mut machine, VK_C), (false, None));
            assert_eq!(
                down(&mut machine, VK_UP),
                (true, Some(Command::Step(Direction::Older)))
            );
            assert_eq!(
                down(&mut machine, VK_DOWN),
                (true, Some(Command::Step(Direction::Newer)))
            );
        }

        #[test]
        fn ctrl_up_without_c_passes() {
            let mut machine = Machine::new();
            down(&mut machine, VK_LCONTROL);
            assert_eq!(down(&mut machine, VK_UP), (false, None));
        }

        #[test]
        fn a_swallowed_key_up_is_swallowed() {
            let mut machine = Machine::new();
            open(&mut machine);
            assert_eq!(up(&mut machine, VK_UP), (true, None));
        }

        #[test]
        fn injected_keys_change_nothing() {
            let mut machine = Machine::new();
            for vk in [VK_LCONTROL, VK_C, VK_UP] {
                let (swallow, command) = machine.handle(
                    Event {
                        vk,
                        down: true,
                        injected: true,
                    },
                    true,
                );
                assert!(!swallow);
                assert!(command.is_none());
            }
            let (swallow, command) = open(&mut machine);
            assert!(swallow);
            assert_eq!(command, Some(Command::Step(Direction::Older)));
            let (swallow, command) = machine.handle(
                Event {
                    vk: VK_ESCAPE,
                    down: true,
                    injected: true,
                },
                true,
            );
            assert!(!swallow);
            assert!(command.is_none());
            assert_eq!(up(&mut machine, VK_LCONTROL).1, Some(Command::Commit));
        }

        #[test]
        fn v_pastes_and_esc_cancels() {
            let mut machine = Machine::new();
            open(&mut machine);
            assert_eq!(down(&mut machine, VK_V), (true, Some(Command::Paste)));
            assert_eq!(up(&mut machine, VK_V), (true, None));
            assert_eq!(down(&mut machine, VK_UP), (false, None));

            let mut machine = Machine::new();
            open(&mut machine);
            assert_eq!(down(&mut machine, VK_ESCAPE), (true, Some(Command::Cancel)));
            assert_eq!(up(&mut machine, VK_ESCAPE), (true, None));
            assert_eq!(down(&mut machine, VK_DOWN), (false, None));
        }

        #[test]
        fn another_key_cancels_and_disarms() {
            let mut machine = Machine::new();
            open(&mut machine);
            assert_eq!(down(&mut machine, VK_A), (false, Some(Command::Cancel)));
            assert_eq!(down(&mut machine, VK_UP), (false, None));

            let mut machine = Machine::new();
            arm(&mut machine);
            assert_eq!(down(&mut machine, VK_A), (false, None));
            assert_eq!(down(&mut machine, VK_UP), (false, None));
        }

        #[test]
        fn a_held_up_keeps_stepping() {
            let mut machine = Machine::new();
            open(&mut machine);
            assert_eq!(
                down(&mut machine, VK_UP),
                (true, Some(Command::Step(Direction::Older)))
            );
        }

        #[test]
        fn shift_c_does_not_arm() {
            let mut machine = Machine::new();
            down(&mut machine, VK_LCONTROL);
            down(&mut machine, VK_LSHIFT);
            down(&mut machine, VK_C);
            assert_eq!(down(&mut machine, VK_UP), (false, None));
        }

        #[test]
        fn releasing_ctrl_commits_and_passes() {
            let mut machine = Machine::new();
            open(&mut machine);
            assert_eq!(
                up(&mut machine, VK_LCONTROL),
                (false, Some(Command::Commit))
            );
            assert_eq!(down(&mut machine, VK_UP), (false, None));
        }

        #[test]
        fn the_other_ctrl_keeps_it_open() {
            let mut machine = Machine::new();
            open(&mut machine);
            down(&mut machine, VK_RCONTROL);
            assert_eq!(up(&mut machine, VK_LCONTROL), (false, None));
            assert_eq!(
                up(&mut machine, VK_RCONTROL),
                (false, Some(Command::Commit))
            );
        }

        #[test]
        fn inactive_resets_and_passes() {
            let mut machine = Machine::new();
            open(&mut machine);
            let (swallow, command) = machine.handle(
                Event {
                    vk: VK_UP,
                    down: true,
                    injected: false,
                },
                false,
            );
            assert!(!swallow);
            assert_eq!(command, Some(Command::Cancel));
            assert_eq!(down(&mut machine, VK_UP), (false, None));
            assert_eq!(down(&mut machine, VK_C), (false, None));
            assert_eq!(
                down(&mut machine, VK_UP),
                (true, Some(Command::Step(Direction::Older)))
            );
        }

        #[test]
        fn steps_wrap_at_both_ends() {
            assert_eq!(first_at(5, Direction::Older), 1);
            assert_eq!(first_at(5, Direction::Newer), 4);
            assert_eq!(step_at(4, 5, Direction::Older), 0);
            assert_eq!(step_at(0, 5, Direction::Newer), 4);
            assert_eq!(step_at(1, 5, Direction::Newer), 0);
            assert_eq!(step_at(0, 5, Direction::Older), 1);
        }
    }
}

#[cfg(windows)]
mod win {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, Sender};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
    use windows::Win32::Foundation::{HWND, POINT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
        MonitorFromWindow,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
        VK_CONTROL, VK_V,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GWL_STYLE, GetCursorPos, GetForegroundWindow, GetWindowLongPtrW, HWND_TOPMOST,
        KBDLLHOOKSTRUCT, LLKHF_INJECTED, SW_HIDE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos, ShowWindow,
        WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WS_CAPTION, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
    };

    use super::gesture::{self, Command, Machine};
    use crate::clipboard::{self, ClipSummary, Kind};

    /// How long the confirmation pill stays after a copy or paste.
    const PILL_FOR: Duration = Duration::from_millis(1200);
    /// The overlay's size, in logical pixels. The page draws the card inside it.
    const WIDTH: f64 = 460.0;
    const HEIGHT: f64 = 400.0;

    static ENABLED: AtomicBool = AtomicBool::new(false);
    static COMMANDS: OnceLock<Sender<Command>> = OnceLock::new();
    static MACHINE: Mutex<Machine> = Mutex::new(Machine::new());
    static HIDE: crate::timer::Debounce = crate::timer::Debounce::new();

    struct Cycle {
        items: Vec<ClipSummary>,
        at: usize,
    }

    #[derive(Clone, serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Shown<'a> {
        items: &'a [ClipSummary],
        at: usize,
    }

    #[derive(Clone, serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Done {
        title: String,
        kind: Kind,
        pasted: bool,
    }

    /// Turns the cycler on or off. The worker starts the first time it's on and
    /// then stays; while it's off every key passes through. The keyboard hook is
    /// the shared one, and starts when this or the volume keys are on.
    pub fn set_enabled(app: &AppHandle, on: bool) {
        if on {
            ensure_worker(app);
        }
        let cancel = {
            let mut machine = MACHINE.lock().unwrap_or_else(|poison| poison.into_inner());
            // Stored under the same lock `on_key` takes, so a key can't land
            // between the flag and the reset and leave Ctrl looking held.
            ENABLED.store(on, Ordering::Relaxed);
            if on { false } else { machine.reset() }
        };
        if cancel {
            send(Command::Cancel);
        }
        if !on {
            return;
        }
        crate::keyhook::install();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(err) = prepare_overlay(&app) {
                log::error!("couldn't make the clipboard cycler: {err}");
            }
        });
    }

    fn ensure_worker(app: &AppHandle) {
        if COMMANDS.get().is_some() {
            return;
        }
        let (sender, commands) = mpsc::channel();
        if COMMANDS.set(sender).is_ok() {
            let app = app.clone();
            std::thread::spawn(move || work(app, commands));
        }
    }

    /// Whether the cycler is taking this key. Only queues work: the hook that
    /// calls this is removed if it's slow.
    pub(crate) fn on_key(info: &KBDLLHOOKSTRUCT, message: u32) -> bool {
        let down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        let up = message == WM_KEYUP || message == WM_SYSKEYUP;
        if !down && !up {
            return false;
        }
        let mut machine = MACHINE.lock().unwrap_or_else(|poison| poison.into_inner());
        if !ENABLED.load(Ordering::Relaxed) {
            if machine.reset() {
                drop(machine);
                send(Command::Cancel);
            }
            return false;
        }
        // A Ctrl release the hook never saw (the secure desktop takes keys during Ctrl+Alt+Del
        // or a lock) would leave Ctrl looking held, and a bare C then Up would be taken.
        // Windows' own state is current for every key before this one.
        let vk = info.vkCode as u16;
        // SAFETY: a plain key state read.
        if !Machine::is_ctrl(vk)
            && machine.ctrl_held()
            && unsafe { GetAsyncKeyState(i32::from(VK_CONTROL.0)) } >= 0
            && machine.reset()
        {
            send(Command::Cancel);
        }
        let active = !crate::stats::bar_visible();
        let (swallow, command) = machine.handle(
            gesture::Event {
                vk,
                down,
                injected: info.flags.0 & LLKHF_INJECTED.0 != 0,
            },
            active,
        );
        drop(machine);
        if let Some(command) = command {
            send(command);
        }
        swallow
    }

    fn send(command: Command) {
        if let Some(commands) = COMMANDS.get() {
            let _ = commands.send(command);
        }
    }

    fn work(app: AppHandle, commands: mpsc::Receiver<Command>) {
        let mut cycle: Option<Cycle> = None;
        // The gesture's first step found fewer than two copies; the rest of it
        // does nothing until Ctrl comes up.
        let mut idle = false;
        for command in commands {
            log::debug!(
                "clipboard cycle {command:?}, bar open: {}",
                crate::stats::bar_visible()
            );
            match command {
                Command::Step(direction) => {
                    if idle {
                        continue;
                    }
                    if cycle.is_none() {
                        clipboard::refresh_now();
                        let items = clipboard::recent(50);
                        if items.len() < 2 {
                            idle = true;
                            continue;
                        }
                        let at = gesture::first_at(items.len(), direction);
                        cycle = Some(Cycle { items, at });
                    } else if let Some(cycle) = cycle.as_mut() {
                        cycle.at = gesture::step_at(cycle.at, cycle.items.len(), direction);
                    }
                    if let Some(cycle) = cycle.as_ref() {
                        show(&app, &cycle.items, cycle.at);
                    }
                }
                Command::Commit => {
                    if let Some(cycle) = cycle.take() {
                        finish(&app, &cycle, false);
                    }
                    idle = false;
                }
                Command::Paste => {
                    if let Some(cycle) = cycle.take() {
                        finish(&app, &cycle, true);
                    }
                    idle = false;
                }
                Command::Cancel => {
                    cycle = None;
                    idle = false;
                    dismiss(&app);
                }
            }
        }
    }

    fn show(app: &AppHandle, items: &[ClipSummary], at: usize) {
        HIDE.cancel();
        let _ = app.emit_to("clipcycle", "clipcycle-show", Shown { items, at });
        let Some(overlay) = app.get_webview_window("clipcycle") else {
            return;
        };
        place(app, &overlay);
    }

    fn finish(app: &AppHandle, cycle: &Cycle, pasted: bool) {
        let Some(item) = cycle.items.get(cycle.at) else {
            dismiss(app);
            return;
        };
        if let Err(err) = clipboard::copy(item.id, false) {
            log::error!("couldn't put the clipboard item back: {err}");
            dismiss(app);
            return;
        }
        if pasted {
            paste_key();
        }
        let _ = app.emit_to(
            "clipcycle",
            "clipcycle-done",
            Done {
                title: item.title.clone(),
                kind: item.kind,
                pasted,
            },
        );
        let Some(overlay) = app.get_webview_window("clipcycle") else {
            return;
        };
        let hidden = overlay.clone();
        HIDE.schedule(PILL_FOR, move || hide(&hidden));
    }

    fn dismiss(app: &AppHandle) {
        HIDE.cancel();
        if let Some(overlay) = app.get_webview_window("clipcycle") {
            hide(&overlay);
        }
        let _ = app.emit_to("clipcycle", "clipcycle-cancel", ());
    }

    /// Presses and releases V. Ctrl is already down under the user's finger, so
    /// the app in front reads it as a paste.
    fn paste_key() {
        let key = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_V,
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

    /// On the work area of the monitor the foreground window is on, centered and
    /// a bit above the middle. The cursor's monitor stands in when that window
    /// can't be placed, then the primary. Shown without activating, so the app
    /// in front keeps the keys.
    fn place(app: &AppHandle, overlay: &tauri::WebviewWindow) {
        let Some((left, top, across, down)) = foreground_work_area()
            .or_else(cursor_work_area)
            .or_else(|| {
                let monitor = app.primary_monitor().ok().flatten()?;
                let area = monitor.work_area();
                Some((
                    area.position.x,
                    area.position.y,
                    area.size.width as i32,
                    area.size.height as i32,
                ))
            })
        else {
            return;
        };
        let scale = app
            .monitor_from_point(
                f64::from(left) + f64::from(across) / 2.0,
                f64::from(top) + f64::from(down) / 2.0,
            )
            .ok()
            .flatten()
            .or_else(|| app.primary_monitor().ok().flatten())
            .map(|monitor| monitor.scale_factor())
            .unwrap_or(1.0);
        let (width, height) = ((WIDTH * scale) as i32, (HEIGHT * scale) as i32);
        let x = left + (across - width) / 2;
        let y = top + (f64::from(down - height) * 0.42) as i32;
        let Ok(hwnd) = overlay.hwnd() else {
            return;
        };
        // SAFETY: the overlay's own window handle; a stale one only makes the calls fail.
        // Moved and sized first, then shown, so a resize never shows half drawn.
        unsafe {
            let _ = SetWindowPos(
                HWND(hwnd.0),
                Some(HWND_TOPMOST),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE,
            );
            let _ = SetWindowPos(
                HWND(hwnd.0),
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }

    fn foreground_work_area() -> Option<(i32, i32, i32, i32)> {
        // SAFETY: a snapshot of the foreground window. An empty handle fails the next call.
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_invalid() {
            return None;
        }
        // SAFETY: `hwnd` was just returned; a stale one makes MonitorFromWindow fail.
        let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        work_area(monitor)
    }

    fn cursor_work_area() -> Option<(i32, i32, i32, i32)> {
        let mut point = POINT::default();
        // SAFETY: `point` is a POINT to fill.
        unsafe { GetCursorPos(&mut point) }.ok()?;
        // SAFETY: `point` was just filled in.
        let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
        work_area(monitor)
    }

    fn work_area(monitor: HMONITOR) -> Option<(i32, i32, i32, i32)> {
        if monitor.is_invalid() {
            return None;
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `info` is a MONITORINFO with cbSize set, as the call requires.
        unsafe { GetMonitorInfoW(monitor, &mut info) }
            .as_bool()
            .then(|| {
                let work = info.rcWork;
                (
                    work.left,
                    work.top,
                    work.right - work.left,
                    work.bottom - work.top,
                )
            })
    }

    /// Makes the overlay ahead of the first press: hidden, transparent, on top,
    /// and never taking focus.
    fn prepare_overlay(app: &AppHandle) -> Result<(), String> {
        if app.get_webview_window("clipcycle").is_some() {
            return Ok(());
        }
        let overlay = WebviewWindowBuilder::new(
            app,
            "clipcycle",
            WebviewUrl::App("index.html?view=clipcycle".into()),
        )
        .title("Zephyr clipboard")
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
            // Dropping the frame grows the client area, but the page keeps the old size (a
            // title bar short) until Tauri sees a resize; so resize through Tauri, there and back.
            let _ = overlay.set_size(tauri::LogicalSize::new(WIDTH, HEIGHT + 1.0));
            let _ = overlay.set_size(tauri::LogicalSize::new(WIDTH, HEIGHT));
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

#[cfg(windows)]
pub(crate) use win::on_key;
#[cfg(windows)]
pub use win::set_enabled;

#[cfg(not(windows))]
pub fn set_enabled(_app: &tauri::AppHandle, _on: bool) {}
