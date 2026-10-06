use std::collections::HashSet;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::apps::{self, App, Catalog};
use crate::destination::{self, Destination, SuggestKind};
use crate::history::{self, HistoryEntry};
use crate::query::{self, Parsed};
use crate::settings::{self, Setting, Target};

#[derive(Debug, Clone, Serialize)]
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestResponse {
    pub mode: String,
    pub items: Vec<Suggestion>,
    pub notice: Option<String>,
    /// Row Enter acts on before the user moves the selection.
    pub preselect: Option<usize>,
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
    if parsed.only_bang && !scoped && !settings_scoped {
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
        };
    }
    if let Some(trigger) = &parsed.bang
        && destination::exact_trigger(destinations, trigger).is_none()
    {
        return SuggestResponse {
            mode: "search".into(),
            items: history_items(history, &parsed.query, destinations, now, 8),
            notice: Some(format!("No destination !{trigger}")),
            preselect: None,
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

    // Settings never take Enter unscoped: at most two rows, below everything else local.
    let matched_settings: Vec<Suggestion> = if offer_apps {
        settings::incidental(catalog.settings, &parsed.query, 2)
            .into_iter()
            .map(setting_item)
            .collect()
    } else {
        Vec::new()
    };

    let mut items = Vec::new();
    if let Some(app) = preferred {
        items.push(app_item(app));
    }
    items.extend(history_items(history, &parsed.query, destinations, now, 4));
    items.extend(other_apps);
    items.truncate(8 - matched_settings.len());
    items.extend(matched_settings);

    let destination = resolve_destination(&parsed, armed_id, destinations);
    if include_remote
        && let Some(destination) = destination
        && !parsed.query.is_empty()
    {
        let remote = fetch_remote(destination.suggest, &parsed.query).await;
        merge_remote(&mut items, remote, destination);
    }
    items.truncate(8);
    SuggestResponse {
        mode: "search".into(),
        items,
        notice: None,
        preselect: preferred.map(|_| 0),
    }
}

fn app_item(app: &App) -> Suggestion {
    Suggestion {
        label: app.name.clone(),
        query: String::new(),
        destination_id: String::new(),
        kind: "app".into(),
        hint: "App".into(),
        app_id: Some(app.id.clone()),
        setting_id: None,
    }
}

fn setting_item(setting: &Setting) -> Suggestion {
    Suggestion {
        label: setting.title.to_string(),
        query: String::new(),
        destination_id: String::new(),
        kind: "setting".into(),
        hint: setting.hint().into(),
        app_id: None,
        setting_id: Some(setting.id.to_string()),
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
            query: String::new(),
            destination_id: destination.id.clone(),
            kind: "destination".into(),
            app_id: None,
            setting_id: None,
            hint: destination
                .triggers
                .iter()
                .map(|trigger| format!("!{trigger}"))
                .collect::<Vec<_>>()
                .join(" "),
        })
        .collect();
    SuggestResponse {
        mode: "destinations".into(),
        items,
        notice,
        preselect: None,
    }
}

fn recent(history: &[HistoryEntry], destinations: &[Destination], now: i64) -> SuggestResponse {
    SuggestResponse {
        mode: "recent".into(),
        items: history_items(history, "", destinations, now, 8),
        notice: None,
        preselect: None,
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
            destination_id: entry.destination_id.clone(),
            kind: "history".into(),
            hint: destination_name(destinations, &entry.destination_id),
            app_id: None,
            setting_id: None,
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
            hint: destination.name.clone(),
            app_id: None,
            setting_id: None,
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

async fn fetch_remote(kind: SuggestKind, query: &str) -> Vec<String> {
    let Some(url) = suggest_url(kind, query) else {
        return Vec::new();
    };
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
    match kind {
        SuggestKind::Google | SuggestKind::Youtube => parse_google(&body, query),
        SuggestKind::Wikipedia => parse_wikipedia(&body, query),
        SuggestKind::Pubmed => parse_pubmed(&body, query),
        SuggestKind::None => Vec::new(),
    }
}

fn suggest_url(kind: SuggestKind, query: &str) -> Option<String> {
    let encoded = urlencoding::encode(query);
    let url = match kind {
        SuggestKind::Google => {
            format!("https://suggestqueries.google.com/complete/search?client=firefox&q={encoded}")
        }
        SuggestKind::Youtube => format!(
            "https://suggestqueries.google.com/complete/search?client=firefox&ds=yt&q={encoded}"
        ),
        SuggestKind::Wikipedia => format!(
            "https://en.wikipedia.org/w/api.php?action=opensearch&search={encoded}&limit=6&namespace=0&format=json"
        ),
        SuggestKind::Pubmed => {
            format!("https://pubmed.ncbi.nlm.nih.gov/suggestions/?term={encoded}")
        }
        SuggestKind::None => return None,
    };
    Some(url)
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

fn parse_google(body: &str, query: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    string_list(value.get(1), query)
}

fn parse_wikipedia(body: &str, query: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    string_list(value.get(1), query)
}

fn parse_pubmed(body: &str, query: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    string_list(value.get("suggestions"), query)
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
        let catalog = Catalog {
            apps,
            settings: SETTINGS,
            launches: &[],
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

    #[test]
    fn parses_live_suggestion_shapes() {
        let google = r#"["crispr",["crispr","crispr cas9"],[],{}]"#;
        assert_eq!(parse_google(google, "crispr"), vec!["crispr cas9"]);

        let wiki = r#"["crispr",["CRISPR","CRISPR gene editing"],[""],[""]]"#;
        assert_eq!(parse_wikipedia(wiki, "crispr"), vec!["CRISPR gene editing"]);

        let pubmed = r#"{"code":0,"suggestions":["crispr","crispr cas9"]}"#;
        assert_eq!(parse_pubmed(pubmed, "crispr"), vec!["crispr cas9"]);
    }
}
