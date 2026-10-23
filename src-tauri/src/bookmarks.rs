//! Browser bookmarks in the bar, read-only. Every profile of the Chromium-family browsers
//! installed (Chrome, Edge, Brave, Vivaldi, Arc, Chromium) keeps them as JSON: `Bookmarks`,
//! and `AccountBookmarks` for ones synced to the signed-in account. Files are re-read when
//! they change, and bookmarks are deduplicated by URL.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;
use serde_json::Value;

use crate::apps::{self, Strength};

/// Bang words that scope the bar to bookmarks: `!bm music`.
pub const SCOPE_TRIGGERS: &[&str] = &["bm", "bookmark", "bookmarks"];

pub fn is_scope(trigger: &str) -> bool {
    SCOPE_TRIGGERS.contains(&trigger)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
    /// "Bookmarks bar › Work".
    pub folder: String,
    pub source: String,
}

/// A browser profile bookmarks were found in, for Settings.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    /// `chrome/Default`.
    pub id: String,
    /// "Chrome · Zach".
    pub label: String,
    pub count: usize,
    pub enabled: bool,
}

struct Profile {
    id: String,
    label: String,
    files: Vec<PathBuf>,
}

#[derive(Default)]
struct Index {
    checked: Option<Instant>,
    stamps: Vec<(PathBuf, Option<SystemTime>)>,
    disabled: Vec<String>,
    bookmarks: Vec<Bookmark>,
    sources: Vec<Source>,
}

fn index() -> &'static Mutex<Index> {
    static INDEX: OnceLock<Mutex<Index>> = OnceLock::new();
    INDEX.get_or_init(Default::default)
}

/// Which profiles to leave out (Settings > Bookmarks), by source id.
pub fn configure(disabled: &[String]) {
    if let Ok(mut index) = index().lock() {
        // The files haven't changed, so forget their stamps to make the next refresh rebuild
        // the list without (or with) these profiles.
        if index.disabled != disabled {
            index.stamps.clear();
        }
        index.disabled = disabled.to_vec();
        index.checked = None;
    }
}

/// The browsers that keep Chromium-style profiles, and where.
fn browser_roots() -> Vec<(&'static str, &'static str, PathBuf)> {
    let mut roots = Vec::new();
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        for (id, name, path) in [
            ("chrome", "Chrome", "Google\\Chrome\\User Data"),
            (
                "chrome-beta",
                "Chrome Beta",
                "Google\\Chrome Beta\\User Data",
            ),
            ("edge", "Edge", "Microsoft\\Edge\\User Data"),
            ("brave", "Brave", "BraveSoftware\\Brave-Browser\\User Data"),
            ("vivaldi", "Vivaldi", "Vivaldi\\User Data"),
            ("chromium", "Chromium", "Chromium\\User Data"),
            (
                "arc",
                "Arc",
                "Packages\\TheBrowserCompany.Arc_ttt1ap7aakyb4\\LocalCache\\Local\\Arc\\User Data",
            ),
        ] {
            roots.push((id, name, local.join(path)));
        }
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::home_dir() {
        let support = home.join("Library/Application Support");
        for (id, name, path) in [
            ("chrome", "Chrome", "Google/Chrome"),
            ("chrome-beta", "Chrome Beta", "Google/Chrome Beta"),
            ("edge", "Edge", "Microsoft Edge"),
            ("brave", "Brave", "BraveSoftware/Brave-Browser"),
            ("vivaldi", "Vivaldi", "Vivaldi"),
            ("chromium", "Chromium", "Chromium"),
            ("arc", "Arc", "Arc/User Data"),
        ] {
            roots.push((id, name, support.join(path)));
        }
    }
    roots
}

/// Every profile with a bookmarks file, named the way the browser names it.
fn profiles() -> Vec<Profile> {
    let mut found = Vec::new();
    for (browser, browser_name, root) in browser_roots() {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        let names = profile_names(&root);
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        dirs.sort();
        for dir in dirs {
            let files: Vec<PathBuf> = ["Bookmarks", "AccountBookmarks"]
                .iter()
                .map(|name| dir.join(name))
                .filter(|path| path.is_file())
                .collect();
            if files.is_empty() {
                continue;
            }
            let folder = dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let person = names
                .iter()
                .find(|(key, _)| *key == folder)
                .map(|(_, name)| name.clone())
                .unwrap_or_else(|| folder.clone());
            found.push(Profile {
                id: format!("{browser}/{folder}"),
                label: format!("{browser_name} · {person}"),
                files,
            });
        }
    }
    found
}

/// Profile folder → display name, from the browser's `Local State`.
fn profile_names(root: &Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(root.join("Local State")) else {
        return Vec::new();
    };
    let Ok(state) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    state["profile"]["info_cache"]
        .as_object()
        .map(|cache| {
            cache
                .iter()
                .filter_map(|(folder, info)| {
                    Some((folder.clone(), info["name"].as_str()?.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The bookmarks in one Chromium `Bookmarks` file.
pub fn parse(text: &str, source: &str) -> Vec<Bookmark> {
    let Ok(document) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    if let Some(roots) = document["roots"].as_object() {
        for root in roots.values() {
            walk(root, None, source, &mut found);
        }
    }
    found
}

fn walk(node: &Value, folder: Option<&str>, source: &str, found: &mut Vec<Bookmark>) {
    let name = node["name"].as_str().unwrap_or_default();
    match node["type"].as_str() {
        Some("url") => {
            let url = node["url"].as_str().unwrap_or_default();
            if url.starts_with("http://") || url.starts_with("https://") {
                found.push(Bookmark {
                    title: if name.trim().is_empty() {
                        url.to_string()
                    } else {
                        name.trim().to_string()
                    },
                    url: url.to_string(),
                    folder: folder.unwrap_or_default().to_string(),
                    source: source.to_string(),
                });
            }
        }
        Some("folder") => {
            let path = match folder {
                Some(parent) if !name.is_empty() => format!("{parent} › {name}"),
                Some(parent) => parent.to_string(),
                None => name.to_string(),
            };
            if let Some(children) = node["children"].as_array() {
                for child in children {
                    walk(child, Some(&path), source, found);
                }
            }
        }
        _ => {}
    }
}

/// Re-reads the files when one has changed, at most every few seconds.
fn refresh(index: &mut Index) {
    if index
        .checked
        .is_some_and(|checked| checked.elapsed() < Duration::from_secs(3))
    {
        return;
    }
    index.checked = Some(Instant::now());
    // Tests never read the real browsers' files.
    if cfg!(test) {
        return;
    }
    let profiles = profiles();
    let stamps: Vec<(PathBuf, Option<SystemTime>)> = profiles
        .iter()
        .flat_map(|profile| profile.files.iter())
        .map(|path| {
            (
                path.clone(),
                std::fs::metadata(path)
                    .and_then(|meta| meta.modified())
                    .ok(),
            )
        })
        .collect();
    if stamps == index.stamps && !index.sources.is_empty() {
        for source in &mut index.sources {
            source.enabled = !index.disabled.contains(&source.id);
        }
        return;
    }
    index.stamps = stamps;
    let mut bookmarks = Vec::new();
    let mut sources = Vec::new();
    let mut seen = HashSet::new();
    for profile in profiles {
        let mut count = 0;
        let enabled = !index.disabled.contains(&profile.id);
        for file in &profile.files {
            let Ok(text) = std::fs::read_to_string(file) else {
                continue;
            };
            for bookmark in parse(&text, &profile.id) {
                count += 1;
                if enabled && seen.insert(bookmark.url.clone()) {
                    bookmarks.push(bookmark);
                }
            }
        }
        sources.push(Source {
            id: profile.id,
            label: profile.label,
            count,
            enabled,
        });
    }
    index.bookmarks = bookmarks;
    index.sources = sources;
}

pub fn sources() -> Vec<Source> {
    let Ok(mut index) = index().lock() else {
        return Vec::new();
    };
    index.checked = None;
    refresh(&mut index);
    index.sources.clone()
}

/// The bookmark titled exactly `query` (any case), which Enter may open like an app.
pub fn exact(query: &str) -> Option<Bookmark> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    let mut index = index().lock().ok()?;
    refresh(&mut index);
    index
        .bookmarks
        .iter()
        .find(|bookmark| bookmark.title.eq_ignore_ascii_case(query))
        .cloned()
}

/// Bookmarks matching `query`, best first: by title, then by folder or site.
pub fn search(query: &str, limit: usize) -> Vec<Bookmark> {
    let Ok(mut index) = index().lock() else {
        return Vec::new();
    };
    refresh(&mut index);
    rank(&index.bookmarks, query, limit)
}

pub fn rank(bookmarks: &[Bookmark], query: &str, limit: usize) -> Vec<Bookmark> {
    let query = query.trim();
    if query.is_empty() {
        return bookmarks.iter().take(limit).cloned().collect();
    }
    let lower = query.to_lowercase();
    let mut scored: Vec<(u8, &Bookmark)> = bookmarks
        .iter()
        .filter_map(|bookmark| {
            let title = apps::strength(&bookmark.title, query).map(|found| match found {
                Strength::Exact => 6,
                Strength::Prefix => 5,
                Strength::WordPrefix => 4,
                Strength::Initials => 3,
                Strength::Substring => 2,
                Strength::Subsequence => 0,
            });
            let elsewhere = (bookmark.folder.to_lowercase().contains(&lower)
                || host(&bookmark.url).contains(&lower))
            .then_some(1);
            title
                .filter(|score| *score > 0)
                .or(elsewhere)
                .map(|score| (score, bookmark))
        })
        .collect();
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, bookmark)| bookmark.clone())
        .collect()
}

pub fn host(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|url| {
            url.host_str()
                .map(|host| host.trim_start_matches("www.").to_string())
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"{
        "roots": {
            "bookmark_bar": { "type": "folder", "name": "Bookmarks bar", "children": [
                { "type": "url", "name": "Music", "url": "https://music.youtube.com/" },
                { "type": "folder", "name": "Work", "children": [
                    { "type": "url", "name": "Jira board", "url": "https://acme.atlassian.net/jira" },
                    { "type": "url", "name": "", "url": "https://github.com/z3k-md" }
                ] },
                { "type": "url", "name": "Local", "url": "chrome://settings" }
            ] },
            "other": { "type": "folder", "name": "Other bookmarks", "children": [] }
        }
    }"#;

    #[test]
    fn reads_titles_urls_and_folder_paths() {
        let found = parse(FILE, "chrome/Default");
        assert_eq!(found.len(), 3, "browser pages are skipped");
        assert_eq!(found[0].title, "Music");
        assert_eq!(found[0].folder, "Bookmarks bar");
        assert_eq!(found[1].folder, "Bookmarks bar › Work");
        // A bookmark without a name shows its address.
        assert_eq!(found[2].title, "https://github.com/z3k-md");
        assert!(parse("not json", "x").is_empty());
    }

    #[test]
    fn ranks_by_title_then_folder_or_site() {
        let found = parse(FILE, "chrome/Default");
        assert_eq!(rank(&found, "music", 5)[0].title, "Music");
        assert_eq!(rank(&found, "jira", 5)[0].title, "Jira board");
        // "work" only matches the folder.
        assert_eq!(rank(&found, "work", 5)[0].title, "Jira board");
        // The site.
        assert_eq!(rank(&found, "atlassian", 5).len(), 1);
        assert!(rank(&found, "spotify", 5).is_empty());
    }

    #[test]
    fn hosts_drop_www() {
        assert_eq!(host("https://www.youtube.com/watch?v=1"), "youtube.com");
        assert_eq!(host("not a url"), "");
    }
}

#[cfg(test)]
mod machine {
    use super::*;

    /// Run by hand (`cargo test bookmarks -- --ignored --nocapture`) to see what this computer has.
    #[test]
    #[ignore]
    fn lists_this_computers_bookmarks() {
        for profile in profiles() {
            let count: usize = profile
                .files
                .iter()
                .filter_map(|file| std::fs::read_to_string(file).ok())
                .map(|text| parse(&text, &profile.id).len())
                .sum();
            println!("{} ({}): {count}", profile.label, profile.id);
        }
    }
}
