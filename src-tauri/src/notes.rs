//! Quick notes: one rich text (HTML) file per note in Zephyr's settings folder, so they stay
//! readable outside Zephyr. The first line is the title. Notes saved as Markdown by earlier
//! builds are still read, and become HTML the next time they are saved.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

const SCOPE_TRIGGERS: &[&str] = &["note", "notes"];

pub fn is_scope(trigger: &str) -> bool {
    SCOPE_TRIGGERS.contains(&trigger)
}

static DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn init(dir: PathBuf) {
    if let Err(err) = fs::create_dir_all(&dir) {
        log::error!("couldn't create the notes folder: {err}");
    }
    let _ = DIR.set(dir);
}

fn dir() -> Result<&'static Path, String> {
    DIR.get()
        .map(PathBuf::as_path)
        .ok_or_else(|| "Notes aren't ready yet".to_string())
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NoteSummary {
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub updated: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub body: String,
    /// `html`, or `markdown` for a note from an earlier build.
    pub format: &'static str,
    pub updated: i64,
}

/// Ids are file stems Zephyr made; anything else could reach outside the notes folder.
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

fn paths_for(id: &str) -> Result<(PathBuf, PathBuf), String> {
    if !valid_id(id) {
        return Err("That isn't a note".into());
    }
    let dir = dir()?;
    Ok((dir.join(format!("{id}.html")), dir.join(format!("{id}.md"))))
}

fn modified(path: &Path) -> i64 {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_secs() as i64)
}

/// The words in a note, one block per line, for titles, snippets and search.
pub fn plain_text(body: &str, html: bool) -> String {
    if !html {
        return body
            .lines()
            .map(|line| line.trim().trim_start_matches('#').trim())
            .collect::<Vec<_>>()
            .join("\n");
    }
    let mut text = String::new();
    let mut tag = String::new();
    let mut in_tag = false;
    for ch in body.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let name = tag
                    .trim_start_matches('/')
                    .split(|c: char| c.is_whitespace() || c == '/')
                    .next()
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if matches!(
                    name.as_str(),
                    "p" | "br"
                        | "li"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "blockquote"
                        | "pre"
                        | "div"
                        | "hr"
                ) {
                    text.push('\n');
                }
            }
            _ if in_tag => tag.push(ch),
            _ => text.push(ch),
        }
    }
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

pub fn title_of(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("Untitled");
    let mut title: String = line.chars().take(80).collect();
    if line.chars().count() > 80 {
        title.push('…');
    }
    title
}

fn snippet_of(text: &str) -> String {
    let rest: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .skip(1)
        .take(3)
        .collect();
    rest.join(" ").chars().take(120).collect()
}

/// Notes newest first; `query` matches the words, every one of them.
pub fn list(query: &str) -> Vec<NoteSummary> {
    let Ok(dir) = dir() else {
        return Vec::new();
    };
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut notes: Vec<NoteSummary> = Vec::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let html = match path.extension().and_then(|ext| ext.to_str()) {
            Some("html") => true,
            // A Markdown note that already has an HTML copy is the stale one.
            Some("md") if !path.with_extension("html").exists() => false,
            _ => continue,
        };
        if !valid_id(id) {
            continue;
        }
        let Ok(body) = fs::read_to_string(&path) else {
            continue;
        };
        let text = plain_text(&body, html);
        let lower = text.to_lowercase();
        if !words.iter().all(|word| lower.contains(word.as_str())) {
            continue;
        }
        notes.push(NoteSummary {
            id: id.to_string(),
            title: title_of(&text),
            snippet: snippet_of(&text),
            updated: modified(&path),
        });
    }
    notes.sort_by(|a, b| b.updated.cmp(&a.updated).then(b.id.cmp(&a.id)));
    notes
}

pub fn get(id: &str) -> Result<Note, String> {
    let (html, markdown) = paths_for(id)?;
    let (path, format) = if html.exists() {
        (html, "html")
    } else {
        (markdown, "markdown")
    };
    let body = fs::read_to_string(&path).map_err(|_| "That note was deleted".to_string())?;
    Ok(Note {
        id: id.to_string(),
        updated: modified(&path),
        format,
        body,
    })
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Makes a note whose first line is `text` (plain text, may be empty) and returns its id.
pub fn create(text: &str) -> Result<String, String> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    let mut id = format!("note-{millis}");
    let mut suffix = 1;
    loop {
        let (html, markdown) = paths_for(&id)?;
        if !html.exists() && !markdown.exists() {
            break;
        }
        id = format!("note-{millis}-{suffix}");
        suffix += 1;
    }
    let body = if text.trim().is_empty() {
        String::new()
    } else {
        format!("<p>{}</p>", escape(text.trim()))
    };
    save(&id, &body)?;
    Ok(id)
}

/// Saves a note's HTML, retiring any Markdown copy from an earlier build.
pub fn save(id: &str, body: &str) -> Result<i64, String> {
    let (path, markdown) = paths_for(id)?;
    let tmp = path.with_extension("html.tmp");
    fs::write(&tmp, body).map_err(|err| format!("Couldn't save the note: {err}"))?;
    fs::rename(&tmp, &path).map_err(|err| format!("Couldn't save the note: {err}"))?;
    let _ = fs::remove_file(markdown);
    Ok(modified(&path))
}

pub fn delete(id: &str) -> Result<(), String> {
    let (html, markdown) = paths_for(id)?;
    for path in [html, markdown] {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(format!("Couldn't delete the note: {err}")),
        }
    }
    Ok(())
}

/// The folder notes live in, for "Show in Finder".
pub fn folder() -> Result<String, String> {
    Ok(dir()?.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_and_snippets_come_from_the_text() {
        let html =
            "<h1>Groceries</h1><ul><li><p>milk &amp; eggs</p></li><li><p>bread</p></li></ul>";
        let text = plain_text(html, true);
        assert_eq!(title_of(&text), "Groceries");
        assert_eq!(snippet_of(&text), "milk & eggs bread");
        assert_eq!(title_of(&plain_text("# Trip\npack", false)), "Trip");
        assert_eq!(title_of(""), "Untitled");
    }

    #[test]
    fn refuses_ids_that_could_escape_the_folder() {
        assert!(valid_id("note-1712345678901"));
        assert!(!valid_id("../state"));
        assert!(!valid_id("a/b"));
        assert!(!valid_id(""));
    }

    #[test]
    fn creates_lists_searches_converts_and_deletes() {
        let dir = std::env::temp_dir().join(format!("zephyr-notes-{}", getrandom::u64().unwrap()));
        init(dir.clone());
        // DIR is process-wide; another test may have set it first.
        let dir = DIR.get().unwrap().clone();
        let id = create("Trip <plans>").unwrap();
        assert_eq!(get(&id).unwrap().body, "<p>Trip &lt;plans&gt;</p>");
        assert!(list("plans").iter().any(|note| note.id == id));
        assert!(list("nothing-like-this").iter().all(|note| note.id != id));

        let legacy = "note-legacy-test";
        fs::write(dir.join(format!("{legacy}.md")), "# Old\nmarkdown body").unwrap();
        assert_eq!(get(legacy).unwrap().format, "markdown");
        save(legacy, "<h1>Old</h1><p>now html</p>").unwrap();
        assert_eq!(get(legacy).unwrap().format, "html");
        assert!(!dir.join(format!("{legacy}.md")).exists());

        delete(&id).unwrap();
        delete(legacy).unwrap();
        assert!(get(&id).is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
