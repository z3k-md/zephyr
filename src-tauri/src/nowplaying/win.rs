//! Windows' media transport controls: every app that shows in the volume flyout's media
//! panel (Spotify, a browser tab playing YouTube, the Media Player app) reports its track and
//! takes play/pause, next and previous here.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    CurrentSessionChangedEventArgs, GlobalSystemMediaTransportControlsSession as MediaSession,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    MediaPropertiesChangedEventArgs, PlaybackInfoChangedEventArgs, SessionsChangedEventArgs,
};
use windows::Storage::Streams::DataReader;

use super::{Control, Session, Sessions, Wake};

/// Covers bigger than this are skipped rather than sent to the bar.
const MAX_ARTWORK: u64 = 4 * 1024 * 1024;

pub struct Windows {
    manager: Manager,
    wake: Sender<Wake>,
    /// Set when apps come or go, so the next read listens to the new ones.
    sessions_changed: Arc<AtomicBool>,
    /// The sessions listened to, with their event tokens.
    watched: Vec<(MediaSession, i64, i64)>,
    /// The sessions in the order of the last list.
    listed: Vec<MediaSession>,
}

impl Windows {
    pub fn open(wake: Sender<Wake>) -> windows::core::Result<Self> {
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
        // This thread only waits on Windows; the media calls answer from any thread.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let manager = Manager::RequestAsync()?.join()?;
        let sessions_changed = Arc::new(AtomicBool::new(true));

        let flag = sessions_changed.clone();
        let sender = wake.clone();
        manager.SessionsChanged(
            &TypedEventHandler::<Manager, SessionsChangedEventArgs>::new(move |_, _| {
                flag.store(true, Ordering::SeqCst);
                let _ = sender.send(Wake::Changed);
                Ok(())
            }),
        )?;
        let sender = wake.clone();
        manager.CurrentSessionChanged(&TypedEventHandler::<
            Manager,
            CurrentSessionChangedEventArgs,
        >::new(move |_, _| {
            let _ = sender.send(Wake::Changed);
            Ok(())
        }))?;

        Ok(Self {
            manager,
            wake,
            sessions_changed,
            watched: Vec::new(),
            listed: Vec::new(),
        })
    }

    /// Listens to each app's track and play state, after apps come or go.
    fn watch_sessions(&mut self, sessions: &[MediaSession]) {
        for (session, media, playback) in self.watched.drain(..) {
            let _ = session.RemoveMediaPropertiesChanged(media);
            let _ = session.RemovePlaybackInfoChanged(playback);
        }
        for session in sessions {
            let sender = self.wake.clone();
            let media = session.MediaPropertiesChanged(&TypedEventHandler::<
                MediaSession,
                MediaPropertiesChangedEventArgs,
            >::new(move |_, _| {
                let _ = sender.send(Wake::Changed);
                Ok(())
            }));
            let sender = self.wake.clone();
            let playback = session.PlaybackInfoChanged(&TypedEventHandler::<
                MediaSession,
                PlaybackInfoChangedEventArgs,
            >::new(move |_, _| {
                let _ = sender.send(Wake::Changed);
                Ok(())
            }));
            if let (Ok(media), Ok(playback)) = (media, playback) {
                self.watched.push((session.clone(), media, playback));
            }
        }
    }
}

impl Sessions for Windows {
    fn list(&mut self) -> Vec<Session> {
        let Ok(all) = self.manager.GetSessions() else {
            self.listed.clear();
            return Vec::new();
        };
        let sessions: Vec<MediaSession> = all.into_iter().collect();
        if self.sessions_changed.swap(false, Ordering::SeqCst) {
            self.watch_sessions(&sessions);
        }
        // Several browser tabs share one app id, so the session the media keys control is told
        // apart by its title too.
        let current = self
            .manager
            .GetCurrentSession()
            .ok()
            .map(|session| read(&session))
            .map(|session| (session.source_app, session.title));
        let read = sessions
            .iter()
            .map(|session| {
                let mut read = read(session);
                read.current = current
                    .as_ref()
                    .is_some_and(|(app, title)| *app == read.source_app && *title == read.title);
                read
            })
            .collect();
        self.listed = sessions;
        read
    }

    fn artwork(&mut self, index: usize) -> Option<String> {
        let session = self.listed.get(index)?;
        let properties = session.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
        let stream = properties
            .Thumbnail()
            .ok()?
            .OpenReadAsync()
            .ok()?
            .join()
            .ok()?;
        let size = stream.Size().ok()?;
        if size == 0 || size > MAX_ARTWORK {
            return None;
        }
        let kind = stream
            .ContentType()
            .map(|kind| kind.to_string())
            .ok()
            .filter(|kind| kind.starts_with("image/"))
            .unwrap_or_else(|| "image/png".to_string());
        let reader = DataReader::CreateDataReader(&stream).ok()?;
        let loaded = reader.LoadAsync(size as u32).ok()?.join().ok()?;
        let mut bytes = vec![0u8; loaded as usize];
        reader.ReadBytes(&mut bytes).ok()?;
        Some(format!("data:{kind};base64,{}", STANDARD.encode(bytes)))
    }

    fn control(&mut self, index: usize, control: Control) -> bool {
        let Some(session) = self.listed.get(index) else {
            return false;
        };
        let pressed = match control {
            Control::Toggle => session.TryTogglePlayPauseAsync(),
            Control::Next => session.TrySkipNextAsync(),
            Control::Previous => session.TrySkipPreviousAsync(),
        };
        pressed
            .and_then(|operation| operation.join())
            .unwrap_or(false)
    }
}

fn read(session: &MediaSession) -> Session {
    let source_app = session
        .SourceAppUserModelId()
        .map(|id| id.to_string())
        .unwrap_or_default();
    let properties = session
        .TryGetMediaPropertiesAsync()
        .and_then(|operation| operation.join())
        .ok();
    let text = |value: windows::core::Result<windows::core::HSTRING>| {
        value
            .map(|value| value.to_string().trim().to_string())
            .unwrap_or_default()
    };
    let (title, artist, album) = match &properties {
        Some(properties) => (
            text(properties.Title()),
            text(properties.Artist()),
            text(properties.AlbumTitle()),
        ),
        None => Default::default(),
    };
    let playing = session
        .GetPlaybackInfo()
        .and_then(|info| info.PlaybackStatus())
        .is_ok_and(|status| status == Status::Playing);
    let timeline = session.GetTimelineProperties().ok();
    // TimeSpan counts 100 ns ticks.
    let ms = |ticks: i64| (ticks > 0).then_some(ticks as u64 / 10_000);
    let position_ms = timeline
        .as_ref()
        .and_then(|timeline| timeline.Position().ok())
        .and_then(|span| ms(span.Duration));
    let duration_ms = timeline
        .as_ref()
        .and_then(|timeline| timeline.EndTime().ok())
        .and_then(|span| ms(span.Duration));
    Session {
        current: false,
        source_app,
        title,
        artist,
        album,
        playing,
        position_ms,
        duration_ms,
    }
}

/// Brings an app's window forward: an `.exe` name finds that program's main window; a
/// packaged app id opens through the Apps folder, which switches to it when it's running.
pub fn focus_app(source_app: &str) -> bool {
    if source_app.to_ascii_lowercase().ends_with(".exe") {
        return focus_program(source_app);
    }
    if source_app.is_empty() {
        return false;
    }
    std::process::Command::new("explorer.exe")
        .arg(format!("shell:AppsFolder\\{source_app}"))
        .spawn()
        .is_ok()
}

fn focus_program(exe: &str) -> bool {
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GetWindow, GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible, SW_RESTORE, SetForegroundWindow, ShowWindow,
    };
    use windows::core::{BOOL, PWSTR};

    struct Search {
        exe: String,
        found: Option<HWND>,
    }

    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
        let search = unsafe { &mut *(data.0 as *mut Search) };
        unsafe {
            // A program's main window: shown, titled and not owned by another window.
            if !IsWindowVisible(hwnd).as_bool()
                || GetWindowTextLengthW(hwnd) == 0
                || GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid())
            {
                return true.into();
            }
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return true.into();
            };
            let mut path = [0u16; 1024];
            let mut length = path.len() as u32;
            let named = QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut length,
            );
            let _ = CloseHandle(process);
            if named.is_ok() {
                let path = String::from_utf16_lossy(&path[..length as usize]);
                let name = path.rsplit('\\').next().unwrap_or_default();
                if name.eq_ignore_ascii_case(&search.exe) {
                    search.found = Some(hwnd);
                    return false.into();
                }
            }
        }
        true.into()
    }

    let mut search = Search {
        exe: exe.to_string(),
        found: None,
    };
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize));
        let Some(hwnd) = search.found else {
            return false;
        };
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd).as_bool()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lists what's playing on this computer: `cargo test lists_what_is_playing -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn lists_what_is_playing() {
        let (wake, _inbox) = std::sync::mpsc::channel();
        let mut sessions = Windows::open(wake).expect("media controls");
        for (index, session) in sessions.list().into_iter().enumerate() {
            let artwork = sessions.artwork(index).map(|url| url.len());
            println!("{session:?} artwork bytes: {artwork:?}");
        }
    }
}
