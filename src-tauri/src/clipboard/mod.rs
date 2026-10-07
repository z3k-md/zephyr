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
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crypto::Cipher;

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
    /// Whether Zephyr may send keystrokes; `prompt` asks the OS to show its permission dialog.
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

/// Windows needs a window to own what Zephyr writes to the clipboard; the bar's will do.
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
    cipher: Option<Cipher>,
    settings: ClipboardSettings,
    /// The change count of Zephyr's own last write, which is not a new copy.
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

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// Loads saved history and starts watching the clipboard.
pub fn start(dir: PathBuf, settings: ClipboardSettings) {
    {
        let Ok(mut history) = history().lock() else {
            return;
        };
        history.dir = dir;
        history.settings = settings;
        if history.settings.enabled {
            history.unlock();
        }
    }
    std::thread::Builder::new()
        .name("clipboard".into())
        .spawn(watch)
        .map_err(|err| log::error!("couldn't start clipboard history: {err}"))
        .ok();
}

pub fn apply_settings(settings: ClipboardSettings) {
    if let Ok(mut history) = history().lock() {
        let enabling = settings.enabled && !history.settings.enabled;
        history.settings = settings;
        if enabling {
            history.unlock();
        }
        history.prune(now_secs());
        history.save();
    }
}

impl History {
    /// Reads the key and the saved history. Until this succeeds nothing is recorded.
    fn unlock(&mut self) {
        if self.cipher.is_some() {
            return;
        }
        match Cipher::from_keychain() {
            Ok(cipher) => {
                if let Some(saved) = read_saved(&self.dir, &cipher) {
                    self.items = saved.items;
                    self.next_id = saved.next_id.max(1);
                }
                self.cipher = Some(cipher);
                self.problem = None;
                self.prune(now_secs());
            }
            Err(err) => {
                log::error!("clipboard history is off: {err}");
                self.problem = Some(err);
            }
        }
    }

    fn save(&self) {
        let Some(cipher) = &self.cipher else {
            return;
        };
        let saved = Saved {
            next_id: self.next_id,
            items: self.items.clone(),
        };
        let result = serde_json::to_vec(&saved)
            .map_err(|err| err.to_string())
            .and_then(|plain| cipher.seal(&plain))
            .and_then(|sealed| write_atomic(&self.dir.join("history.bin"), &sealed));
        if let Err(err) = result {
            log::error!("couldn't save clipboard history: {err}");
        }
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
            format!("{:x}", hasher.finalize())
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
                fs::create_dir_all(self.dir.join("images")).ok()?;
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

fn read_saved(dir: &Path, cipher: &Cipher) -> Option<Saved> {
    let sealed = fs::read(dir.join("history.bin")).ok()?;
    match cipher
        .open(&sealed)
        .and_then(|plain| serde_json::from_slice(&plain).map_err(|err| err.to_string()))
    {
        Ok(saved) => Some(saved),
        Err(err) => {
            log::error!("couldn't read clipboard history, starting over: {err}");
            None
        }
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, data).map_err(|err| err.to_string())?;
    fs::rename(&tmp, path).map_err(|err| err.to_string())
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
        // The keychain refuses while the Mac is asleep or locked; try again every ~30 s, but
        // never re-ask after the user declined.
        if ticks.is_multiple_of(75)
            && let Ok(mut state) = history().lock()
            && state.settings.enabled
            && state.cipher.is_none()
            && state
                .problem
                .as_deref()
                .is_some_and(|problem| problem.contains("UI"))
        {
            state.unlock();
        }
        let current = platform.change_count();
        if current == last {
            continue;
        }
        last = current;
        {
            let Ok(mut state) = history().lock() else {
                continue;
            };
            if !state.settings.enabled || state.cipher.is_none() {
                continue;
            }
            if state.own_write.take() == Some(current) {
                continue;
            }
        }

        // Reading can be slow for big images, so it happens outside the lock.
        let Some(capture) = platform.read() else {
            continue;
        };
        let Ok(mut history) = history().lock() else {
            continue;
        };
        if history.ignores(capture.source.as_ref()) {
            continue;
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
    /// Copied, but Zephyr may not send keystrokes yet.
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
            cipher: Some(Cipher::from_key(&[3u8; 32]).unwrap()),
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
        let raw = fs::read(history.dir.join("history.bin")).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("secret note"));
        let saved = read_saved(&history.dir, history.cipher.as_ref().unwrap()).unwrap();
        assert_eq!(saved.items[0].text, "secret note");
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
