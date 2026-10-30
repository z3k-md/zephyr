//! What's playing on this computer, for the strip at the end of the search row:
//! the song or video, its artwork, and play/pause, next and previous. Each platform lists its
//! media sessions through [`Sessions`]; the choosing, the artwork cache and the events to the
//! bar are shared.

// Windows and macOS have backends; elsewhere the shared watcher is built but unused.
#![cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]

#[cfg(target_os = "macos")]
mod mac;
#[cfg(windows)]
mod win;

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::util::now_ms;

/// One app's media as the platform reports it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Session {
    /// The app playing it (an AppUserModelID on Windows, such as `Spotify.exe` or `Chrome`).
    pub source_app: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    /// The session the system itself would control with the media keys.
    pub current: bool,
    pub position_ms: Option<u64>,
    pub duration_ms: Option<u64>,
}

/// What the bar shows.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub source_app: String,
    pub playing: bool,
    pub position_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    /// The cover as a `data:` URL, when the app gives one.
    pub artwork: Option<String>,
    /// When this was read, in ms since the epoch.
    pub updated_at: u64,
    /// When this app was last seen playing, so the bar can keep a paused song up a while.
    pub last_played_at: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Toggle,
    Next,
    Previous,
}

/// One platform's media sessions.
pub trait Sessions {
    /// Every app reporting media right now, freshly read.
    fn list(&mut self) -> Vec<Session>;
    /// The artwork of the session at `index` in the last list, as a `data:` URL.
    fn artwork(&mut self, index: usize) -> Option<String>;
    /// Presses play/pause, next or previous on the session at `index` in the last list.
    fn control(&mut self, index: usize, control: Control) -> bool;
}

/// What wakes the watcher: the platform saying something changed, or the bar pressing a
/// button.
pub enum Wake {
    Changed,
    Control(Control),
}

fn current() -> &'static Mutex<Option<NowPlaying>> {
    static CURRENT: OnceLock<Mutex<Option<NowPlaying>>> = OnceLock::new();
    CURRENT.get_or_init(Default::default)
}

static WAKE: OnceLock<Sender<Wake>> = OnceLock::new();

/// Starts watching what's playing, for the life of the app.
pub fn start(app: &AppHandle) {
    let (wake, inbox) = mpsc::channel();
    if WAKE.set(wake.clone()).is_err() {
        return;
    }
    let app = app.clone();
    std::thread::Builder::new()
        .name("now-playing".into())
        .spawn(move || {
            #[cfg(windows)]
            match win::Windows::open(wake) {
                Ok(sessions) => watch(&app, sessions, &inbox),
                Err(err) => log::error!("couldn't read what's playing: {err}"),
            }
            #[cfg(target_os = "macos")]
            {
                let sessions = mac::Mac::open(&app, wake);
                watch(&app, sessions, &inbox);
            }
            #[cfg(not(any(windows, target_os = "macos")))]
            let _ = (app, wake, inbox);
        })
        .ok();
}

/// What's playing now, for a bar that just opened.
pub fn snapshot() -> Option<NowPlaying> {
    current().lock().ok()?.clone()
}

pub fn control(control: Control) {
    if let Some(wake) = WAKE.get() {
        let _ = wake.send(Wake::Control(control));
    }
}

/// Reads what's playing again: the bar opened, and macOS doesn't poll while it's hidden.
pub fn refresh() {
    if let Some(wake) = WAKE.get() {
        let _ = wake.send(Wake::Changed);
    }
}

/// Turns what's playing up or down by `step` (0–1): just that app in the volume mixer on
/// Windows, the player's own volume on macOS (which has no per-app volume). The new level, or
/// none where that can't be done.
pub fn change_volume(step: f32) -> Option<f32> {
    let source = snapshot()?.source_app;
    #[cfg(windows)]
    {
        win::change_volume(&program(&source), step)
    }
    #[cfg(target_os = "macos")]
    {
        mac::change_volume(&source, step)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (source, step);
        None
    }
}

/// The program behind an app id: `Spotify.exe` as is, a browser's own name
/// (`chrome.exe` for `Chrome`), or a Store app's last part (`Spotify.exe` for
/// `SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify`).
#[cfg(windows)]
fn program(source_app: &str) -> String {
    if source_app.to_ascii_lowercase().ends_with(".exe") {
        return source_app.to_string();
    }
    if let Some(exe) = browser_exe(source_app) {
        return exe.to_string();
    }
    let name = source_app.rsplit('!').next().unwrap_or(source_app);
    format!("{name}.exe")
}

/// The cover of the song shown, and how many times it has been read.
struct Artwork {
    key: String,
    cover: Option<String>,
    reads: u32,
}

/// How many times a new song's cover is read over its first seconds.
const ARTWORK_READS: u32 = 4;

/// Wakes the watcher a few times over the next seconds to read a new song's cover again.
fn reread_artwork_soon() {
    let Some(wake) = WAKE.get().cloned() else {
        return;
    };
    std::thread::spawn(move || {
        for _ in 1..ARTWORK_READS {
            std::thread::sleep(Duration::from_millis(1200));
            if wake.send(Wake::Changed).is_err() {
                return;
            }
        }
    });
}

fn watch(app: &AppHandle, mut sessions: impl Sessions, inbox: &Receiver<Wake>) {
    let mut last_played: HashMap<String, u64> = HashMap::new();
    let mut playing_before: HashSet<String> = HashSet::new();
    let mut started = Started::default();
    let mut artwork: Option<Artwork> = None;
    let mut shown: Option<NowPlaying> = None;
    let mut wake = Some(Wake::Changed);
    loop {
        let Some(woke) = wake.take().or_else(|| inbox.recv().ok()) else {
            return;
        };
        // A button needs to know which session it presses; a change is read once it settles.
        if let Wake::Control(control) = woke {
            let list = sessions.list();
            started.update(&list, now_ms());
            if let Some(index) = choose(&list, &started) {
                sessions.control(index, control);
            }
            continue;
        }
        // A track change sends a burst of events; read once it settles. A button pressed
        // meanwhile goes to the session shown, read only then.
        while let Ok(more) = inbox.recv_timeout(Duration::from_millis(120)) {
            if let Wake::Control(control) = more {
                let list = sessions.list();
                started.update(&list, now_ms());
                if let Some(index) = choose(&list, &started) {
                    sessions.control(index, control);
                }
            }
        }
        let list = sessions.list();
        let now = now_ms();
        started.update(&list, now);
        let chosen = choose(&list, &started);
        // An app playing at the last read played until now, even if it just paused: a long
        // song sends no events, and pausing it must not look like it stopped minutes ago.
        for app in playing_before.drain() {
            last_played.insert(app, now);
        }
        for session in list.iter().filter(|session| session.playing) {
            last_played.insert(session.source_app.clone(), now);
            playing_before.insert(session.source_app.clone());
        }
        let next = chosen.map(|index| {
            let session = &list[index];
            let key = track_key(session);
            // Browsers send the new song's cover a moment after its title, so for the first
            // seconds of a track the cover is read again (it may be missing, or the last
            // song's) before it's trusted.
            let cover = match &mut artwork {
                Some(saved) if saved.key == key && saved.reads >= ARTWORK_READS => {
                    saved.cover.clone()
                }
                Some(saved) if saved.key == key => {
                    saved.reads += 1;
                    if let Some(cover) = sessions.artwork(index) {
                        saved.cover = Some(cover);
                    }
                    saved.cover.clone()
                }
                _ => {
                    let cover = sessions.artwork(index);
                    artwork = Some(Artwork {
                        key,
                        cover: cover.clone(),
                        reads: 1,
                    });
                    reread_artwork_soon();
                    cover
                }
            };
            NowPlaying {
                title: session.title.clone(),
                artist: session.artist.clone(),
                album: session.album.clone(),
                source_app: session.source_app.clone(),
                playing: session.playing,
                position_ms: session.position_ms,
                duration_ms: session.duration_ms,
                artwork: cover,
                updated_at: now,
                last_played_at: last_played.get(&session.source_app).copied(),
            }
        });
        if same_apart_from_time(shown.as_ref(), next.as_ref()) {
            continue;
        }
        if let Ok(mut saved) = current().lock() {
            saved.clone_from(&next);
        }
        if let Err(err) = app.emit("nowplaying", &next) {
            log::error!("couldn't tell the bar what's playing: {err}");
        }
        shown = next;
    }
}

/// When each session started playing, by app and title: browser tabs share one app id, and a
/// tab can keep saying it plays (a muted preview, a paused video that never told the system),
/// so the one that most recently started is the one being listened to.
#[derive(Default)]
pub struct Started(HashMap<String, u64>);

impl Started {
    fn key(session: &Session) -> String {
        format!("{}\n{}", session.source_app, session.title)
    }

    /// Notes sessions that started playing since the last read and forgets ones that stopped.
    pub fn update(&mut self, sessions: &[Session], now: u64) {
        let playing: HashSet<String> = sessions
            .iter()
            .filter(|session| session.playing)
            .map(Self::key)
            .collect();
        self.0.retain(|key, _| playing.contains(key));
        for key in playing {
            self.0.entry(key).or_insert(now);
        }
    }

    fn at(&self, session: &Session) -> u64 {
        self.0.get(&Self::key(session)).copied().unwrap_or(0)
    }
}

/// The session the bar follows: one that's playing over one that's paused; among those, the
/// one that started most recently, then the one the system's media keys control, then the
/// first listed. Sessions with no title (an app that registered but hasn't said what it
/// plays) don't count.
pub fn choose(sessions: &[Session], started: &Started) -> Option<usize> {
    sessions
        .iter()
        .enumerate()
        .filter(|(_, session)| !session.title.trim().is_empty())
        .max_by_key(|(index, session)| {
            (
                session.playing,
                started.at(session),
                session.current,
                std::cmp::Reverse(*index),
            )
        })
        .map(|(index, _)| index)
}

fn track_key(session: &Session) -> String {
    format!(
        "{}\n{}\n{}\n{}",
        session.source_app, session.title, session.artist, session.album
    )
}

fn same_apart_from_time(a: Option<&NowPlaying>, b: Option<&NowPlaying>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.title == b.title
                && a.artist == b.artist
                && a.album == b.album
                && a.source_app == b.source_app
                && a.playing == b.playing
                && a.artwork == b.artwork
                && a.duration_ms == b.duration_ms
        }
        _ => false,
    }
}

/// Whether an app id is a web browser, whose playing tab the extension can find.
pub fn browser_exe(source_app: &str) -> Option<&'static str> {
    // Chromium browsers report their name first: `Chrome`, `MSEdge`, `Brave.<profile hash>`.
    let name = source_app
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()?
        .to_ascii_lowercase();
    [
        ("chrome", "chrome.exe"),
        ("chromium", "chrome.exe"),
        ("msedge", "msedge.exe"),
        ("brave", "brave.exe"),
        ("vivaldi", "vivaldi.exe"),
        ("opera", "opera.exe"),
        ("arc", "arc.exe"),
    ]
    .into_iter()
    .find(|(browser, _)| name == *browser)
    .map(|(_, exe)| exe)
}

/// Brings the playing app forward before asking its extension to select the media tab.
/// A successful extension reply alone doesn't guarantee Windows foreground activation.
pub fn focus_source(source_app: &str, title: &str) -> bool {
    if let Some(exe) = browser_exe(source_app) {
        let app_focused = focus_app(exe);
        let tab_focused = crate::browser::focus_playing(title) == Some(true);
        return app_focused || tab_focused;
    }
    focus_app(source_app)
}

#[cfg(windows)]
fn focus_app(source_app: &str) -> bool {
    win::focus_app(source_app)
}

#[cfg(target_os = "macos")]
fn focus_app(source_app: &str) -> bool {
    mac::focus_app(source_app)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn focus_app(_source_app: &str) -> bool {
    false
}

/// macOS: a player's AppleScript reply, a line each: its state, then title, artist, album,
/// duration and position in ms, and Spotify's cover address. A session and the cover
/// address, or none when the player isn't playing or paused.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_player_reply(reply: &str, source_app: String) -> Option<(Session, String)> {
    let mut fields = reply.split('\n');
    let state = fields.next().unwrap_or_default().trim();
    if state != "playing" && state != "paused" {
        return None;
    }
    let mut next = || fields.next().unwrap_or_default().trim().to_string();
    let (title, artist, album) = (next(), next(), next());
    let duration_ms = next().parse().ok();
    let position_ms = next().parse().ok();
    let cover = next();
    let session = Session {
        source_app,
        title,
        artist,
        album,
        playing: state == "playing",
        current: false,
        position_ms,
        duration_ms,
    };
    Some((session, cover))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_players_applescript_reply_reads_as_a_session() {
        let reply = "playing\nSo What\nMiles Davis\nKind of Blue\n562000\n12345\nhttps://i.scdn.co/image/ab67";
        let (session, cover) =
            parse_player_reply(reply, "/Applications/Spotify.app".into()).expect("a session");
        assert_eq!(session.title, "So What");
        assert_eq!(session.artist, "Miles Davis");
        assert_eq!(session.album, "Kind of Blue");
        assert!(session.playing);
        assert_eq!(session.duration_ms, Some(562000));
        assert_eq!(session.position_ms, Some(12345));
        assert_eq!(session.source_app, "/Applications/Spotify.app");
        assert_eq!(cover, "https://i.scdn.co/image/ab67");

        // Music: no cover address, and a stream without a length.
        let (session, cover) =
            parse_player_reply("paused\nRadio\n\n\nmissing value\n0\n", String::new()).unwrap();
        assert!(!session.playing);
        assert_eq!(session.duration_ms, None);
        assert_eq!(session.position_ms, Some(0));
        assert_eq!(cover, "");

        for reply in ["stopped", "", "fast forwarding"] {
            assert!(
                parse_player_reply(reply, String::new()).is_none(),
                "{reply}"
            );
        }
    }

    fn session(title: &str, playing: bool, current: bool) -> Session {
        Session {
            source_app: title.to_string(),
            title: title.to_string(),
            playing,
            current,
            ..Default::default()
        }
    }

    #[test]
    fn a_playing_session_beats_a_paused_one_even_when_the_paused_one_is_current() {
        let sessions = [
            session("paused", false, true),
            session("playing", true, false),
        ];
        assert_eq!(choose(&sessions, &Started::default()), Some(1));
    }

    #[test]
    fn among_playing_sessions_the_current_one_wins_then_the_first() {
        let sessions = [
            session("a", true, false),
            session("b", true, true),
            session("c", true, false),
        ];
        assert_eq!(choose(&sessions, &Started::default()), Some(1));
        let sessions = [session("a", true, false), session("b", true, false)];
        assert_eq!(choose(&sessions, &Started::default()), Some(0));
    }

    #[test]
    fn of_two_playing_tabs_in_one_browser_the_one_that_started_last_wins() {
        let tab = |title: &str| Session {
            source_app: "Chrome".into(),
            title: title.into(),
            playing: true,
            current: true,
            ..Default::default()
        };
        let mut started = Started::default();
        // The YouTube tab was already playing; YouTube Music starts after it.
        started.update(&[tab("A video")], 1_000);
        let both = [tab("A video"), tab("A song")];
        started.update(&both, 5_000);
        assert_eq!(choose(&both, &started), Some(1));
        // A new song in the music tab is a new start too, so it keeps winning.
        let next = [tab("A video"), tab("Next song")];
        started.update(&next, 9_000);
        assert_eq!(choose(&next, &started), Some(1));
    }

    #[test]
    fn with_nothing_playing_the_current_session_shows_paused() {
        let sessions = [session("a", false, false), session("b", false, true)];
        assert_eq!(choose(&sessions, &Started::default()), Some(1));
    }

    #[test]
    fn untitled_sessions_never_show() {
        let sessions = [session("", true, true), session("song", false, false)];
        assert_eq!(choose(&sessions, &Started::default()), Some(1));
        assert_eq!(
            choose(&[session("  ", true, true)], &Started::default()),
            None
        );
        assert_eq!(choose(&[], &Started::default()), None);
    }

    #[test]
    fn browsers_are_recognized_by_their_app_id() {
        assert_eq!(browser_exe("Chrome"), Some("chrome.exe"));
        assert_eq!(browser_exe("MSEdge"), Some("msedge.exe"));
        assert_eq!(browser_exe("Brave.ABC123"), Some("brave.exe"));
        assert_eq!(browser_exe("Spotify.exe"), None);
        assert_eq!(
            browser_exe("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"),
            None
        );
    }
}
