//! Clipboard history: everything copied, searchable, pinnable, and pasteable back into the
//! app in front. Capture and paste are per-platform behind [`Platform`]; history, search and
//! storage are shared. History is encrypted at rest (see [`crypto`]).

pub mod crypto;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crypto::Cipher;

use crate::util::{now_secs, write_atomic};

/// Most items kept, pinned ones aside; older ones go first.
const MAX_ITEMS: usize = 2000;
const MAX_TEXT_BYTES: usize = 1 << 20;
const POLL: Duration = Duration::from_millis(400);

// Settings

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClipboardSettings {
    pub enabled: bool,
    /// Global shortcut that opens clipboard history; empty for none.
    pub shortcut: String,
    /// Days unpinned items are kept; 0 keeps them until the item limit.
    pub retention_days: u32,
    /// Apps whose copies are never recorded, by name or bundle id.
    pub ignored_apps: Vec<String>,
    /// Recognize text in copied images so they can be searched.
    pub recognize_text: bool,
}

#[cfg(target_os = "macos")]
pub const DEFAULT_SHORTCUT: &str = "command+shift+v";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_SHORTCUT: &str = "alt+shift+v";

impl Default for ClipboardSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            shortcut: DEFAULT_SHORTCUT.into(),
            retention_days: 90,
            ignored_apps: [
                "1Password",
                "Bitwarden",
                "Dashlane",
                "Enpass",
                "KeePass",
                "KeePassXC",
                "keeperpasswordmanager",
                "Keychain Access",
                "LastPass",
                "NordPass",
                "Passwords",
                "Proton Pass",
            ]
            .map(String::from)
            .to_vec(),
            recognize_text: true,
        }
    }
}

impl ClipboardSettings {
    pub fn normalized(mut self) -> Result<Self, String> {
        self.shortcut = self.shortcut.trim().to_string();
        if !self.shortcut.is_empty() {
            self.shortcut = crate::shortcut::canonical_shortcut(&self.shortcut)?;
        }
        let mut seen = Vec::<String>::new();
        self.ignored_apps.retain(|app| {
            let app = app.trim();
            let fresh = !app.is_empty() && !seen.iter().any(|seen| seen.eq_ignore_ascii_case(app));
            if fresh {
                seen.push(app.to_string());
            }
            fresh
        });
        self.ignored_apps = seen;
        Ok(self)
    }
}

// Items

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Text,
    Link,
    Color,
    Image,
    Files,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipItem {
    pub id: u64,
    pub kind: Kind,
    /// The text, link or color; for files, their paths one per line.
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub image: Option<ImageInfo>,
    /// Text recognized in an image.
    #[serde(default)]
    pub ocr: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    pub first_copied: i64,
    pub last_copied: i64,
    pub copies: u32,
    pub pinned: bool,
    hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
}

/// What the platform read off the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    Text(String),
    Files(Vec<String>),
    Image {
        png: Vec<u8>,
        width: u32,
        height: u32,
    },
}

#[derive(Debug, Clone)]
pub struct Capture {
    pub content: Content,
    /// The app in front when it was copied: (name, bundle or process id).
    pub source: Option<(String, String)>,
}

/// Capture and paste for one OS.
pub trait Platform: Send + Sync {
    /// Changes whenever anything is copied.
    fn change_count(&self) -> i64;
    /// The clipboard's contents, or nothing when it is empty or marked concealed or transient
    /// (how password managers flag what they copy).
    fn read(&self) -> Option<Capture>;
    fn write(&self, content: &Content, plain: bool) -> Result<(), String>;
    /// Whether the app may send keystrokes; `prompt` asks the OS to show its permission dialog.
    fn can_paste(&self, prompt: bool) -> bool;
    /// Sends the paste keystroke to the app in front.
    fn send_paste(&self) -> Result<(), String>;
    fn recognize_text(&self, png: &[u8]) -> Option<String>;
}

/// For platforms whose capture isn't built yet: history stays empty.
#[cfg(not(any(target_os = "macos", windows)))]
struct Unsupported;

#[cfg(not(any(target_os = "macos", windows)))]
impl Platform for Unsupported {
    fn change_count(&self) -> i64 {
        0
    }
    fn read(&self) -> Option<Capture> {
        None
    }
    fn write(&self, _content: &Content, _plain: bool) -> Result<(), String> {
        Err("Clipboard history isn't available on this system yet".into())
    }
    fn can_paste(&self, _prompt: bool) -> bool {
        false
    }
    fn send_paste(&self) -> Result<(), String> {
        Err("Pasting isn't available on this system yet".into())
    }
    fn recognize_text(&self, _png: &[u8]) -> Option<String> {
        None
    }
}

fn platform() -> &'static dyn Platform {
    #[cfg(target_os = "macos")]
    {
        static MAC: macos::MacClipboard = macos::MacClipboard;
        &MAC
    }
    #[cfg(windows)]
    {
        static WINDOWS: self::windows::WinClipboard = self::windows::WinClipboard;
        &WINDOWS
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        static NONE: Unsupported = Unsupported;
        &NONE
    }
}

/// Windows needs a window to own what is written to the clipboard; the bar's will do.
#[cfg(windows)]
pub fn set_owner_window(window: &tauri::WebviewWindow) {
    if let Ok(hwnd) = window.hwnd() {
        self::windows::set_owner(hwnd);
    }
}

// History

struct History {
    items: Vec<ClipItem>,
    next_id: u64,
    dir: PathBuf,
    /// Shared with the saver thread, which encrypts outside the lock.
    cipher: Option<Arc<Cipher>>,
    settings: ClipboardSettings,
    /// The change count of this module's own last write, which is not a new copy.
    own_write: Option<i64>,
    problem: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    next_id: u64,
    items: Vec<ClipItem>,
}

fn history() -> &'static Mutex<History> {
    static HISTORY: OnceLock<Mutex<History>> = OnceLock::new();
    HISTORY.get_or_init(|| {
        Mutex::new(History {
            items: Vec::new(),
            next_id: 1,
            dir: PathBuf::new(),
            cipher: None,
            settings: ClipboardSettings::default(),
            own_write: None,
            problem: None,
        })
    })
}

/// Loads saved history and starts watching the clipboard.
pub fn start(dir: PathBuf, settings: ClipboardSettings) {
    let enabled = settings.enabled;
    {
        let Ok(mut history) = history().lock() else {
            return;
        };
        history.dir = dir;
        history.settings = settings;
    }
    // The keychain read and the history's decryption happen on the watcher's own thread, so
    // startup doesn't wait on them; nothing is recorded until the key is there anyway.
    std::thread::Builder::new()
        .name("clipboard".into())
        .spawn(move || {
            if enabled {
                unlock();
            }
            watch();
        })
        .map_err(|err| log::error!("couldn't start clipboard history: {err}"))
        .ok();
}

pub fn apply_settings(settings: ClipboardSettings) {
    let enabling = {
        let Ok(mut history) = history().lock() else {
            return;
        };
        let enabling = settings.enabled && !history.settings.enabled;
        history.settings = settings;
        enabling
    };
    if enabling {
        unlock();
    }
    if let Ok(mut history) = history().lock() {
        history.prune(now_secs());
        history.save();
    }
}

/// Reads the key and the saved history. Until this succeeds nothing is recorded.
///
/// The keychain can stop to ask the user for access, and the bar reads history on the main
/// thread, so the slow part runs without the history lock; only installing the result takes it.
fn unlock() {
    // One attempt at a time, so two callers never put up two keychain prompts.
    static UNLOCKING: Mutex<()> = Mutex::new(());
    let Ok(_unlocking) = UNLOCKING.lock() else {
        return;
    };
    let dir = match history().lock() {
        Ok(history) if history.cipher.is_none() => history.dir.clone(),
        _ => return,
    };
    // Nothing is recorded or saved while there's no key, so the file can't change under us.
    let result = Cipher::from_keychain().map(|cipher| {
        let saved = read_saved(&dir, &cipher);
        (cipher, saved)
    });
    let Ok(mut history) = history().lock() else {
        return;
    };
    // Unlocked some other way meanwhile: keep that key and what it loaded.
    if history.cipher.is_some() {
        return;
    }
    match result {
        Ok((cipher, saved)) => {
            if let Some(saved) = saved {
                history.items = saved.items;
                history.next_id = saved.next_id.max(1);
            }
            history.cipher = Some(Arc::new(cipher));
            history.problem = None;
            history.prune(now_secs());
        }
        Err(err) => {
            log::error!("clipboard history is off: {err}");
            history.problem = Some(err);
        }
    }
}

impl History {
    /// Hands a copy of the history to the saver thread. Encrypting and writing all of it takes
    /// a while for a big history, too long to hold the lock the bar reads through.
    fn save(&self) {
        let Some(cipher) = &self.cipher else {
            return;
        };
        saver().queue(Snapshot {
            path: self.dir.join("history.bin"),
            cipher: Arc::clone(cipher),
            saved: Saved {
                next_id: self.next_id,
                items: self.items.clone(),
            },
        });
    }

    fn record(&mut self, capture: Capture, now: i64) -> Option<u64> {
        let source_name = capture.source.as_ref().map(|(name, _)| name.clone());
        let (kind, text, files, png) = match capture.content {
            Content::Text(text) => {
                if text.trim().is_empty() || text.len() > MAX_TEXT_BYTES {
                    return None;
                }
                (classify(&text), text, Vec::new(), None)
            }
            Content::Files(files) if !files.is_empty() => {
                (Kind::Files, files.join("\n"), files, None)
            }
            Content::Files(_) => return None,
            Content::Image { png, width, height } => (
                Kind::Image,
                String::new(),
                Vec::new(),
                Some((png, width, height)),
            ),
        };
        let hash = {
            let mut hasher = Sha256::new();
            hasher.update(format!("{kind:?}"));
            hasher.update(text.as_bytes());
            if let Some((png, _, _)) = &png {
                hasher.update(png);
            }
            hasher
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };

        if let Some(existing) = self.items.iter_mut().find(|item| item.hash == hash) {
            existing.last_copied = now;
            existing.copies = existing.copies.saturating_add(1);
            if source_name.is_some() {
                existing.source = source_name;
            }
            return None;
        }

        let id = self.next_id;
        self.next_id += 1;
        let image = match png {
            Some((png, width, height)) => {
                let cipher = self.cipher.as_ref()?;
                let sealed = cipher.seal(&png).ok()?;
                write_atomic(&self.image_path(id), &sealed).ok()?;
                Some(ImageInfo {
                    width,
                    height,
                    bytes: png.len() as u64,
                })
            }
            None => None,
        };
        self.items.push(ClipItem {
            id,
            kind,
            text,
            files,
            image,
            ocr: None,
            source: source_name,
            first_copied: now,
            last_copied: now,
            copies: 1,
            pinned: false,
            hash,
        });
        self.prune(now);
        Some(id)
    }

    fn image_path(&self, id: u64) -> PathBuf {
        self.dir.join("images").join(format!("{id}.bin"))
    }

    fn image_png(&self, id: u64) -> Option<Vec<u8>> {
        let sealed = fs::read(self.image_path(id)).ok()?;
        self.cipher.as_ref()?.open(&sealed).ok()
    }

    /// Drops unpinned items past the retention period or the item limit.
    fn prune(&mut self, now: i64) {
        let retention = i64::from(self.settings.retention_days) * 86_400;
        let mut unpinned: Vec<(i64, u64)> = self
            .items
            .iter()
            .filter(|item| !item.pinned)
            .map(|item| (item.last_copied, item.id))
            .collect();
        unpinned.sort_unstable_by(|a, b| b.cmp(a));
        let mut drop: Vec<u64> = unpinned
            .iter()
            .enumerate()
            .filter(|(index, (last, _))| {
                *index >= MAX_ITEMS || (retention > 0 && now - last > retention)
            })
            .map(|(_, (_, id))| *id)
            .collect();
        drop.sort_unstable();
        if drop.is_empty() {
            return;
        }
        for id in &drop {
            let _ = fs::remove_file(self.image_path(*id));
        }
        self.items
            .retain(|item| drop.binary_search(&item.id).is_err());
    }

    fn ignores(&self, source: Option<&(String, String)>) -> bool {
        let Some((name, id)) = source else {
            return false;
        };
        // "KeePass.exe" and "KeePass" name the same Windows app.
        let bare = |app: &str| {
            let app = app.trim();
            app.strip_suffix(".exe")
                .or_else(|| app.strip_suffix(".EXE"))
                .unwrap_or(app)
                .to_lowercase()
        };
        let (name, id) = (bare(name), bare(id));
        self.settings.ignored_apps.iter().any(|ignored| {
            let ignored = bare(ignored);
            ignored == name || ignored == id
        })
    }
}

// Saving

/// History as it stood when a save was asked for, waiting to be written.
struct Snapshot {
    path: PathBuf,
    cipher: Arc<Cipher>,
    saved: Saved,
}

impl Snapshot {
    fn write(&self) {
        let result = serde_json::to_vec(&self.saved)
            .map_err(|err| err.to_string())
            .and_then(|plain| self.cipher.seal(&plain))
            .and_then(|sealed| write_atomic(&self.path, &sealed).map_err(|err| err.to_string()));
        if let Err(err) = result {
            log::error!("couldn't save clipboard history: {err}");
        }
    }
}

#[derive(Default)]
struct SaveQueue {
    /// The newest unwritten snapshot of each file.
    pending: Vec<Snapshot>,
    writing: bool,
}

impl SaveQueue {
    /// Only the latest state needs writing, so a newer snapshot replaces an older one still
    /// waiting: a burst of copies costs one write, not one each.
    fn push(&mut self, snapshot: Snapshot) {
        self.pending.retain(|older| older.path != snapshot.path);
        self.pending.push(snapshot);
    }
}

/// Writes history on one thread of its own, in the order saves were asked for, so an older
/// state never lands after a newer one. It writes as soon as asked, without waiting to batch,
/// so a save asked for just before quitting is on disk by the time the user gets there; quitting
/// mid-write leaves the previous file whole (`write_atomic`).
struct Saver {
    queue: Mutex<SaveQueue>,
    changed: Condvar,
    /// False if the thread couldn't start; saves are then written by whoever asks.
    threaded: bool,
}

/// On quit: lets a save still being written finish, so the last copy isn't lost.
pub fn flush() {
    saver().flush(Duration::from_secs(2));
}

fn saver() -> &'static Saver {
    static SAVER: OnceLock<Saver> = OnceLock::new();
    SAVER.get_or_init(|| {
        let threaded = std::thread::Builder::new()
            .name("clipboard-save".into())
            .spawn(|| saver().run())
            .map_err(|err| log::error!("couldn't start saving clipboard history: {err}"))
            .is_ok();
        Saver {
            queue: Mutex::new(SaveQueue::default()),
            changed: Condvar::new(),
            threaded,
        }
    })
}

impl Saver {
    fn queue(&self, snapshot: Snapshot) {
        if !self.threaded {
            snapshot.write();
            return;
        }
        if let Ok(mut queue) = self.queue.lock() {
            queue.push(snapshot);
            self.changed.notify_all();
        }
    }

    fn run(&self) {
        loop {
            let batch = {
                let Ok(queue) = self.queue.lock() else {
                    return;
                };
                let Ok(mut queue) = self
                    .changed
                    .wait_while(queue, |queue| queue.pending.is_empty())
                else {
                    return;
                };
                queue.writing = true;
                std::mem::take(&mut queue.pending)
            };
            for snapshot in &batch {
                snapshot.write();
            }
            let Ok(mut queue) = self.queue.lock() else {
                return;
            };
            queue.writing = false;
            self.changed.notify_all();
        }
    }

    /// Waits, up to `limit`, until everything asked for so far is written.
    fn flush(&self, limit: Duration) {
        if let Ok(queue) = self.queue.lock() {
            let _ = self.changed.wait_timeout_while(queue, limit, |queue| {
                !queue.pending.is_empty() || queue.writing
            });
        }
    }
}

fn read_saved(dir: &Path, cipher: &Cipher) -> Option<Saved> {
    let sealed = fs::read(dir.join("history.bin")).ok()?;
    match cipher
        .open(&sealed)
        .and_then(|plain| serde_json::from_slice(&plain).map_err(|err| err.to_string()))
    {
        Ok(saved) => Some(saved),
        Err(err) => {
            // Set aside rather than overwritten by the next save, so a key that comes back
            // (a restored keychain) can still read it.
            let aside = dir.join("history.unreadable.bin");
            let _ = fs::rename(dir.join("history.bin"), &aside);
            log::error!(
                "couldn't read clipboard history, starting over (kept it as {}): {err}",
                aside.display()
            );
            None
        }
    }
}

/// Links and colors get their own kinds so they can be filtered and previewed.
fn classify(text: &str) -> Kind {
    let trimmed = text.trim();
    if trimmed.contains(char::is_whitespace) {
        return Kind::Text;
    }
    if color_hex(trimmed).is_some() {
        return Kind::Color;
    }
    let lower = trimmed.to_ascii_lowercase();
    if ["https://", "http://", "mailto:", "ftp://"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
    {
        return Kind::Link;
    }
    Kind::Text
}

/// `#rgb`, `#rrggbb` or `#rrggbbaa` as a CSS color, or an `rgb()`/`hsl()` function.
pub fn color_hex(text: &str) -> Option<String> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix('#')
        && matches!(hex.len(), 3 | 6 | 8)
        && hex.chars().all(|ch| ch.is_ascii_hexdigit())
    {
        return Some(format!("#{}", hex.to_ascii_lowercase()));
    }
    let lower = text.to_ascii_lowercase();
    let function = ["rgb(", "rgba(", "hsl(", "hsla("]
        .iter()
        .any(|prefix| lower.starts_with(prefix));
    (function && lower.ends_with(')') && lower.len() <= 40).then_some(lower)
}

fn watch() {
    let platform = platform();
    let mut last = platform.change_count();
    let mut ticks: u32 = 0;
    loop {
        std::thread::sleep(POLL);
        ticks = ticks.wrapping_add(1);
        // macOS: each check's pasteboard objects are let go before the next, rather than
        // piling up for the life of the thread.
        #[cfg(target_os = "macos")]
        objc2::rc::autoreleasepool(|_| check(platform, &mut last, ticks));
        #[cfg(not(target_os = "macos"))]
        check(platform, &mut last, ticks);
    }
}

/// One look at the clipboard: records what was copied since the last.
fn check(platform: &dyn Platform, last: &mut i64, ticks: u32) {
    // The keychain refuses while the Mac is asleep or locked; try again every ~30 s, but
    // never re-ask after the user declined.
    if ticks.is_multiple_of(75)
        && history().lock().is_ok_and(|state| {
            state.settings.enabled
                && state.cipher.is_none()
                && state
                    .problem
                    .as_deref()
                    .is_some_and(|problem| problem.contains("UI"))
        })
    {
        unlock();
    }
    let current = platform.change_count();
    if current == *last {
        return;
    }
    *last = current;
    {
        let Ok(mut state) = history().lock() else {
            return;
        };
        if !state.settings.enabled || state.cipher.is_none() {
            return;
        }
        if state.own_write.take() == Some(current) {
            return;
        }
    }

    // Reading can be slow for big images, so it happens outside the lock.
    let Some(capture) = platform.read() else {
        return;
    };
    let Ok(mut history) = history().lock() else {
        return;
    };
    if history.ignores(capture.source.as_ref()) {
        return;
    }
    let image = match &capture.content {
        Content::Image { png, .. } if history.settings.recognize_text => Some(png.clone()),
        _ => None,
    };
    let added = history.record(capture, now_secs());
    history.save();
    drop(history);

    if let (Some(id), Some(png)) = (added, image) {
        recognize_later(id, png);
    }
}

/// Text recognition takes a moment, so it fills in after the image is already listed.
fn recognize_later(id: u64, png: Vec<u8>) {
    std::thread::spawn(move || {
        let Some(text) = platform().recognize_text(&png) else {
            return;
        };
        if let Ok(mut history) = history().lock()
            && let Some(item) = history.items.iter_mut().find(|item| item.id == id)
        {
            item.ocr = Some(text);
            history.save();
        }
    });
}

// Queries

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipSummary {
    pub id: u64,
    pub kind: Kind,
    pub title: String,
    pub source: Option<String>,
    pub last_copied: i64,
    pub copies: u32,
    pub pinned: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipList {
    pub items: Vec<ClipSummary>,
    /// Why history is empty or off, when it is.
    pub problem: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipDetail {
    pub id: u64,
    pub kind: Kind,
    pub text: String,
    pub files: Vec<String>,
    /// A `data:image/png;base64,...` URL for images.
    pub image_url: Option<String>,
    pub image: Option<ImageInfo>,
    pub ocr: Option<String>,
    pub source: Option<String>,
    pub first_copied: i64,
    pub last_copied: i64,
    pub copies: u32,
    pub pinned: bool,
    pub color: Option<String>,
}

pub fn list(query: &str, filter: &str, limit: usize) -> ClipList {
    let Ok(history) = history().lock() else {
        return ClipList {
            items: Vec::new(),
            problem: Some("Clipboard history is unavailable".into()),
            enabled: false,
        };
    };
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut matches: Vec<&ClipItem> = history
        .items
        .iter()
        .filter(|item| match filter {
            "pinned" => item.pinned,
            "text" => item.kind == Kind::Text,
            "link" => item.kind == Kind::Link,
            "color" => item.kind == Kind::Color,
            "image" => item.kind == Kind::Image,
            "files" => item.kind == Kind::Files,
            _ => true,
        })
        .filter(|item| words.is_empty() || matches_words(item, &words))
        .collect();
    matches.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then(b.last_copied.cmp(&a.last_copied))
            .then(b.id.cmp(&a.id))
    });
    ClipList {
        items: matches.into_iter().take(limit).map(summary).collect(),
        problem: history.problem.clone(),
        enabled: history.settings.enabled,
    }
}

fn matches_words(item: &ClipItem, words: &[String]) -> bool {
    let haystack = format!(
        "{}\n{}\n{}",
        item.text,
        item.ocr.as_deref().unwrap_or_default(),
        item.source.as_deref().unwrap_or_default()
    )
    .to_lowercase();
    words.iter().all(|word| haystack.contains(word.as_str()))
}

fn summary(item: &ClipItem) -> ClipSummary {
    let title = match item.kind {
        Kind::Image => match &item.image {
            Some(image) => format!("Image ({}×{})", image.width, image.height),
            None => "Image".into(),
        },
        Kind::Files => item
            .files
            .iter()
            .map(|path| file_name(path))
            .collect::<Vec<_>>()
            .join(", "),
        _ => item
            .text
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or_default()
            .to_string(),
    };
    ClipSummary {
        id: item.id,
        kind: item.kind,
        title: truncate(&title, 160),
        source: item.source.clone(),
        last_copied: item.last_copied,
        copies: item.copies,
        pinned: item.pinned,
        color: (item.kind == Kind::Color)
            .then(|| color_hex(&item.text))
            .flatten(),
        image: item.image,
    }
}

fn file_name(path: &str) -> &str {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(max).collect();
    cut.push('…');
    cut
}

pub fn detail(id: u64) -> Result<ClipDetail, String> {
    let history = history()
        .lock()
        .map_err(|_| "Clipboard history is unavailable")?;
    let item = history
        .items
        .iter()
        .find(|item| item.id == id)
        .ok_or("That item is no longer in your history")?;
    let image_url = (item.kind == Kind::Image)
        .then(|| history.image_png(id))
        .flatten()
        .map(|png| format!("data:image/png;base64,{}", STANDARD.encode(png)));
    Ok(ClipDetail {
        id: item.id,
        kind: item.kind,
        text: truncate(&item.text, 100_000),
        files: item.files.clone(),
        image_url,
        image: item.image,
        ocr: item.ocr.clone(),
        source: item.source.clone(),
        first_copied: item.first_copied,
        last_copied: item.last_copied,
        copies: item.copies,
        pinned: item.pinned,
        color: (item.kind == Kind::Color)
            .then(|| color_hex(&item.text))
            .flatten(),
    })
}

/// Files to drag a history item out as: a files item's own paths, an image as a PNG and text
/// (or a link or color) as a .txt, both written into `dir`.
pub fn drag_files(id: u64, dir: &Path) -> Result<Vec<String>, String> {
    let history = history()
        .lock()
        .map_err(|_| "Clipboard history is unavailable")?;
    let item = history
        .items
        .iter()
        .find(|item| item.id == id)
        .ok_or("That item is no longer in your history")?;
    if item.kind == Kind::Files {
        return Ok(item.files.clone());
    }
    fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    let (name, bytes) = if item.kind == Kind::Image {
        let png = history
            .image_png(id)
            .ok_or("That image is no longer in your history")?;
        (format!("Clipboard image {id}.png"), png)
    } else {
        (
            format!("Clipboard text {id}.txt"),
            item.text.clone().into_bytes(),
        )
    };
    let path = dir.join(name);
    fs::write(&path, bytes).map_err(|err| err.to_string())?;
    Ok(vec![path.to_string_lossy().into_owned()])
}

// Actions

/// Puts an item back on the clipboard and moves it to the top of history.
pub fn copy(id: u64, plain: bool) -> Result<(), String> {
    let mut history = history()
        .lock()
        .map_err(|_| "Clipboard history is unavailable")?;
    let item = history
        .items
        .iter()
        .find(|item| item.id == id)
        .cloned()
        .ok_or("That item is no longer in your history")?;
    let content = match item.kind {
        Kind::Image => Content::Image {
            png: history.image_png(id).ok_or("That image couldn't be read")?,
            width: item.image.map_or(0, |image| image.width),
            height: item.image.map_or(0, |image| image.height),
        },
        Kind::Files if !plain => Content::Files(item.files.clone()),
        _ => Content::Text(item.text.clone()),
    };
    platform().write(&content, plain)?;
    history.own_write = Some(platform().change_count());
    let now = now_secs();
    if let Some(entry) = history.items.iter_mut().find(|entry| entry.id == id) {
        entry.last_copied = now;
    }
    history.save();
    Ok(())
}

pub enum PasteOutcome {
    Pasted,
    /// Copied, but keystrokes can't be sent yet.
    NeedsPermission,
}

/// Copies an item, then pastes it into the app in front once the bar is out of the way.
pub fn paste(id: u64, plain: bool, hide: impl FnOnce()) -> Result<PasteOutcome, String> {
    copy(id, plain)?;
    if !platform().can_paste(false) {
        platform().can_paste(true);
        return Ok(PasteOutcome::NeedsPermission);
    }
    hide();
    std::thread::spawn(|| {
        // Give the previous app a moment to take focus back from the bar.
        std::thread::sleep(Duration::from_millis(120));
        if let Err(err) = platform().send_paste() {
            log::error!("couldn't paste: {err}");
        }
    });
    Ok(PasteOutcome::Pasted)
}

pub fn can_paste() -> bool {
    platform().can_paste(false)
}

/// Puts a PNG on the clipboard as an image, as a debug snapshot does. History records it like
/// any other copy.
pub fn copy_png(png: Vec<u8>) -> Result<(), String> {
    // The size is in the PNG's header chunk, right after the signature.
    let size = |at: usize| {
        png.get(at..at + 4)
            .map(|bytes| u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    };
    let (Some(width), Some(height)) = (size(16), size(20)) else {
        return Err("That isn't a PNG".into());
    };
    platform().write(&Content::Image { png, width, height }, false)
}

pub fn request_paste_permission() -> bool {
    platform().can_paste(true)
}

pub fn set_pinned(id: u64, pinned: bool) -> Result<(), String> {
    let mut history = history()
        .lock()
        .map_err(|_| "Clipboard history is unavailable")?;
    let item = history
        .items
        .iter_mut()
        .find(|item| item.id == id)
        .ok_or("That item is no longer in your history")?;
    item.pinned = pinned;
    history.save();
    Ok(())
}

pub fn delete(id: u64) -> Result<(), String> {
    let mut history = history()
        .lock()
        .map_err(|_| "Clipboard history is unavailable")?;
    let _ = fs::remove_file(history.image_path(id));
    history.items.retain(|item| item.id != id);
    history.save();
    Ok(())
}

/// Clears history; pinned items stay unless `pinned_too`.
pub fn clear(pinned_too: bool) -> Result<usize, String> {
    let mut history = history()
        .lock()
        .map_err(|_| "Clipboard history is unavailable")?;
    let doomed: Vec<u64> = history
        .items
        .iter()
        .filter(|item| pinned_too || !item.pinned)
        .map(|item| item.id)
        .collect();
    for id in &doomed {
        let _ = fs::remove_file(history.image_path(*id));
    }
    history.items.retain(|item| !pinned_too && item.pinned);
    history.save();
    Ok(doomed.len())
}

/// Counts by kind, for the settings page.
pub fn stats() -> HashMap<&'static str, usize> {
    let mut counts = HashMap::new();
    if let Ok(history) = history().lock() {
        counts.insert("total", history.items.len());
        counts.insert(
            "pinned",
            history.items.iter().filter(|item| item.pinned).count(),
        );
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> History {
        let dir =
            std::env::temp_dir().join(format!("zephyr-clip-test-{}", getrandom::u64().unwrap()));
        History {
            items: Vec::new(),
            next_id: 1,
            dir,
            cipher: Some(Arc::new(Cipher::from_key(&[3u8; 32]).unwrap())),
            settings: ClipboardSettings::default(),
            own_write: None,
            problem: None,
        }
    }

    fn text(value: &str) -> Capture {
        Capture {
            content: Content::Text(value.into()),
            source: Some(("Notes".into(), "com.apple.Notes".into())),
        }
    }

    #[test]
    fn classifies_links_colors_and_text() {
        assert_eq!(classify("https://zephyr.dev/x"), Kind::Link);
        assert_eq!(classify("#FF8800"), Kind::Color);
        assert_eq!(classify("rgb(1, 2, 3)"), Kind::Text); // has spaces
        assert_eq!(classify("rgb(1,2,3)"), Kind::Color);
        assert_eq!(classify("hello world"), Kind::Text);
        assert_eq!(color_hex("#ABC").as_deref(), Some("#abc"));
        assert_eq!(color_hex("#ABCD"), None);
    }

    #[test]
    fn copying_the_same_thing_again_moves_it_up_instead_of_duplicating() {
        let mut history = fresh();
        history.record(text("one"), 100);
        history.record(text("two"), 200);
        assert_eq!(history.record(text("one"), 300), None);
        assert_eq!(history.items.len(), 2);
        let one = history
            .items
            .iter()
            .find(|item| item.text == "one")
            .unwrap();
        assert_eq!((one.copies, one.last_copied), (2, 300));
    }

    #[test]
    fn prunes_old_unpinned_items_but_keeps_pins() {
        let mut history = fresh();
        history.settings.retention_days = 1;
        history.record(text("old"), 0);
        history.record(text("pinned"), 0);
        history
            .items
            .iter_mut()
            .find(|item| item.text == "pinned")
            .unwrap()
            .pinned = true;
        history.record(text("new"), 2 * 86_400);
        let left: Vec<&str> = history
            .items
            .iter()
            .map(|item| item.text.as_str())
            .collect();
        assert_eq!(left, vec!["pinned", "new"]);
    }

    #[test]
    fn ignores_listed_apps_by_name_or_id() {
        let history = fresh();
        assert!(history.ignores(Some(&(
            "1Password".into(),
            "com.1password.1password".into()
        ))));
        assert!(!history.ignores(Some(&("Notes".into(), "com.apple.Notes".into()))));
        assert!(!history.ignores(None));
        // Windows sources are (exe stem, exe name).
        assert!(history.ignores(Some(&("KeePass".into(), "KeePass.exe".into()))));
        let mut by_exe = fresh();
        by_exe.settings.ignored_apps = vec!["Notepad.exe".into()];
        assert!(by_exe.ignores(Some(&("notepad".into(), "notepad.exe".into()))));
    }

    #[test]
    fn saves_encrypted_and_reads_back() {
        let mut history = fresh();
        history.record(text("secret note"), 100);
        history.save();
        saver().flush(Duration::from_secs(10));
        let raw = fs::read(history.dir.join("history.bin")).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("secret note"));
        let saved = read_saved(&history.dir, history.cipher.as_ref().unwrap()).unwrap();
        assert_eq!(saved.items[0].text, "secret note");
        let _ = fs::remove_dir_all(&history.dir);
    }

    #[test]
    fn a_newer_save_replaces_one_still_waiting() {
        let history = fresh();
        let snapshot = |path: &str, next_id| Snapshot {
            path: path.into(),
            cipher: Arc::clone(history.cipher.as_ref().unwrap()),
            saved: Saved {
                next_id,
                items: Vec::new(),
            },
        };
        let mut queue = SaveQueue::default();
        queue.push(snapshot("a", 1));
        queue.push(snapshot("b", 2));
        queue.push(snapshot("a", 3));
        let pending: Vec<(String, u64)> = queue
            .pending
            .iter()
            .map(|it| (it.path.display().to_string(), it.saved.next_id))
            .collect();
        assert_eq!(pending, vec![("b".into(), 2), ("a".into(), 3)]);
    }

    #[test]
    fn rapid_saves_end_with_the_latest_state_on_disk() {
        let mut history = fresh();
        for n in 0..50 {
            history.record(text(&format!("note {n}")), n);
            history.save();
        }
        saver().flush(Duration::from_secs(10));
        let saved = read_saved(&history.dir, history.cipher.as_ref().unwrap()).unwrap();
        assert_eq!(saved.items.len(), 50);
        assert_eq!(saved.next_id, 51);
        let _ = fs::remove_dir_all(&history.dir);
    }

    #[test]
    fn searches_text_and_recognized_text() {
        let mut history = fresh();
        history.record(text("alpha beta"), 1);
        let mut item = history.items[0].clone();
        item.ocr = Some("invoice total".into());
        assert!(matches_words(&item, &["invoice".into()]));
        assert!(matches_words(&item, &["alpha".into(), "beta".into()]));
        assert!(!matches_words(&item, &["gamma".into()]));
    }
}
