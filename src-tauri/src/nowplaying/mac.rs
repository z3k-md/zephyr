//! macOS: what Spotify and Music are playing, through each app's own AppleScript. macOS has no
//! public way to read other apps' media (MediaRemote is closed to third parties since 15.4),
//! so the two players that script well are asked directly, and only while they're running:
//! a `tell` would otherwise launch them. Each asks for Automation permission the first time.
//! Both announce track and play/pause changes as distributed notifications, which wake the
//! watcher at once; a slow poll catches the rest (a player quitting, a seek).

use std::sync::mpsc::Sender;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use super::{Control, Session, Sessions, Wake};

/// The players Zephyr can read: bundle id, and the notification each posts on a change.
const PLAYERS: [(&str, &str); 2] = [
    (
        "com.spotify.client",
        "com.spotify.client.PlaybackStateChanged",
    ),
    ("com.apple.Music", "com.apple.Music.playerInfo"),
];

/// A cover bigger than this is left out rather than sent to the bar.
const MAX_ARTWORK: usize = 2 * 1024 * 1024;

pub struct Mac {
    /// The last list's players, by index: bundle id and Spotify's cover address.
    listed: Vec<(&'static str, String)>,
}

impl Mac {
    pub fn open(app: &tauri::AppHandle, wake: Sender<Wake>) -> Mac {
        listen(app, wake.clone());
        std::thread::Builder::new()
            .name("now-playing-poll".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(3));
                    if wake.send(Wake::Changed).is_err() {
                        return;
                    }
                }
            })
            .ok();
        Mac { listed: Vec::new() }
    }
}

impl Sessions for Mac {
    fn list(&mut self) -> Vec<Session> {
        self.listed.clear();
        let mut sessions = Vec::new();
        for (bundle, _) in PLAYERS {
            let Some(path) = running(bundle) else {
                continue;
            };
            let Some(reply) = run(&read_script(bundle)) else {
                continue;
            };
            let mut fields = reply.split('\n');
            let state = fields.next().unwrap_or_default().trim();
            if state != "playing" && state != "paused" {
                continue;
            }
            let mut next = || fields.next().unwrap_or_default().trim().to_string();
            let (title, artist, album) = (next(), next(), next());
            let duration_ms = next().parse().ok();
            let position_ms = next().parse().ok();
            let cover = next();
            sessions.push(Session {
                source_app: path,
                title,
                artist,
                album,
                playing: state == "playing",
                current: false,
                position_ms,
                duration_ms,
            });
            self.listed.push((bundle, cover));
        }
        sessions
    }

    fn artwork(&mut self, index: usize) -> Option<String> {
        let (bundle, cover) = self.listed.get(index)?.clone();
        let bytes = if cover.starts_with("https://") {
            fetch(&cover)?
        } else if bundle == "com.apple.Music" {
            music_artwork()?
        } else {
            return None;
        };
        if bytes.is_empty() || bytes.len() > MAX_ARTWORK {
            return None;
        }
        let kind = if bytes.starts_with(b"\x89PNG") {
            "image/png"
        } else {
            "image/jpeg"
        };
        Some(format!("data:{kind};base64,{}", STANDARD.encode(bytes)))
    }

    fn control(&mut self, index: usize, control: Control) -> bool {
        let Some((bundle, _)) = self.listed.get(index) else {
            return false;
        };
        let command = match control {
            Control::Toggle => "playpause",
            Control::Next => "next track",
            Control::Previous => "previous track",
        };
        run(&format!("tell application id \"{bundle}\" to {command}")).is_some()
    }
}

/// Turns the player behind `source_app` (its bundle's path) up or down by `step` (0–1) on
/// its own volume control; the new level.
pub fn change_volume(source_app: &str, step: f32) -> Option<f32> {
    let (bundle, _) = PLAYERS
        .into_iter()
        .find(|(bundle, _)| running(bundle).as_deref() == Some(source_app))?;
    let change = (step * 100.0).round() as i32;
    let reply = run(&format!(
        "tell application id \"{bundle}\"\n\
         set level to (sound volume) + ({change})\n\
         if level < 0 then set level to 0\n\
         if level > 100 then set level to 100\n\
         set sound volume to level\n\
         return level\n\
         end tell"
    ))?;
    let level: f32 = reply.trim().parse().ok()?;
    Some(level / 100.0)
}

/// Brings the player forward.
pub fn focus_app(source_app: &str) -> bool {
    PLAYERS
        .into_iter()
        .find(|(bundle, _)| running(bundle).as_deref() == Some(source_app))
        .is_some_and(|(bundle, _)| {
            run(&format!("tell application id \"{bundle}\" to activate")).is_some()
        })
}

/// The player's state, then title, artist, album, duration and position in ms, and Spotify's
/// cover address, a line each. Times go out as whole milliseconds, so no locale's decimal
/// comma gets in the way.
fn read_script(bundle: &str) -> String {
    let (duration, cover) = if bundle == "com.spotify.client" {
        // Spotify gives durations in ms already, and its cover as an address.
        ("(duration of t)", "(artwork url of t)")
    } else {
        ("((duration of t) * 1000)", "\"\"")
    };
    format!(
        "tell application id \"{bundle}\"\n\
         set st to player state as string\n\
         if st is not \"playing\" and st is not \"paused\" then return st\n\
         try\n\
         set t to current track\n\
         on error\n\
         return \"stopped\"\n\
         end try\n\
         set lf to linefeed\n\
         return st & lf & (name of t) & lf & (artist of t) & lf & (album of t) & lf & \
         ({duration} as integer as string) & lf & \
         ((player position * 1000) as integer as string) & lf & {cover}\n\
         end tell"
    )
}

/// Music hands over its cover as raw data, which the script writes to a file to read back.
fn music_artwork() -> Option<Vec<u8>> {
    let path = std::env::temp_dir().join(format!("zephyr-cover-{}", std::process::id()));
    let script = format!(
        "tell application id \"com.apple.Music\" to set d to raw data of artwork 1 of current track\n\
         set f to open for access (POSIX file \"{}\") with write permission\n\
         set eof f to 0\n\
         write d to f\n\
         close access f",
        path.display()
    );
    run(&script)?;
    let bytes = std::fs::read(&path).ok();
    let _ = std::fs::remove_file(&path);
    bytes
}

fn fetch(url: &str) -> Option<Vec<u8>> {
    let url = url.to_string();
    tauri::async_runtime::block_on(async move {
        let response = reqwest::Client::new()
            .get(url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        response.bytes().await.ok().map(|bytes| bytes.to_vec())
    })
}

/// Runs AppleScript, returning what it printed; none when it failed (the player quit, or
/// Automation permission was refused) or took too long (a permission prompt still waiting
/// for an answer, which would otherwise hold up the watcher).
fn run(script: &str) -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;
    let mut child = Command::new("/usr/bin/osascript")
        .args(["-e", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().ok()? {
            break status;
        }
        if started.elapsed() > Duration::from_secs(4) {
            let _ = child.kill();
            let _ = child.wait();
            log::debug!("AppleScript took too long and was stopped");
            return None;
        }
        std::thread::sleep(Duration::from_millis(15));
    };
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    if !status.success() {
        let mut err = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut err);
        }
        log::debug!("AppleScript failed: {}", err.trim());
        return None;
    }
    Some(out.trim_end().to_string())
}

/// The running app's bundle path, when the app with `bundle` is running.
fn running(bundle: &str) -> Option<String> {
    use objc2_app_kit::NSRunningApplication;
    use objc2_foundation::NSString;
    let apps =
        NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(bundle));
    let app = apps.firstObject()?;
    let url = app.bundleURL()?;
    Some(url.path()?.to_string())
}

/// Wakes the watcher on each player's change notification. Distributed notifications arrive
/// through the main run loop, so they're watched from the main thread.
fn listen(app: &tauri::AppHandle, wake: Sender<Wake>) {
    let _ = app.run_on_main_thread(move || {
        use block2::RcBlock;
        use objc2_foundation::{NSDistributedNotificationCenter, NSNotification, NSString};
        let center = NSDistributedNotificationCenter::defaultCenter();
        for (_, name) in PLAYERS {
            let wake = wake.clone();
            let block = RcBlock::new(move |_: std::ptr::NonNull<NSNotification>| {
                let _ = wake.send(Wake::Changed);
            });
            // SAFETY: the block only sends on a channel; the observer lives as long as the app.
            let observer = unsafe {
                center.addObserverForName_object_queue_usingBlock(
                    Some(&NSString::from_str(name)),
                    None,
                    None,
                    &block,
                )
            };
            std::mem::forget(observer);
        }
    });
}
