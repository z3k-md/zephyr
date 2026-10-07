use std::collections::HashSet;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::answer;
use crate::apps::{self, App, Catalog};
use crate::destination::{self, Destination};
use crate::files::{self, Hit};
use crate::history::{self, HistoryEntry};
use crate::notes;
use crate::query::{self, Parsed};
use crate::settings::{self, Setting, Target};

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub label: String,
    pub query: String,
    pub destination_id: String,
    pub kind: String,
    pub hint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setting_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_id: Option<String>,
    /// The list section the row belongs to: apps, answer, recent, web, files, settings,
    /// notes or destinations.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub section: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub subtitle: String,
    /// `app:<id>`, `dest:<id>` or `glyph:<name>`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub icon: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestResponse {
    pub mode: String,
    pub items: Vec<Suggestion>,
    pub notice: Option<String>,
    /// Row Enter acts on before the user moves the selection.
    pub preselect: Option<usize>,
    /// Where free text goes for an unscoped query: the bang's destination, `url`, or the
    /// armed destination.
    pub target: Option<String>,
    /// The typed text without any bang.
    pub text: String,
    /// Destinations to offer under "Search with", best first.
    pub search_with: Vec<String>,
}

/// Local results (apps, history) come back without waiting on the network unless
/// `include_remote` is set; remote suggestions only ever append, so row indices stay put.
pub async fn gather(
    input: &str,
    armed_id: &str,
    destinations: &[Destination],
    history: &[HistoryEntry],
    catalog: Catalog<'_>,
    include_remote: bool,
) -> SuggestResponse {
    let now = catalog.now;
    let parsed = query::parse_input(input);
    let scoped = parsed.bang.as_deref().is_some_and(apps::is_scope);
    let settings_scoped = parsed.bang.as_deref().is_some_and(settings::is_scope);
    let files_scoped = parsed.bang.as_deref().is_some_and(files::is_scope);
    let notes_scoped = parsed.bang.as_deref().is_some_and(notes::is_scope);
    let typing = parsed.bang.as_deref() == Some("type");
    if parsed.only_bang && !scoped && !settings_scoped && !files_scoped && !notes_scoped && !typing
    {
        return palette(destinations, parsed.bang.as_deref().unwrap_or(""));
    }
    if input.trim().is_empty() {
        return recent(history, destinations, now);
    }
    if scoped {
        let items: Vec<Suggestion> = if parsed.query.is_empty() {
            catalog.recent(8).into_iter().map(app_item).collect()
        } else {
            catalog
                .ranked(&parsed.query, 8)
                .into_iter()
                .map(|(app, _)| app_item(app))
                .collect()
        };
        let notice = (items.is_empty() && !parsed.query.is_empty())
            .then(|| format!("No app matches {}", parsed.query));
        return SuggestResponse {
            mode: "search".into(),
            preselect: (!items.is_empty() && !parsed.query.is_empty()).then_some(0),
            items,
            notice,
            text: parsed.query.clone(),
            ..Default::default()
        };
    }
    if settings_scoped {
        let items: Vec<Suggestion> = if parsed.query.is_empty() {
            starters(catalog.settings, 8)
        } else {
            settings::ranked(catalog.settings, &parsed.query, 8)
        }
        .into_iter()
        .map(setting_item)
        .collect();
        let notice = (items.is_empty() && !parsed.query.is_empty())
            .then(|| format!("No setting matches {}", parsed.query));
        return SuggestResponse {
            mode: "search".into(),
            preselect: (!items.is_empty() && !parsed.query.is_empty()).then_some(0),
            items,
            notice,
            text: parsed.query.clone(),
            ..Default::default()
        };
    }
    if notes_scoped {
        return SuggestResponse {
            text: parsed.query.clone(),
            ..notes_response(&parsed.query)
        };
    }
    if parsed.bang.as_deref() == Some("type") {
        return SuggestResponse {
            mode: "search".into(),
            notice: Some("Press Enter to start a typing test".into()),
            ..Default::default()
        };
    }
    if files_scoped {
        let items: Vec<Suggestion> = if parsed.query.is_empty() {
            files::recent(catalog.file_opens, now, 8)
        } else {
            catalog.files(&parsed.query, 8)
        }
        .iter()
        .map(file_item)
        .collect();
        let notice = if items.is_empty() && !parsed.query.is_empty() {
            Some(format!("No file matches {}", parsed.query))
        } else if items.is_empty() {
            Some("Type a file or folder name".into())
        } else {
            None
        };
        return SuggestResponse {
            mode: "search".into(),
            preselect: (!items.is_empty() && !parsed.query.is_empty()).then_some(0),
            items,
            notice,
            text: parsed.query.clone(),
            ..Default::default()
        };
    }
    if let Some(trigger) = &parsed.bang
        && destination::exact_trigger(destinations, trigger).is_none()
    {
        return SuggestResponse {
            mode: "search".into(),
            items: history_items(history, &parsed.query, destinations, now, 8),
            notice: Some(format!("No destination !{trigger}")),
            text: parsed.query.clone(),
            ..Default::default()
        };
    }

    // A bang or a URL names where the text goes, so apps stay out of the way.
    let offer_apps = parsed.bang.is_none() && !query::looks_like_url(&parsed.query);
    let preferred = offer_apps
        .then(|| catalog.preferred(&parsed.query))
        .flatten();
    let other_apps: Vec<Suggestion> = if offer_apps {
        catalog
            .ranked(&parsed.query, 4)
            .into_iter()
            .filter(|(app, found)| {
                *found >= apps::Strength::Substring
                    && preferred.is_none_or(|chosen| chosen.id != app.id)
            })
            .take(2)
            .map(|(app, _)| app_item(app))
            .collect()
    } else {
        Vec::new()
    };

    // Files and settings never take Enter unscoped: at most two rows between them, below
    // everything else local. Files only come from ones opened before, so this never waits on
    // the index.
    let incidental: Vec<Suggestion> = if offer_apps {
        let opened = files::incidental(catalog.file_opens, &parsed.query, now, 2);
        let mut rows: Vec<Suggestion> = opened.iter().map(file_item).collect();
        rows.extend(
            settings::incidental(catalog.settings, &parsed.query, 2 - rows.len())
                .into_iter()
                .map(setting_item),
        );
        rows
    } else {
        Vec::new()
    };

    // A calculation or time question answers in the top row, unless an app already claims
    // Enter for this text.
    let answer = (offer_apps && preferred.is_none())
        .then(|| answer::inline(&parsed.query, chrono::Utc::now()))
        .flatten();

    let mut items = Vec::new();
    if let Some(app) = preferred {
        items.push(app_item(app));
    }
    if let Some(answer) = &answer {
        items.push(Suggestion {
            label: answer.value.clone(),
            query: answer.value.clone(),
            kind: "answer".into(),
            hint: "Copy".into(),
            section: "answer".into(),
            subtitle: parsed.query.clone(),
            icon: match answer.hint {
                "Conversion" => "glyph:conversion",
                "Time" => "glyph:clock",
                _ => "glyph:calculator",
            }
            .into(),
            ..Default::default()
        });
    }
    // An app match keeps the apps together at the top; otherwise recent searches lead.
    if preferred.is_some() {
        items.extend(other_apps);
        items.extend(history_items(history, &parsed.query, destinations, now, 4));
    } else {
        items.extend(history_items(history, &parsed.query, destinations, now, 4));
        items.extend(other_apps);
    }
    items.truncate(8 - incidental.len());
    items.extend(incidental);

    let is_url = parsed.bang.is_none() && query::looks_like_url(&parsed.query);
    let destination = resolve_destination(&parsed, armed_id, destinations);
    // Suggestions only help when free text is going to a search: not under an app match,
    // an inline answer, or a link.
    if include_remote
        && let Some(destination) = destination
        && !parsed.query.is_empty()
        && preferred.is_none()
        && answer.is_none()
        && !is_url
    {
        let remote = fetch_remote(destination, &parsed.query).await;
        merge_remote(&mut items, remote, destination);
    }
    items.truncate(8);
    let target = if parsed.bang.is_some() {
        destination.map(|destination| destination.id.clone())
    } else if is_url {
        Some("url".to_string())
    } else {
        destination::enabled(destinations, armed_id).map(|destination| destination.id.clone())
    };
    SuggestResponse {
        mode: "search".into(),
        items,
        notice: None,
        preselect: (preferred.is_some() || answer.is_some()).then_some(0),
        target,
        text: parsed.query.clone(),
        search_with: searchable(destinations),
    }
}

/// Enabled destinations that take the typed text, in settings order. Fixed links (no
/// `{query}`) would drop the text, so they aren't offered.
fn searchable(destinations: &[Destination]) -> Vec<String> {
    destinations
        .iter()
        .filter(|destination| !destination.disabled)
        .filter(|destination| {
            destination.is_ai() || crate::template::uses_input(&destination.url_template)
        })
        .map(|destination| destination.id.clone())
        .collect()
}

/// `!note` lists recent notes; with text it finds notes and offers to start one with it.
/// Enter opens the best match, or creates the note when nothing matches by title.
fn notes_response(query: &str) -> SuggestResponse {
    let found = notes::list(query);
    let lower = query.to_lowercase();
    let note_rows = found.iter().take(7).map(|note| Suggestion {
        label: note.title.clone(),
        kind: "note".into(),
        section: "notes".into(),
        subtitle: note.snippet.clone(),
        icon: "glyph:note".into(),
        note_id: Some(note.id.clone()),
        ..Default::default()
    });
    let create = (!query.is_empty()).then(|| Suggestion {
        label: format!("New note: {query}"),
        query: query.to_string(),
        kind: "noteNew".into(),
        hint: "Create".into(),
        section: "notes".into(),
        icon: "glyph:plus".into(),
        ..Default::default()
    });
    let title_match = found
        .iter()
        .any(|note| note.title.to_lowercase().contains(&lower));
    let mut items: Vec<Suggestion> = Vec::new();
    if title_match {
        items.extend(note_rows);
        items.extend(create);
    } else {
        items.extend(create);
        items.extend(note_rows);
    }
    let notice = (items.is_empty()).then(|| "No notes yet. Type to start one.".to_string());
    SuggestResponse {
        mode: "search".into(),
        preselect: (!items.is_empty() && !query.is_empty()).then_some(0),
        items,
        notice,
        ..Default::default()
    }
}

fn app_item(app: &App) -> Suggestion {
    Suggestion {
        label: app.name.clone(),
        kind: "app".into(),
        hint: "App".into(),
        section: "apps".into(),
        icon: format!("app:{}", app.id),
        app_id: Some(app.id.clone()),
        ..Default::default()
    }
}

fn setting_item(setting: &Setting) -> Suggestion {
    Suggestion {
        label: setting.title.to_string(),
        kind: "setting".into(),
        hint: setting.hint().into(),
        section: "settings".into(),
        icon: if matches!(setting.target, Target::Zephyr(_)) {
            "glyph:sliders"
        } else {
            "glyph:gear"
        }
        .into(),
        setting_id: Some(setting.id.to_string()),
        ..Default::default()
    }
}

fn file_item(hit: &Hit) -> Suggestion {
    Suggestion {
        label: hit.name.clone(),
        kind: "file".into(),
        hint: if hit.dir { "Folder" } else { "File" }.into(),
        section: "files".into(),
        subtitle: hit.location(),
        icon: if hit.dir {
            "glyph:folder"
        } else {
            "glyph:file"
        }
        .into(),
        path: Some(hit.path.clone()),
        ..Default::default()
    }
}

/// What an empty `!set` lists: Zephyr's own settings window, then the most-used OS pages.
fn starters(all: &[Setting], limit: usize) -> Vec<&Setting> {
    let own = all.iter().find(|setting| setting.id == "zephyr.settings");
    let system = all
        .iter()
        .filter(|setting| !matches!(setting.target, Target::Zephyr(_)));
    own.into_iter().chain(system).take(limit).collect()
}

fn palette(destinations: &[Destination], prefix: &str) -> SuggestResponse {
    let mut matches = destination::filter_destinations(destinations, prefix);
    let notice = (matches.len() > 12).then(|| "Keep typing to narrow destinations".to_string());
    matches.truncate(12);
    let items = matches
        .into_iter()
        .map(|destination| Suggestion {
            label: destination.name.clone(),
            destination_id: destination.id.clone(),
            kind: "destination".into(),
            section: "destinations".into(),
            icon: format!("dest:{}", destination.id),
            hint: destination
                .triggers
                .iter()
                .map(|trigger| format!("!{trigger}"))
                .collect::<Vec<_>>()
                .join(" "),
            ..Default::default()
        })
        .collect();
    SuggestResponse {
        mode: "destinations".into(),
        items,
        notice,
        ..Default::default()
    }
}

fn recent(history: &[HistoryEntry], destinations: &[Destination], now: i64) -> SuggestResponse {
    SuggestResponse {
        mode: "recent".into(),
        items: history_items(history, "", destinations, now, 8),
        ..Default::default()
    }
}

fn history_items(
    history: &[HistoryEntry],
    query: &str,
    destinations: &[Destination],
    now: i64,
    limit: usize,
) -> Vec<Suggestion> {
    history::matching(history, query, now, limit)
        .into_iter()
        .map(|entry| Suggestion {
            label: entry.query.clone(),
            query: entry.query,
            kind: "history".into(),
            hint: destination_name(destinations, &entry.destination_id),
            section: "recent".into(),
            icon: if entry.destination_id == "url" {
                "glyph:link".into()
            } else {
                format!("dest:{}", entry.destination_id)
            },
            destination_id: entry.destination_id,
            ..Default::default()
        })
        .collect()
}

fn merge_remote(items: &mut Vec<Suggestion>, remote: Vec<String>, destination: &Destination) {
    let mut seen: HashSet<String> = items.iter().map(|item| item.query.to_lowercase()).collect();
    for label in remote {
        if !seen.insert(label.to_lowercase()) {
            continue;
        }
        items.push(Suggestion {
            query: label.clone(),
            label,
            destination_id: destination.id.clone(),
            kind: "remote".into(),
            section: "web".into(),
            icon: "glyph:search".into(),
            ..Default::default()
        });
    }
}

fn resolve_destination<'a>(
    parsed: &Parsed,
    armed_id: &str,
    destinations: &'a [Destination],
) -> Option<&'a Destination> {
    if let Some(trigger) = &parsed.bang {
        return destination::exact_trigger(destinations, trigger);
    }
    destination::enabled(destinations, armed_id)
}

fn destination_name(destinations: &[Destination], id: &str) -> String {
    if id == "url" {
        return "Link".into();
    }
    destinations
        .iter()
        .find(|destination| destination.id == id)
        .map(|destination| destination.name.clone())
        .unwrap_or_else(|| id.to_string())
}

async fn fetch_remote(destination: &Destination, query: &str) -> Vec<String> {
    let Some((template, path)) = destination.suggest_source() else {
        return Vec::new();
    };
    let url = template.replace("{query}", &urlencoding::encode(query));
    let Some(client) = http_client() else {
        return Vec::new();
    };
    let response = match client.get(url).send().await {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => {
            log::debug!("suggestions returned {}", response.status());
            return Vec::new();
        }
        Err(err) => {
            log::debug!("suggestions failed: {err}");
            return Vec::new();
        }
    };
    let body = match response.text().await {
        Ok(body) => body,
        Err(err) => {
            log::debug!("suggestions body failed: {err}");
            return Vec::new();
        }
    };
    let Ok(value) = serde_json::from_str::<Value>(&body) else {
        return Vec::new();
    };
    string_list(Some(&Value::Array(select(&value, path))), query)
}

/// Follows a dotted JSON path. A number indexes an array, `*` fans out over one, and an
/// empty path is the whole document; whatever is reached is flattened into strings.
fn select(value: &Value, path: &str) -> Vec<Value> {
    let mut current = vec![value.clone()];
    for step in path.split('.').filter(|step| !step.is_empty()) {
        current = current
            .iter()
            .flat_map(|value| match (step, value) {
                ("*", Value::Array(items)) => items.clone(),
                ("*", Value::Object(map)) => map.values().cloned().collect(),
                (_, Value::Array(items)) => step
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| items.get(index).cloned())
                    .into_iter()
                    .collect(),
                (_, Value::Object(map)) => map.get(step).cloned().into_iter().collect(),
                _ => Vec::new(),
            })
            .collect();
    }
    current
        .into_iter()
        .flat_map(|value| match value {
            Value::Array(items) => items,
            other => vec![other],
        })
        .collect()
}

fn http_client() -> Option<&'static reqwest::Client> {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .user_agent("Zephyr")
                .timeout(Duration::from_secs(4))
                .build()
                .ok()
        })
        .as_ref()
}

fn string_list(value: Option<&Value>, query: &str) -> Vec<String> {
    let Some(list) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for entry in list {
        let Some(text) = entry.as_str() else {
            continue;
        };
        let text = text.trim();
        if text.is_empty() || text.eq_ignore_ascii_case(query) {
            continue;
        }
        if items
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(text))
        {
            continue;
        }
        items.push(text.to_string());
        if items.len() == 6 {
            break;
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::LaunchEntry;

    const SETTINGS: &[Setting] = &[
        Setting {
            id: "win.display",
            title: "Display",
            keywords: &["screen"],
            target: Target::Uri("ms-settings:display"),
        },
        Setting {
            id: "win.colors",
            title: "Colors",
            keywords: &["dark mode"],
            target: Target::Uri("ms-settings:colors"),
        },
        Setting {
            id: "win.nightlight",
            title: "Night Light",
            keywords: &["display warmth"],
            target: Target::Uri("ms-settings:nightlight"),
        },
        Setting {
            id: "win.textsize",
            title: "Text Size",
            keywords: &["display text"],
            target: Target::Uri("ms-settings:easeofaccess-display"),
        },
    ];

    fn local(input: &str, apps: &[App]) -> SuggestResponse {
        local_with(input, apps, &[])
    }

    fn local_with(input: &str, apps: &[App], file_opens: &[LaunchEntry]) -> SuggestResponse {
        static NO_FILES: std::sync::LazyLock<crate::files::FileIndex> =
            std::sync::LazyLock::new(crate::files::FileIndex::default);
        let catalog = Catalog {
            apps,
            settings: SETTINGS,
            files: &NO_FILES,
            launches: &[],
            file_opens,
            overrides: &[],
            now: 0,
        };
        tauri::async_runtime::block_on(gather(
            input,
            "google",
            &crate::destination::builtins(),
            &[],
            catalog,
            false,
        ))
    }

    fn local_full(
        input: &str,
        apps: &[App],
        history: &[HistoryEntry],
        armed: &str,
        include_remote: bool,
    ) -> SuggestResponse {
        static NO_FILES: std::sync::LazyLock<crate::files::FileIndex> =
            std::sync::LazyLock::new(crate::files::FileIndex::default);
        let catalog = Catalog {
            apps,
            settings: SETTINGS,
            files: &NO_FILES,
            launches: &[],
            file_opens: &[],
            overrides: &[],
            now: 1_000_000,
        };
        tauri::async_runtime::block_on(gather(
            input,
            armed,
            &crate::destination::builtins(),
            history,
            catalog,
            include_remote,
        ))
    }

    fn searched(query: &str, destination: &str) -> HistoryEntry {
        HistoryEntry {
            query: query.into(),
            destination_id: destination.into(),
            uses: 1,
            last_used: 1_000_000,
        }
    }

    #[test]
    fn says_where_free_text_goes() {
        assert_eq!(
            local_full("cats !w", &[], &[], "google", false)
                .target
                .as_deref(),
            Some("wikipedia")
        );
        assert_eq!(
            local_full("example.com", &[], &[], "google", false)
                .target
                .as_deref(),
            Some("url")
        );
        let plain = local_full("cats", &[], &[], "pubmed", false);
        assert_eq!(plain.target.as_deref(), Some("pubmed"));
        assert_eq!(plain.text, "cats");
        assert!(plain.search_with.contains(&"google".to_string()));
    }

    #[test]
    fn an_app_match_keeps_apps_above_recent_searches() {
        let apps = [
            App {
                id: "code".into(),
                name: "Visual Studio Code".into(),
            },
            App {
                id: "vscodium".into(),
                name: "VSCodium".into(),
            },
        ];
        let history = [searched("vs code shortcuts", "google")];
        let response = local_full("vs", &apps, &history, "google", false);
        let kinds: Vec<&str> = response
            .items
            .iter()
            .map(|item| item.kind.as_str())
            .collect();
        let first_history = kinds.iter().position(|kind| *kind == "history");
        let last_app = kinds.iter().rposition(|kind| *kind == "app");
        if let (Some(history), Some(app)) = (first_history, last_app) {
            assert!(app < history, "{kinds:?}");
        }
        assert_eq!(response.items[0].section, "apps");
        assert!(response.items[0].icon.starts_with("app:"));
    }

    #[test]
    fn links_never_fetch_suggestions() {
        let response = local_full("example.com", &[], &[], "google", true);
        assert!(response.items.iter().all(|item| item.kind != "remote"));
    }

    #[test]
    fn rows_carry_their_section() {
        let history = [searched("crispr review", "pubmed")];
        let response = local_full("crispr", &[], &history, "google", false);
        let row = response
            .items
            .iter()
            .find(|item| item.kind == "history")
            .unwrap();
        assert_eq!(
            (row.section.as_str(), row.icon.as_str()),
            ("recent", "dest:pubmed")
        );
        let answer = local_full("12 * 7", &[], &[], "google", false);
        assert_eq!(answer.items[0].section, "answer");
        assert_eq!(answer.items[0].subtitle, "12 * 7");
    }

    #[test]
    fn unscoped_settings_are_at_most_two_rows_below_and_never_preselected() {
        let apps = [App {
            id: "displayfusion".into(),
            name: "DisplayFusion".into(),
        }];
        let response = local("display", &apps);
        let kinds: Vec<_> = response
            .items
            .iter()
            .map(|item| item.kind.as_str())
            .collect();
        assert_eq!(kinds, ["app", "setting", "setting"]);
        assert_eq!(response.items[1].label, "Display");
        assert_eq!(response.preselect, Some(0));

        let response = local("dark mode", &[]);
        assert_eq!(response.items[0].setting_id.as_deref(), Some("win.colors"));
        assert_eq!(response.preselect, None);

        assert!(local("display !g", &[]).items.is_empty());
    }

    #[test]
    fn opened_files_share_the_two_unscoped_rows_and_the_empty_file_scope_lists_them() {
        let dir = std::env::temp_dir().join(format!("zephyr-suggest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Display calibration.pdf");
        std::fs::write(&file, b"").unwrap();
        let opens = vec![LaunchEntry {
            app_id: file.to_string_lossy().into_owned(),
            uses: 1,
            last_used: 0,
        }];

        let response = local_with("display", &[], &opens);
        assert_eq!(response.preselect, None);
        let kinds: Vec<&str> = response
            .items
            .iter()
            .map(|item| item.kind.as_str())
            .collect();
        assert_eq!(kinds, ["file", "setting"]);
        assert_eq!(response.items[0].path.as_deref(), file.to_str());

        let scoped = local_with("!f", &[], &opens);
        assert_eq!(scoped.preselect, None);
        assert_eq!(scoped.items[0].label, "Display calibration.pdf");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_settings_scope_preselects_its_best_match() {
        let response = local("!set dark", &[]);
        assert_eq!(response.items[0].label, "Colors");
        assert_eq!(response.preselect, Some(0));

        let empty = local("!set", &[]);
        assert_eq!(empty.items.len(), SETTINGS.len());
        assert_eq!(empty.preselect, None);

        let none = local("!set wallpaper", &[]);
        assert!(none.items.is_empty());
        assert_eq!(none.notice.as_deref(), Some("No setting matches wallpaper"));
    }

    fn pick(body: &str, path: &str, query: &str) -> Vec<String> {
        let value: Value = serde_json::from_str(body).unwrap();
        string_list(Some(&Value::Array(select(&value, path))), query)
    }

    #[test]
    fn parses_live_suggestion_shapes() {
        let google = r#"["crispr",["crispr","crispr cas9"],[],{}]"#;
        assert_eq!(pick(google, "1", "crispr"), vec!["crispr cas9"]);

        let wiki = r#"["crispr",["CRISPR","CRISPR gene editing"],[""],[""]]"#;
        assert_eq!(pick(wiki, "1", "crispr"), vec!["CRISPR gene editing"]);

        let pubmed = r#"{"code":0,"suggestions":["crispr","crispr cas9"]}"#;
        assert_eq!(pick(pubmed, "suggestions", "crispr"), vec!["crispr cas9"]);
    }

    #[test]
    fn follows_custom_json_paths() {
        let body = r#"{"items":[{"title":"rust book"},{"title":"rust by example"},{"id":3}]}"#;
        assert_eq!(
            pick(body, "items.*.title", "rust"),
            vec!["rust book", "rust by example"]
        );
        assert!(pick(body, "nope.*", "rust").is_empty());
    }
}
