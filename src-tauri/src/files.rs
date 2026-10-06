use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, RwLock, mpsc};
use std::time::{Duration, Instant};

use ignore::{WalkBuilder, WalkState};
use notify::{RecursiveMode, Watcher};

use crate::apps::{self, LaunchEntry, Strength};

/// Bang words that scope the bar to files instead of naming a destination.
pub const SCOPE_TRIGGERS: &[&str] = &["f", "file", "files"];

pub fn is_scope(trigger: &str) -> bool {
    SCOPE_TRIGGERS.contains(&trigger)
}

/// A home folder this large is unusual; stop there rather than grow without bound.
const MAX_ENTRIES: usize = 1_000_000;
/// The watcher keeps the index current, so a full rescan is only a safety net.
const STALE_AFTER: Duration = Duration::from_secs(6 * 60 * 60);
const UNWATCHED_STALE_AFTER: Duration = Duration::from_secs(15 * 60);
/// Changes are applied in batches, at most this long after the first one arrives.
const SETTLE: Duration = Duration::from_millis(500);
const FORCED_RESCAN_GAP: Duration = Duration::from_secs(10 * 60);
/// More changes than this in one burst (a checkout, an unzip) are cheaper to rescan than patch.
const RESCAN_ABOVE: usize = 256;
const WALK_THREADS: usize = 4;
const SEARCH_THREADS: usize = 4;
const PARALLEL_ABOVE: usize = 50_000;

/// A matching entry: name strength (none when only folders matched), open frecency, position.
type Match = (Option<Strength>, f64, usize);

/// Folders skipped wherever they appear: package caches nobody opens by hand.
const SKIPPED_DIRS: &[&str] = &["node_modules", "__pycache__", "bower_components"];

/// Folders directly under home that hold app data rather than the user's files.
#[cfg(target_os = "macos")]
const HOME_SKIPPED: &[&str] = &["Library"];
#[cfg(windows)]
const HOME_SKIPPED: &[&str] = &["AppData"];
#[cfg(not(any(windows, target_os = "macos")))]
const HOME_SKIPPED: &[&str] = &[];

const CACHE_HEADER: &str = "zephyr-files 1";

/// Which folders are indexed and which are left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub roots: Vec<PathBuf>,
    pub excludes: Vec<PathBuf>,
}

impl Config {
    /// No saved roots means the default: the home folder.
    pub fn resolve(roots: Option<&[String]>, excludes: &[String]) -> Self {
        let roots = match roots {
            Some(roots) => roots.iter().map(PathBuf::from).collect(),
            None => home().into_iter().collect(),
        };
        Self {
            roots,
            excludes: excludes.iter().map(PathBuf::from).collect(),
        }
    }
}

pub fn home() -> Option<PathBuf> {
    std::env::home_dir().filter(|path| path.is_absolute())
}

/// Turns typed folders into absolute, existing, de-duplicated paths.
pub fn normalize_folders(folders: &[String]) -> Result<Vec<String>, String> {
    let mut normalized: Vec<String> = Vec::new();
    for folder in folders {
        let trimmed = folder.trim();
        if trimmed.is_empty() {
            continue;
        }
        let path = match trimmed.strip_prefix('~') {
            Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
                let home = home().ok_or("Couldn't find your home folder")?;
                home.join(rest.trim_start_matches(['/', '\\']))
            }
            _ => PathBuf::from(trimmed),
        };
        if !path.is_absolute() || !path.is_dir() {
            return Err(format!("{trimmed} isn't a folder"));
        }
        let text = path
            .to_str()
            .ok_or_else(|| format!("{trimmed} has a name Zephyr can't read"))?
            .trim_end_matches(['/', '\\'])
            .to_string();
        // A bare drive or "/" loses its only separator above; keep it.
        let text = if text.is_empty() || text.ends_with(':') {
            format!("{text}{}", std::path::MAIN_SEPARATOR)
        } else {
            text
        };
        if !normalized.contains(&text) {
            normalized.push(text);
        }
    }
    Ok(normalized)
}

/// One indexed file or folder, stored relative to its root to keep the index small.
#[derive(Debug, Clone)]
struct Entry {
    root: u16,
    rel: Box<str>,
    /// `rel` lowercased; the name starts at `name_at`.
    lower: Box<str>,
    name_at: u32,
    depth: u16,
    dir: bool,
}

fn is_separator(ch: char) -> bool {
    ch == '/' || (cfg!(windows) && ch == '\\')
}

impl Entry {
    fn new(root: u16, rel: &str, dir: bool) -> Self {
        let split = rel.rfind(is_separator).map_or(0, |at| at + 1);
        let mut lower = rel[..split].to_lowercase();
        let name_at = lower.len() as u32;
        lower.push_str(&rel[split..].to_lowercase());
        let depth = rel.chars().filter(|ch| is_separator(*ch)).count();
        Self {
            root,
            rel: rel.into(),
            lower: lower.into(),
            name_at,
            depth: depth.min(u16::MAX as usize) as u16,
            dir,
        }
    }

    fn name(&self) -> &str {
        let split = self.rel.rfind(is_separator).map_or(0, |at| at + 1);
        &self.rel[split..]
    }

    fn lower_name(&self) -> &str {
        &self.lower[self.name_at as usize..]
    }

    /// Whether this entry is `rel` itself or inside it.
    fn within(&self, root: u16, rel: &str) -> bool {
        self.root == root
            && self.rel.starts_with(rel)
            && (self.rel.len() == rel.len() || self.rel[rel.len()..].starts_with(is_separator))
    }
}

/// A search result, resolved to a full path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: String,
    pub name: String,
    pub dir: bool,
}

impl Hit {
    fn from_path(path: &str) -> Option<Self> {
        let as_path = Path::new(path);
        let name = as_path.file_name()?.to_str()?.to_string();
        Some(Self {
            path: path.to_string(),
            name,
            dir: as_path.is_dir(),
        })
    }

    /// The containing folder, with the home folder shortened to `~`.
    pub fn location(&self) -> String {
        let parent = Path::new(&self.path)
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned())
            .unwrap_or_default();
        match home().and_then(|home| home.to_str().map(str::to_string)) {
            Some(home) if parent == home => "~".into(),
            Some(home)
                if parent.starts_with(&home) && parent[home.len()..].starts_with(is_separator) =>
            {
                format!("~{}", &parent[home.len()..])
            }
            _ => parent,
        }
    }
}

/// How well `query` (lowercased) names a file called `name` (lowercased).
pub fn name_strength(name: &str, query: &str) -> Option<Strength> {
    if query.is_empty() {
        return None;
    }
    let stem = match name.rfind('.') {
        Some(at) if at > 0 => &name[..at],
        _ => name,
    };
    if name == query || stem == query {
        return Some(Strength::Exact);
    }
    if name.starts_with(query) {
        return Some(Strength::Prefix);
    }
    let mut found = false;
    for (at, _) in name.match_indices(query) {
        found = true;
        let before = name[..at].chars().next_back();
        if before.is_none_or(|ch| !ch.is_alphanumeric()) {
            return Some(Strength::WordPrefix);
        }
    }
    found.then_some(Strength::Substring)
}

/// How an entry matches the query's words: by its name (with a strength), or only through the
/// folders above it. Every word must appear in the path, and at least one in the name.
fn entry_match(entry: &Entry, words: &[String], whole: &str) -> Option<Option<Strength>> {
    if !words.iter().all(|word| entry.lower.contains(word.as_str())) {
        return None;
    }
    let name = entry.lower_name();
    if !words.iter().any(|word| name.contains(word.as_str())) {
        return None;
    }
    if let Some(found) = name_strength(name, whole) {
        return Some(Some(found));
    }
    if words.iter().all(|word| name.contains(word.as_str())) {
        return Some(Some(Strength::Substring));
    }
    Some(None)
}

fn words(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

/// The folder rules shared by full walks and change events.
#[derive(Debug, Clone)]
struct Rules {
    excludes: Vec<PathBuf>,
    home: Option<PathBuf>,
}

impl Rules {
    fn new(config: &Config) -> Self {
        Self {
            excludes: config.excludes.clone(),
            home: home(),
        }
    }

    fn allows(&self, path: &Path, dir: bool) -> bool {
        if self
            .excludes
            .iter()
            .any(|exclude| path.starts_with(exclude))
        {
            return false;
        }
        if !dir {
            return true;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return false;
        };
        if SKIPPED_DIRS.contains(&name) {
            return false;
        }
        if HOME_SKIPPED.contains(&name) && self.home.as_deref() == path.parent() {
            return false;
        }
        // Build output and caches mark themselves (Cargo's target, among others).
        !path.join("CACHEDIR.TAG").exists()
    }

    /// A cheap first pass over a changed path, before touching the disk: anything hidden or
    /// inside a skipped folder can be dropped without a stat.
    fn may_allow(&self, root: &Path, rel: &str) -> bool {
        if self
            .excludes
            .iter()
            .any(|exclude| root.join(rel).starts_with(exclude))
        {
            return false;
        }
        let at_home = self.home.as_deref() == Some(root);
        rel.split(is_separator).enumerate().all(|(depth, part)| {
            !part.starts_with('.')
                && !SKIPPED_DIRS.contains(&part)
                && !(at_home && depth == 0 && HOME_SKIPPED.contains(&part))
        })
    }

    fn walker(&self, path: &Path, max_depth: Option<usize>) -> WalkBuilder {
        let mut builder = WalkBuilder::new(path);
        builder
            .hidden(true)
            .parents(true)
            .ignore(true)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .follow_links(false)
            .max_depth(max_depth)
            .threads(WALK_THREADS);
        let rules = self.clone();
        builder.filter_entry(move |entry| {
            let dir = entry.file_type().is_some_and(|kind| kind.is_dir());
            entry.depth() == 0 || rules.allows(entry.path(), dir)
        });
        builder
    }
}

/// Walks `from` (inside `roots[root]`) and returns what it holds, not `from` itself.
fn walk_under(
    rules: &Rules,
    roots: &[PathBuf],
    root: u16,
    from: &Path,
    max_depth: Option<usize>,
    budget: &AtomicUsize,
) -> Vec<Entry> {
    let base = roots[root as usize].clone();
    let (sender, receiver) = mpsc::channel();
    rules.walker(from, max_depth).build_parallel().run(|| {
        let sender = sender.clone();
        let base = base.clone();
        Box::new(move |result| {
            let Ok(found) = result else {
                return WalkState::Continue;
            };
            if found.depth() == 0 {
                return WalkState::Continue;
            }
            if budget.fetch_add(1, Ordering::Relaxed) >= MAX_ENTRIES {
                return WalkState::Quit;
            }
            let dir = found.file_type().is_some_and(|kind| kind.is_dir());
            if let Some(rel) = found.path().strip_prefix(&base).ok().and_then(Path::to_str) {
                let _ = sender.send(Entry::new(root, rel, dir));
            }
            WalkState::Continue
        })
    });
    drop(sender);
    receiver.into_iter().collect()
}

fn walk(config: &Config) -> Vec<Entry> {
    let rules = Rules::new(config);
    let budget = AtomicUsize::new(0);
    let mut entries = Vec::new();
    for (root, path) in config.roots.iter().enumerate() {
        if root > u16::MAX as usize {
            break;
        }
        entries.extend(walk_under(
            &rules,
            &config.roots,
            root as u16,
            path,
            None,
            &budget,
        ));
    }
    if entries.len() >= MAX_ENTRIES {
        log::warn!("file index stopped at {MAX_ENTRIES} items");
    }
    entries
}

#[derive(Default)]
struct Contents {
    config: Config,
    entries: Vec<Entry>,
}

impl Contents {
    /// Splits a full path into its root index and the path below that root.
    fn locate<'p>(&self, path: &'p Path) -> Option<(u16, &'p str)> {
        // The deepest root wins when roots nest.
        let (root, base) = self
            .config
            .roots
            .iter()
            .enumerate()
            .filter(|(_, root)| path.starts_with(root))
            .max_by_key(|(_, root)| root.as_os_str().len())?;
        let rel = path.strip_prefix(base).ok()?.to_str()?;
        Some((root as u16, rel))
    }

    fn full_path(&self, entry: &Entry) -> String {
        self.config.roots[entry.root as usize]
            .join(&*entry.rel)
            .to_string_lossy()
            .into_owned()
    }

    fn contains(&self, path: &Path) -> bool {
        let Some((root, rel)) = self.locate(path) else {
            return false;
        };
        !rel.is_empty()
            && self
                .entries
                .iter()
                .any(|entry| entry.root == root && &*entry.rel == rel)
    }

    fn search(&self, query: &str, opens: &[LaunchEntry], now: i64, limit: usize) -> Vec<Hit> {
        let words = words(query);
        if words.is_empty() || limit == 0 {
            return Vec::new();
        }
        let whole = words.join(" ");
        let used: HashMap<(u16, &str), f64> = opens
            .iter()
            .filter_map(|open| {
                let (root, rel) = self.locate(Path::new(&open.app_id))?;
                Some(((root, rel), apps::frecency(opens, &open.app_id, now)))
            })
            .collect();

        // Most entries were never opened; skip the hash lookup unless the length could match.
        let opened_lengths: HashSet<usize> = used.keys().map(|(_, rel)| rel.len()).collect();
        let score = |entry: &Entry| {
            if opened_lengths.contains(&entry.rel.len()) {
                used.get(&(entry.root, &*entry.rel)).copied().unwrap_or(0.0)
            } else {
                0.0
            }
        };
        let order = |left: &Match, right: &Match| {
            let (a, b) = (&self.entries[left.2], &self.entries[right.2]);
            right
                .0
                .cmp(&left.0)
                .then(right.1.total_cmp(&left.1))
                .then(a.depth.cmp(&b.depth))
                .then(a.lower_name().len().cmp(&b.lower_name().len()))
                .then(a.lower.cmp(&b.lower))
        };
        let best_in = |start: usize, chunk: &[Entry]| {
            let mut found: Vec<Match> = chunk
                .iter()
                .enumerate()
                .filter_map(|(offset, entry)| {
                    let strength = entry_match(entry, &words, &whole)?;
                    Some((strength, score(entry), start + offset))
                })
                .collect();
            if found.len() > limit {
                found.select_nth_unstable_by(limit - 1, order);
                found.truncate(limit);
            }
            found
        };

        // Split large indexes across a few threads so a keystroke stays well under 50 ms.
        let chunk_size = self
            .entries
            .len()
            .div_ceil(SEARCH_THREADS)
            .max(PARALLEL_ABOVE);
        let mut matches: Vec<Match> = if self.entries.len() > PARALLEL_ABOVE {
            std::thread::scope(|scope| {
                let workers: Vec<_> = self
                    .entries
                    .chunks(chunk_size)
                    .enumerate()
                    .map(|(at, chunk)| scope.spawn(move || best_in(at * chunk_size, chunk)))
                    .collect();
                workers
                    .into_iter()
                    .flat_map(|worker| worker.join().unwrap_or_default())
                    .collect()
            })
        } else {
            best_in(0, &self.entries)
        };
        if matches.len() > limit {
            matches.select_nth_unstable_by(limit - 1, order);
            matches.truncate(limit);
        }
        matches.sort_by(order);
        matches
            .into_iter()
            .map(|(_, _, index)| {
                let entry = &self.entries[index];
                Hit {
                    path: self.full_path(entry),
                    name: entry.name().to_string(),
                    dir: entry.dir,
                }
            })
            .collect()
    }
}

/// Files Zephyr opened before, most used first, skipping ones that are gone.
pub fn recent(opens: &[LaunchEntry], now: i64, limit: usize) -> Vec<Hit> {
    let mut ranked: Vec<(&LaunchEntry, f64)> = opens
        .iter()
        .map(|open| (open, apps::frecency(opens, &open.app_id, now)))
        .collect();
    ranked.sort_by(|left, right| right.1.total_cmp(&left.1));
    ranked
        .into_iter()
        .filter(|(open, _)| Path::new(&open.app_id).exists())
        .filter_map(|(open, _)| Hit::from_path(&open.app_id))
        .take(limit)
        .collect()
}

/// Files worth a row under unscoped results: only ones opened through Zephyr before whose name
/// clearly matches, so ordinary searches never sprout file rows or wait on the index.
pub fn incidental(opens: &[LaunchEntry], query: &str, now: i64, limit: usize) -> Vec<Hit> {
    let query = query.trim().to_lowercase();
    if query.chars().count() < 3 {
        return Vec::new();
    }
    let mut matches: Vec<(Strength, f64, &LaunchEntry)> = opens
        .iter()
        .filter_map(|open| {
            let name = Path::new(&open.app_id)
                .file_name()?
                .to_str()?
                .to_lowercase();
            let found =
                name_strength(&name, &query).filter(|found| *found >= Strength::WordPrefix)?;
            Some((found, apps::frecency(opens, &open.app_id, now), open))
        })
        .collect();
    matches.sort_by(|left, right| right.0.cmp(&left.0).then(right.1.total_cmp(&left.1)));
    matches
        .into_iter()
        .filter(|(_, _, open)| Path::new(&open.app_id).exists())
        .filter_map(|(_, _, open)| Hit::from_path(&open.app_id))
        .take(limit)
        .collect()
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub roots: Vec<String>,
    pub entries: usize,
    pub scanning: bool,
}

/// An in-memory index of file and folder names under the configured roots. A full walk fills
/// it, a file system watcher keeps it current, and a cache on disk covers the next start.
#[derive(Default)]
pub struct FileIndex {
    contents: RwLock<Contents>,
    /// Serializes everything that replaces or patches `contents`.
    writer: Mutex<()>,
    cache_path: Mutex<Option<PathBuf>>,
    scanned_at: Mutex<Option<Instant>>,
    scanning: AtomicBool,
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
}

pub fn index() -> &'static FileIndex {
    use std::sync::OnceLock;
    static INDEX: OnceLock<FileIndex> = OnceLock::new();
    INDEX.get_or_init(FileIndex::default)
}

impl FileIndex {
    /// An index holding `paths` (relative to the first root), for tests.
    #[cfg(test)]
    pub fn with_paths(roots: Vec<PathBuf>, paths: &[(&str, bool)]) -> Self {
        let index = Self::default();
        if let Ok(mut contents) = index.contents.write() {
            contents.config = Config {
                roots,
                excludes: Vec::new(),
            };
            contents.entries = paths
                .iter()
                .map(|(rel, dir)| Entry::new(0, rel, *dir))
                .collect();
        }
        index
    }

    pub fn search(&self, query: &str, opens: &[LaunchEntry], now: i64, limit: usize) -> Vec<Hit> {
        self.contents
            .read()
            .map(|contents| contents.search(query, opens, now, limit))
            .unwrap_or_default()
    }

    pub fn contains(&self, path: &str) -> bool {
        self.contents
            .read()
            .is_ok_and(|contents| contents.contains(Path::new(path)))
    }

    pub fn status(&self) -> Status {
        let (roots, entries) = self
            .contents
            .read()
            .map(|contents| {
                (
                    contents
                        .config
                        .roots
                        .iter()
                        .map(|root| root.to_string_lossy().into_owned())
                        .collect(),
                    contents.entries.len(),
                )
            })
            .unwrap_or_default();
        Status {
            roots,
            entries,
            scanning: self.scanning.load(Ordering::SeqCst),
        }
    }

    /// Points the index at `config`: loads the cache when it matches, watches the roots and
    /// starts a full walk. Returns at once; the work runs on a worker thread.
    pub fn configure(&'static self, config: Config, cache_path: Option<PathBuf>) {
        if let Ok(mut path) = self.cache_path.lock() {
            *path = cache_path.clone();
        }
        std::thread::spawn(move || {
            {
                let _writing = self.writer.lock();
                let cached = cache_path
                    .as_deref()
                    .and_then(|path| load_cache(path, &config));
                if let Ok(mut contents) = self.contents.write() {
                    let unchanged = contents.config.roots == config.roots;
                    contents.config = config.clone();
                    if let Some(entries) = cached {
                        contents.entries = entries;
                    } else if !unchanged {
                        contents.entries.clear();
                    }
                }
            }
            self.watch(&config);
            self.rescan();
        });
    }

    /// Rescans in the background if the last walk is old, or never finished.
    pub fn refresh_if_stale(&'static self) {
        let watching = self.watcher.lock().is_ok_and(|watcher| watcher.is_some());
        let limit = if watching {
            STALE_AFTER
        } else {
            UNWATCHED_STALE_AFTER
        };
        let fresh = self
            .scanned_at
            .lock()
            .ok()
            .and_then(|at| *at)
            .is_some_and(|at| at.elapsed() < limit);
        let configured = self.cache_path.lock().is_ok_and(|path| path.is_some());
        if configured && !fresh {
            self.rescan();
        }
    }

    /// Starts a full walk unless one is running.
    pub fn rescan(&'static self) {
        if self.scanning.swap(true, Ordering::SeqCst) {
            return;
        }
        std::thread::spawn(move || {
            let config = self
                .contents
                .read()
                .map(|contents| contents.config.clone())
                .unwrap_or_default();
            let started = Instant::now();
            let entries = walk(&config);
            log::info!(
                "indexed {} files and folders in {:?}",
                entries.len(),
                started.elapsed()
            );
            {
                let _writing = self.writer.lock();
                if let Ok(mut contents) = self.contents.write() {
                    // Settings may have changed during the walk; that change starts its own.
                    if contents.config == config {
                        contents.entries = entries;
                    }
                }
            }
            if let Ok(mut at) = self.scanned_at.lock() {
                *at = Some(Instant::now());
            }
            self.scanning.store(false, Ordering::SeqCst);
            self.save_cache();
        });
    }

    fn save_cache(&self) {
        let Some(path) = self.cache_path.lock().ok().and_then(|path| path.clone()) else {
            return;
        };
        let Ok(contents) = self.contents.read() else {
            return;
        };
        if let Err(err) = save_cache(&path, &contents) {
            log::warn!("couldn't save the file index: {err}");
        }
    }

    fn watch(&'static self, config: &Config) {
        let (sender, receiver) = mpsc::channel();
        let watcher = notify::recommended_watcher(move |event| {
            let _ = sender.send(event);
        });
        let mut watcher = match watcher {
            Ok(watcher) => watcher,
            Err(err) => {
                log::warn!("file changes won't be tracked: {err}");
                return;
            }
        };
        for root in &config.roots {
            if let Err(err) = watcher.watch(root, RecursiveMode::Recursive) {
                log::warn!("couldn't watch {}: {err}", root.display());
            }
        }
        // Replacing the old watcher closes its channel, which ends its thread.
        if let Ok(mut slot) = self.watcher.lock() {
            *slot = Some(watcher);
        }
        let config = config.clone();
        std::thread::spawn(move || self.follow(config, receiver));
    }

    /// Collects change events until the file system settles, then patches the index.
    fn follow(
        &'static self,
        config: Config,
        receiver: mpsc::Receiver<notify::Result<notify::Event>>,
    ) {
        let rules = Rules::new(&config);
        let lookup = Contents {
            config,
            entries: Vec::new(),
        };
        // Changes under skipped folders (AppData, Library, caches) churn constantly; drop them
        // on arrival so they neither delay nor overflow a batch.
        let relevant = |path: &Path| {
            lookup.locate(path).is_some_and(|(root, rel)| {
                !rel.is_empty() && rules.may_allow(&lookup.config.roots[root as usize], rel)
            })
        };
        let mut pending: HashSet<PathBuf> = HashSet::new();
        let mut overflowed = false;
        let mut batch_started: Option<Instant> = None;
        let mut last_forced: Option<Instant> = None;
        loop {
            match receiver.recv_timeout(SETTLE) {
                // Reads (including the walk's own) change nothing.
                Ok(Ok(event)) if matches!(event.kind, notify::EventKind::Access(_)) => {}
                Ok(Ok(event)) => {
                    overflowed |= event.need_rescan();
                    if !overflowed {
                        pending.extend(event.paths.into_iter().filter(|path| relevant(path)));
                        overflowed = pending.len() > RESCAN_ABOVE * 4;
                    }
                }
                Ok(Err(err)) => {
                    log::debug!("file watcher error: {err}");
                    overflowed = true;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if pending.is_empty() && !overflowed {
                batch_started = None;
                continue;
            }
            let started = *batch_started.get_or_insert_with(Instant::now);
            let settled = started.elapsed() >= SETTLE;
            // A running walk will see these changes or not; apply them once it lands.
            if !settled || self.scanning.load(Ordering::SeqCst) {
                continue;
            }
            if overflowed || !self.apply(&pending) {
                // Bursts too big to patch rescan, but not over and over under constant churn.
                if last_forced.is_none_or(|at| at.elapsed() >= FORCED_RESCAN_GAP) {
                    last_forced = Some(Instant::now());
                    self.rescan();
                }
            }
            pending.clear();
            overflowed = false;
            batch_started = None;
        }
    }

    /// Reconciles the index with the disk for each changed path. Returns false when the
    /// change is too broad to patch and needs a full walk.
    fn apply(&self, changed: &HashSet<PathBuf>) -> bool {
        let _writing = self.writer.lock();
        let (config, located) = {
            let Ok(contents) = self.contents.read() else {
                return true;
            };
            let rules = Rules::new(&contents.config);
            let located: Vec<(u16, String)> = changed
                .iter()
                .filter_map(|path| {
                    let (root, rel) = contents.locate(path)?;
                    let base = &contents.config.roots[root as usize];
                    (rel.is_empty() || rules.may_allow(base, rel)).then(|| (root, rel.to_string()))
                })
                .collect();
            (contents.config.clone(), located)
        };
        if located.is_empty() {
            return true;
        }
        if located.len() > RESCAN_ABOVE || located.iter().any(|(_, rel)| rel.is_empty()) {
            return false;
        }
        let rules = Rules::new(&config);

        // What the index holds for each changed path now.
        let wanted: HashSet<(u16, &str)> = located
            .iter()
            .map(|(root, rel)| (*root, rel.as_str()))
            .collect();
        let mut indexed: HashMap<(u16, String), bool> = HashMap::new();
        let mut indexed_dirs: HashSet<(u16, String)> = HashSet::new();
        if let Ok(contents) = self.contents.read() {
            for entry in &contents.entries {
                if wanted.contains(&(entry.root, &*entry.rel)) {
                    indexed.insert((entry.root, entry.rel.to_string()), entry.dir);
                }
            }
            let parents: HashSet<(u16, &str)> = located
                .iter()
                .filter_map(|(root, rel)| parent_rel(rel).map(|parent| (*root, parent)))
                .collect();
            for entry in &contents.entries {
                if entry.dir && parents.contains(&(entry.root, &*entry.rel)) {
                    indexed_dirs.insert((entry.root, entry.rel.to_string()));
                }
            }
        }

        let mut removals: Vec<(u16, String)> = Vec::new();
        let mut additions: HashMap<(u16, String), Vec<String>> = HashMap::new();
        for (root, rel) in &located {
            let full = config.roots[*root as usize].join(rel);
            let on_disk = fs::symlink_metadata(&full).ok().map(|meta| meta.is_dir());
            let held = indexed.get(&(*root, rel.clone())).copied();
            if on_disk == held {
                continue;
            }
            if held.is_some() {
                removals.push((*root, rel.clone()));
            }
            if on_disk.is_some() {
                let parent = parent_rel(rel).unwrap_or("").to_string();
                if parent.is_empty() || indexed_dirs.contains(&(*root, parent.clone())) {
                    additions
                        .entry((*root, parent))
                        .or_default()
                        .push(rel.clone());
                }
            }
        }

        // Each new path must survive the same rules a full walk applies, .gitignore included,
        // so list its parent one level deep and keep what shows up.
        let budget = AtomicUsize::new(0);
        let mut added: Vec<Entry> = Vec::new();
        for ((root, parent), children) in &additions {
            let base = config.roots[*root as usize].join(parent);
            let visible = walk_under(&rules, &config.roots, *root, &base, Some(1), &budget);
            for entry in visible {
                if !children.iter().any(|child| **child == *entry.rel) {
                    continue;
                }
                if entry.dir {
                    let inside = config.roots[*root as usize].join(&*entry.rel);
                    added.extend(walk_under(
                        &rules,
                        &config.roots,
                        *root,
                        &inside,
                        None,
                        &budget,
                    ));
                }
                added.push(entry);
            }
        }

        if let Ok(mut contents) = self.contents.write() {
            if contents.config != config {
                return true;
            }
            if !removals.is_empty() {
                contents
                    .entries
                    .retain(|entry| !removals.iter().any(|(root, rel)| entry.within(*root, rel)));
            }
            contents.entries.extend(added);
        }
        true
    }
}

fn parent_rel(rel: &str) -> Option<&str> {
    rel.rfind(is_separator).map(|at| &rel[..at])
}

fn save_cache(path: &Path, contents: &Contents) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    let mut out = std::io::BufWriter::new(fs::File::create(&tmp)?);
    write!(out, "{CACHE_HEADER}\0{}\0", contents.config.roots.len())?;
    for root in &contents.config.roots {
        write!(out, "{}\0", root.to_string_lossy())?;
    }
    for entry in &contents.entries {
        let kind = if entry.dir { 'd' } else { 'f' };
        write!(out, "{kind}{}|{}\0", entry.root, entry.rel)?;
    }
    out.into_inner()
        .map_err(|err| err.into_error())?
        .sync_all()?;
    if fs::rename(&tmp, path).is_err() {
        let _ = fs::remove_file(path);
        fs::rename(&tmp, path)?;
    }
    Ok(())
}

/// The cached entries, if the cache was written for the same roots.
fn load_cache(path: &Path, config: &Config) -> Option<Vec<Entry>> {
    let data = fs::read_to_string(path).ok()?;
    let mut fields = data.split('\0');
    if fields.next()? != CACHE_HEADER {
        return None;
    }
    let count: usize = fields.next()?.parse().ok()?;
    let roots: Vec<PathBuf> = fields.by_ref().take(count).map(PathBuf::from).collect();
    if roots != config.roots {
        return None;
    }
    let rules = Rules::new(config);
    let mut entries = Vec::new();
    for field in fields {
        if field.is_empty() {
            continue;
        }
        let dir = field.starts_with('d');
        let (root, rel) = field.get(1..)?.split_once('|')?;
        let root: u16 = root.parse().ok()?;
        let base = config.roots.get(root as usize)?;
        // Exclusions may have changed since the cache was written.
        if !rules
            .excludes
            .iter()
            .any(|exclude| base.join(rel).starts_with(exclude))
        {
            entries.push(Entry::new(root, rel, dir));
        }
    }
    Some(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sep(rel: &str) -> String {
        rel.replace('/', std::path::MAIN_SEPARATOR_STR)
    }

    fn root() -> PathBuf {
        std::env::temp_dir().join("zephyr-root")
    }

    fn index(paths: &[(&str, bool)]) -> FileIndex {
        let owned: Vec<(String, bool)> = paths.iter().map(|(rel, dir)| (sep(rel), *dir)).collect();
        let borrowed: Vec<(&str, bool)> = owned
            .iter()
            .map(|(rel, dir)| (rel.as_str(), *dir))
            .collect();
        FileIndex::with_paths(vec![root()], &borrowed)
    }

    fn names(hits: Vec<Hit>) -> Vec<String> {
        hits.into_iter().map(|hit| hit.name).collect()
    }

    #[test]
    fn scope_words_are_recognized() {
        assert!(is_scope("f"));
        assert!(is_scope("file"));
        assert!(is_scope("files"));
        assert!(!is_scope("fi"));
    }

    #[test]
    fn name_strength_tiers() {
        assert_eq!(
            name_strength("budget.xlsx", "budget"),
            Some(Strength::Exact)
        );
        assert_eq!(
            name_strength("budget.xlsx", "budget.xlsx"),
            Some(Strength::Exact)
        );
        assert_eq!(
            name_strength("budget 2026.xlsx", "budget"),
            Some(Strength::Prefix)
        );
        assert_eq!(
            name_strength("2026 budget.xlsx", "bud"),
            Some(Strength::WordPrefix)
        );
        assert_eq!(
            name_strength("2026_budget.xlsx", "bud"),
            Some(Strength::WordPrefix)
        );
        assert_eq!(
            name_strength("rebudget.xlsx", "bud"),
            Some(Strength::Substring)
        );
        assert_eq!(name_strength("notes.md", "bud"), None);
        assert_eq!(name_strength(".bashrc", ".bash"), Some(Strength::Prefix));
    }

    #[test]
    fn better_name_matches_rank_first_then_shallower_paths() {
        let index = index(&[
            ("Archive/old/budget.xlsx", false),
            ("Documents/budget.xlsx", false),
            ("Documents/rebudgeting.txt", false),
            ("Documents/2026 budget plan.pdf", false),
            ("Documents/budget", true),
        ]);
        assert_eq!(
            names(index.search("budget", &[], 0, 8)),
            [
                "budget",
                "budget.xlsx",
                "budget.xlsx",
                "2026 budget plan.pdf",
                "rebudgeting.txt"
            ]
        );
        let top = index.search("budget", &[], 0, 2);
        assert_eq!(top.len(), 2);
        assert!(top[1].path.ends_with(&sep("Documents/budget.xlsx")));
    }

    #[test]
    fn words_can_match_folders_but_one_must_name_the_file() {
        let index = index(&[
            ("Taxes/2025/return.pdf", false),
            ("Taxes/2025/receipts", true),
            ("Taxes/notes.txt", false),
        ]);
        assert_eq!(
            names(index.search("taxes return", &[], 0, 8)),
            ["return.pdf"]
        );
        // A folder name alone isn't enough: "taxes" doesn't list everything inside Taxes.
        assert!(index.search("taxes", &[], 0, 8).is_empty());
        assert_eq!(names(index.search("2025 rec", &[], 0, 8)), ["receipts"]);
    }

    #[test]
    fn opened_files_win_ties() {
        let index = index(&[("a/report.docx", false), ("b/report.docx", false)]);
        let opened = root().join(sep("b/report.docx"));
        let opens = vec![LaunchEntry {
            app_id: opened.to_string_lossy().into_owned(),
            uses: 3,
            last_used: 0,
        }];
        let hits = index.search("report", &opens, 0, 2);
        assert_eq!(hits[0].path, opened.to_string_lossy());
    }

    #[test]
    fn contains_only_knows_indexed_paths() {
        let index = index(&[("Documents/budget.xlsx", false)]);
        let inside = root().join(sep("Documents/budget.xlsx"));
        assert!(index.contains(&inside.to_string_lossy()));
        assert!(!index.contains(&root().join("elsewhere.txt").to_string_lossy()));
        assert!(!index.contains(&root().to_string_lossy()));
    }

    #[test]
    fn within_matches_the_path_and_its_contents_only() {
        let entry = Entry::new(0, &sep("a/b/c.txt"), false);
        assert!(entry.within(0, &sep("a/b")));
        assert!(entry.within(0, &sep("a/b/c.txt")));
        assert!(!entry.within(0, &sep("a/b/c")));
        assert!(!entry.within(0, &sep("a/bb")));
        assert!(!entry.within(1, &sep("a/b")));
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zephyr-files-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn rels(entries: &[Entry]) -> Vec<String> {
        let mut found: Vec<String> = entries
            .iter()
            .map(|entry| entry.rel.replace('\\', "/"))
            .collect();
        found.sort();
        found
    }

    #[test]
    fn the_walk_skips_hidden_ignored_and_cache_folders() {
        let dir = scratch("walk");
        for path in [
            "Documents/plan.md",
            "code/app/src/main.rs",
            "code/app/dist/bundle.js",
            "code/app/node_modules/pkg/index.js",
            "code/app/target/debug/app",
            ".config/secret.toml",
            "Excluded/private.txt",
        ] {
            let full = dir.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, b"").unwrap();
        }
        fs::create_dir_all(dir.join("code/app/.git")).unwrap();
        fs::write(dir.join("code/app/.gitignore"), "dist/\n").unwrap();
        fs::write(dir.join("code/app/target/CACHEDIR.TAG"), "Signature").unwrap();

        let config = Config {
            roots: vec![dir.clone()],
            excludes: vec![dir.join("Excluded")],
        };
        let found = rels(&walk(&config));
        assert_eq!(
            found,
            [
                "Documents",
                "Documents/plan.md",
                "code",
                "code/app",
                "code/app/src",
                "code/app/src/main.rs",
            ]
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn changes_are_patched_in_by_the_same_rules() {
        let dir = scratch("apply");
        fs::create_dir_all(dir.join("Documents")).unwrap();
        fs::write(dir.join("Documents/old.txt"), b"").unwrap();
        let config = Config {
            roots: vec![dir.clone()],
            excludes: Vec::new(),
        };
        let index = FileIndex::default();
        index.contents.write().unwrap().config = config.clone();
        index.contents.write().unwrap().entries = walk(&config);

        fs::remove_file(dir.join("Documents/old.txt")).unwrap();
        fs::write(dir.join("Documents/new.txt"), b"").unwrap();
        fs::create_dir_all(dir.join("Projects/site/node_modules/x")).unwrap();
        fs::write(dir.join("Projects/site/index.html"), b"").unwrap();
        fs::write(dir.join("Projects/site/node_modules/x/a.js"), b"").unwrap();
        fs::write(dir.join("Documents/.hidden"), b"").unwrap();

        let changed: HashSet<PathBuf> = [
            "Documents/old.txt",
            "Documents/new.txt",
            "Projects",
            "Projects/site/node_modules/x/a.js",
            "Documents/.hidden",
        ]
        .iter()
        .map(|rel| dir.join(sep(rel)))
        .collect();
        assert!(index.apply(&changed));
        let found = rels(&index.contents.read().unwrap().entries);
        assert_eq!(
            found,
            [
                "Documents",
                "Documents/new.txt",
                "Projects",
                "Projects/site",
                "Projects/site/index.html",
            ]
        );

        // A change to a root itself is too broad to patch.
        let root_changed: HashSet<PathBuf> = [dir.clone()].into_iter().collect();
        assert!(!index.apply(&root_changed));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_cache_round_trips_for_the_same_roots_only() {
        let dir = scratch("cache");
        let config = Config {
            roots: vec![dir.join("a"), dir.join("b")],
            excludes: Vec::new(),
        };
        let contents = Contents {
            config: config.clone(),
            entries: vec![
                Entry::new(0, &sep("Docs/plan.md"), false),
                Entry::new(1, "Photos", true),
            ],
        };
        let path = dir.join("files.idx");
        save_cache(&path, &contents).unwrap();
        let loaded = load_cache(&path, &config).unwrap();
        assert_eq!(rels(&loaded), ["Docs/plan.md", "Photos"]);
        assert!(loaded[1].dir && loaded[1].root == 1);

        let other = Config {
            roots: vec![dir.join("a")],
            excludes: Vec::new(),
        };
        assert!(load_cache(&path, &other).is_none());
        let excluding = Config {
            roots: config.roots.clone(),
            excludes: vec![dir.join("a").join("Docs")],
        };
        assert_eq!(rels(&load_cache(&path, &excluding).unwrap()), ["Photos"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unscoped_rows_come_only_from_opened_files_with_clear_names() {
        let dir = scratch("incidental");
        let budget = dir.join("budget 2026.xlsx");
        fs::write(&budget, b"").unwrap();
        let gone = dir.join("budget old.xlsx");
        let opens: Vec<LaunchEntry> = [&budget, &gone]
            .iter()
            .map(|path| LaunchEntry {
                app_id: path.to_string_lossy().into_owned(),
                uses: 1,
                last_used: 0,
            })
            .collect();
        assert_eq!(
            names(incidental(&opens, "budget", 0, 2)),
            ["budget 2026.xlsx"]
        );
        assert!(incidental(&opens, "bu", 0, 2).is_empty());
        assert!(incidental(&opens, "udget", 0, 2).is_empty());
        assert_eq!(names(recent(&opens, 0, 8)), ["budget 2026.xlsx"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn typed_folders_are_normalized() {
        let dir = scratch("normalize");
        let typed = format!("  {}{}  ", dir.display(), std::path::MAIN_SEPARATOR);
        let normalized = normalize_folders(&[typed.clone(), typed, String::new()]).unwrap();
        assert_eq!(normalized, [dir.to_string_lossy().into_owned()]);
        assert!(normalize_folders(&["relative/path".into()]).is_err());
        assert!(normalize_folders(&[dir.join("missing").to_string_lossy().into_owned()]).is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
