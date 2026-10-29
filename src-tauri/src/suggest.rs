use std::collections::HashSet;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::answer;
use crate::apps::{self, App, Catalog};
use crate::bookmarks::{self, Bookmark};
use crate::files::{self, Hit};
use crate::history::{self, HistoryEntry};
use crate::notes;
use crate::query;
use crate::settings::{self, Setting, Target};

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub label: String,
    pub query: String,
    /// For a search row: `web` for a Google search of `query`, `url` to open it as a link.
    pub target: String,
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
    /// notes or bookmarks.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub section: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub subtitle: String,
    /// `app:<id>`, `site:<url>` or `glyph:<name>`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub icon: String,
    /// An open app's windows, front to back; its row switches to the first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<crate::running::WindowRef>,
    /// The window a `window` row switches to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<i64>,
    /// For a browser: what its private window is called (Incognito, InPrivate); Shift+Enter
    /// opens one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_window: Option<String>,
    /// For a port row: the socket and the process that has it open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_use: Option<crate::net::ports::PortUse>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestResponse {
    pub mode: String,
    pub items: Vec<Suggestion>,
    pub notice: Option<String>,
    /// Row Enter acts on before the user moves the selection.
    pub preselect: Option<usize>,
    /// Where free text goes for an unscoped query: `web` for a Google search, or `url`.
    pub target: Option<String>,
    /// The typed text without any bang.
    pub text: String,
    /// Whether the second, network pass would add anything (web suggestions, the public
    /// address), so the bar can skip asking for it.
    pub remote: bool,
}

/// Local results (apps, history) come back without waiting on the network unless
/// `include_remote` is set; remote suggestions only ever append, so row indices stay put.
pub async fn gather(
    input: &str,
    history: &[HistoryEntry],
    catalog: Catalog<'_>,
    include_remote: bool,
) -> SuggestResponse {
    if input.trim().is_empty() {
        return recent(history, &catalog);
    }
    let parsed = query::parse_input(input);
    if let Some(scope) = parsed.scope.as_deref()
        && let Some(response) = scoped(scope, &parsed.query, &catalog)
    {
        return response;
    }
    let text = parsed.query.as_str();
    // A URL names where the text goes, so apps stay out of the way.
    if query::looks_like_url(text) {
        return SuggestResponse {
            mode: "search".into(),
            items: history_items(history, text, catalog.now, 4),
            target: Some("url".into()),
            text: text.to_string(),
            ..Default::default()
        };
    }
    if let Some(port) = crate::net::ports::asked_port(text) {
        return port_answers_off_thread(text.to_string(), Some(port)).await;
    }
    if crate::net::ports::asks_for_list(text) {
        return port_answers_off_thread(text.to_string(), None).await;
    }
    if crate::net::ip::is_query(text) {
        return ip_answers(text, include_remote).await;
    }
    if settings::is_bare(text) {
        return bare_settings(text, &catalog);
    }
    let mut response = unscoped(text, history, &catalog);
    if include_remote && response.remote {
        let found = fetch_remote(text).await;
        merge_remote(&mut response.items, found);
        response.items.truncate(8);
    }
    response
}

/// A scoped query lists only its own kind of result; `None` for a scope not listed here.
fn scoped(scope: &str, text: &str, catalog: &Catalog<'_>) -> Option<SuggestResponse> {
    let response = if apps::is_scope(scope) {
        let items: Vec<Suggestion> = if text.is_empty() {
            catalog.recent(8).into_iter().map(app_item).collect()
        } else {
            catalog
                .ranked(text, 8)
                .into_iter()
                .map(|(app, _)| app_item(app))
                .collect()
        };
        let notice = empty_notice(&items, text, "app", None);
        scope_list(text, items, notice)
    } else if settings::is_scope(scope) {
        let items: Vec<Suggestion> = if text.is_empty() {
            starters(catalog.settings, 8)
        } else {
            settings::ranked(catalog.settings, text, 8)
        }
        .into_iter()
        .map(setting_item)
        .collect();
        let notice = empty_notice(&items, text, "setting", None);
        scope_list(text, items, notice)
    } else if bookmarks::is_scope(scope) {
        let items: Vec<Suggestion> = bookmarks::search(text, 8)
            .iter()
            .map(bookmark_item)
            .collect();
        let notice = empty_notice(&items, text, "bookmark", Some("No browser bookmarks found"));
        scope_list(text, items, notice)
    } else if notes::is_scope(scope) {
        SuggestResponse {
            text: text.to_string(),
            ..notes_response(text)
        }
    } else if scope == "type" {
        SuggestResponse {
            mode: "search".into(),
            notice: Some("Press Enter to start a typing test".into()),
            ..Default::default()
        }
    } else if files::is_scope(scope) {
        let items: Vec<Suggestion> = if text.is_empty() {
            files::recent(catalog.file_opens, catalog.now, 8)
        } else {
            catalog.files(text, 8)
        }
        .iter()
        .map(file_item)
        .collect();
        let notice = empty_notice(&items, text, "file", Some("Type a file or folder name"));
        scope_list(text, items, notice)
    } else {
        return None;
    };
    Some(response)
}

/// A scope's rows; once something is typed, Enter takes the first.
fn scope_list(text: &str, items: Vec<Suggestion>, notice: Option<String>) -> SuggestResponse {
    SuggestResponse {
        mode: "search".into(),
        preselect: (!items.is_empty() && !text.is_empty()).then_some(0),
        items,
        notice,
        text: text.to_string(),
        ..Default::default()
    }
}

/// What an empty scope says: that nothing matches the text, or `idle` before anything is typed.
fn empty_notice(
    items: &[Suggestion],
    text: &str,
    kind: &str,
    idle: Option<&str>,
) -> Option<String> {
    if !items.is_empty() {
        None
    } else if !text.is_empty() {
        Some(format!("No {kind} matches {text}"))
    } else {
        idle.map(String::from)
    }
}

/// A bare "settings": the system's settings app, the app's own Settings, then the usual pages.
fn bare_settings(text: &str, catalog: &Catalog<'_>) -> SuggestResponse {
    let mut items: Vec<Suggestion> = catalog
        .system_settings_app()
        .map(app_item)
        .into_iter()
        .collect();
    items.extend(
        catalog
            .settings
            .iter()
            .find(|setting| setting.id == "zephyr.settings")
            .map(setting_item),
    );
    items.extend(
        settings::favorites(catalog.settings, catalog.setting_opens, catalog.now, 4)
            .into_iter()
            .map(setting_item),
    );
    SuggestResponse {
        mode: "search".into(),
        preselect: (!items.is_empty()).then_some(0),
        items,
        notice: None,
        target: Some("web".into()),
        text: text.to_string(),
        remote: false,
    }
}

/// Unscoped text that isn't a link: settings pages, apps, bookmarks, an answer, recent
/// searches and files, over a web search for the text.
fn unscoped(text: &str, history: &[HistoryEntry], catalog: &Catalog<'_>) -> SuggestResponse {
    let now = catalog.now;
    // "wifi settings", "settings dark": the pages lead and Enter opens the best; the web
    // row stays right under them. An app named exactly this text still wins.
    let intent: Vec<Suggestion> = catalog
        .setting_intent(text)
        .into_iter()
        .map(match_item)
        .collect();
    let (preferred, other_apps) = app_rows(text, catalog, !intent.is_empty());
    let (exact_bookmark, other_bookmarks) = bookmark_rows(text, preferred.is_some());

    // A page named exactly ("bluetooth", "dark mode") sits right under the web row; the web
    // search keeps Enter.
    let exact_pane: Option<Suggestion> =
        (intent.is_empty() && preferred.is_none() && exact_bookmark.is_none())
            .then(|| settings::exact(catalog.settings, text, catalog.setting_opens, now))
            .flatten()
            .map(match_item);
    let incidental = incidental_rows(
        text,
        catalog,
        preferred,
        &other_apps,
        &intent,
        exact_pane.as_ref(),
    );

    // A calculation or time question answers in the top row, unless an app already claims
    // Enter for this text.
    let answer = (preferred.is_none() && exact_bookmark.is_none() && intent.is_empty())
        .then(|| answer::inline(text, chrono::Utc::now()))
        .flatten();

    let has_intent = !intent.is_empty();
    let mut items = intent;
    if let Some(pane) = exact_pane {
        items.push(pane);
    }
    if let Some(app) = preferred {
        items.push(app_item(app));
    }
    if let Some(bookmark) = &exact_bookmark {
        items.push(bookmark_item(bookmark));
    }
    if let Some(answer) = &answer {
        items.push(answer_item(answer, text));
    }
    // An app match keeps the apps together at the top; otherwise recent searches lead.
    if preferred.is_some() {
        items.extend(other_apps);
        items.extend(history_items(history, text, now, 4));
    } else {
        items.extend(history_items(history, text, now, 4));
        items.extend(other_apps);
    }
    items.extend(other_bookmarks);
    items.truncate(8 - incidental.len());
    items.extend(incidental);

    // Suggestions only help when free text is going to a search: not under an app match or
    // an inline answer.
    let remote = !text.is_empty() && preferred.is_none() && answer.is_none();
    SuggestResponse {
        mode: "search".into(),
        items,
        notice: None,
        preselect: (preferred.is_some()
            || answer.is_some()
            || exact_bookmark.is_some()
            || has_intent)
            .then_some(0),
        target: Some("web".into()),
        text: text.to_string(),
        remote,
    }
}

/// The app Enter would launch (none when settings pages already lead), and up to two other
/// good matches.
fn app_rows<'a>(
    text: &str,
    catalog: &Catalog<'a>,
    pages_lead: bool,
) -> (Option<&'a App>, Vec<Suggestion>) {
    // Ranked once: the app Enter would launch is the best of these.
    let app_matches = catalog.ranked(text, 4);
    let preferred = (!pages_lead)
        .then(|| apps::preferred_of(&app_matches, catalog.overrides, text))
        .flatten();
    let others = app_matches
        .into_iter()
        .filter(|(app, found)| {
            *found >= apps::Strength::Substring
                && preferred.is_none_or(|chosen| chosen.id != app.id)
        })
        .take(2)
        .map(|(app, _)| app_item(app))
        .collect();
    (preferred, others)
}

/// A bookmark titled exactly what was typed ("music") opens on Enter, like an app name;
/// other matches show below but never take Enter, so searching stays the default.
fn bookmark_rows(text: &str, app_claims_enter: bool) -> (Option<Bookmark>, Vec<Suggestion>) {
    let exact = (!app_claims_enter)
        .then(|| bookmarks::exact(text))
        .flatten();
    let others = if text.trim().len() >= 2 {
        bookmarks::search(text, 3)
            .iter()
            .filter(|found| exact.as_ref().is_none_or(|exact| exact.url != found.url))
            .take(2)
            .map(bookmark_item)
            .collect()
    } else {
        Vec::new()
    };
    (exact, others)
}

/// Files and settings never take Enter unscoped: at most two rows between them, below
/// everything else local. Files only come from ones opened before, so this never waits on
/// the index.
fn incidental_rows(
    text: &str,
    catalog: &Catalog<'_>,
    preferred: Option<&App>,
    other_apps: &[Suggestion],
    intent: &[Suggestion],
    exact_pane: Option<&Suggestion>,
) -> Vec<Suggestion> {
    let opened = files::incidental(catalog.file_opens, text, catalog.now, 2);
    let mut rows: Vec<Suggestion> = opened.iter().map(file_item).collect();
    // A system tool that is also an installed app (Registry Editor) shows once, as the app.
    let shown_app = |title: &str| {
        preferred.is_some_and(|app| app.name.eq_ignore_ascii_case(title))
            || other_apps
                .iter()
                .any(|item| item.label.eq_ignore_ascii_case(title))
    };
    rows.extend(
        settings::incidental(catalog.settings, text, 2)
            .into_iter()
            .filter(|setting| !shown_app(setting.title))
            .filter(|setting| {
                let id = Some(setting.id.to_string());
                !intent.iter().any(|row| row.setting_id == id)
                    && exact_pane.is_none_or(|row| row.setting_id != id)
            })
            .take(2 - rows.len())
            .map(setting_item),
    );
    rows
}

fn answer_item(answer: &answer::Answer, text: &str) -> Suggestion {
    Suggestion {
        label: answer.value.clone(),
        query: answer.value.clone(),
        kind: "answer".into(),
        hint: "Copy".into(),
        section: "answer".into(),
        subtitle: text.to_string(),
        icon: match answer.hint {
            "Conversion" => "glyph:conversion",
            "Time" => "glyph:clock",
            _ => "glyph:calculator",
        }
        .into(),
        ..Default::default()
    }
}

/// The port answers on a blocking thread: reading the sockets runs `lsof` on macOS.
async fn port_answers_off_thread(text: String, port: Option<u16>) -> SuggestResponse {
    tauri::async_runtime::spawn_blocking(move || port_answers(&text, port))
        .await
        .unwrap_or_default()
}

/// "port 3000": the process using the port as a card, and any others below it. "ports":
/// every listening port as a row. The sockets and processes are read at most once every
/// two seconds (ports.rs), so typing ":3000" reads them once.
fn port_answers(text: &str, port: Option<u16>) -> SuggestResponse {
    let uses = match port {
        Some(port) => crate::net::ports::on_port(port),
        None => crate::net::ports::listening(),
    };
    let row = |item: crate::net::ports::PortUse, card: bool| {
        let name = if item.name.is_empty() {
            format!("PID {}", item.pid)
        } else {
            item.name.clone()
        };
        let state = if item.state.is_empty() {
            String::new()
        } else {
            format!(" · {}", item.state)
        };
        Suggestion {
            label: if card {
                name.clone()
            } else {
                format!(":{} {name}", item.port)
            },
            query: item.pid.to_string(),
            target: item.port.to_string(),
            kind: if card { "portCard" } else { "port" }.into(),
            hint: format!("PID {}", item.pid),
            section: if card { "card" } else { "ports" }.into(),
            subtitle: format!("{} {}{state}", item.protocol, item.local),
            icon: item.icon.clone(),
            port_use: Some(item),
            ..Default::default()
        }
    };
    let card = port.is_some();
    let items: Vec<Suggestion> = uses
        .into_iter()
        .enumerate()
        .map(|(index, item)| row(item, card && index == 0))
        .collect();
    let notice = match (port, items.is_empty()) {
        (Some(port), true) => Some(format!("Nothing is using port {port}")),
        (None, true) => Some("Nothing is listening".to_string()),
        _ => None,
    };
    SuggestResponse {
        mode: "search".into(),
        preselect: (!items.is_empty()).then_some(0),
        items,
        notice,
        target: Some("web".into()),
        text: text.to_string(),
        remote: false,
    }
}

/// "ip", "my ip": the network card with the local address the internet is reached through,
/// preselected so Enter copies it, then the public address (asked only on the pass that may
/// use the network), then the computer's other addresses.
async fn ip_answers(text: &str, include_remote: bool) -> SuggestResponse {
    // A globe for the public address; the local ones show their link (Wi-Fi, Ethernet).
    let row = |value: String, detail: String, icon: &str| Suggestion {
        label: value.clone(),
        query: value,
        kind: "answer".into(),
        hint: "Copy".into(),
        section: "answer".into(),
        subtitle: detail,
        icon: icon.into(),
        ..Default::default()
    };
    let local = crate::net::ip::local();
    let mut items: Vec<Suggestion> = Vec::new();
    let mut rest = local.into_iter();
    if let Some(primary) = rest.next() {
        items.push(Suggestion {
            kind: "netCard".into(),
            target: "ip".into(),
            section: "card".into(),
            icon: primary.link.glyph().into(),
            ..row(primary.value, primary.detail, "")
        });
    }
    if include_remote && let Some(public) = crate::net::ip::public().await {
        items.push(row(
            public,
            "Public IP, as the internet sees this computer".into(),
            "glyph:globe",
        ));
    }
    items.extend(rest.map(|address| {
        let icon = address.link.glyph();
        row(address.value, address.detail, icon)
    }));
    SuggestResponse {
        mode: "search".into(),
        preselect: (!items.is_empty()).then_some(0),
        items,
        notice: None,
        target: Some("web".into()),
        text: text.to_string(),
        // The public address comes on the network pass.
        remote: true,
    }
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
    let private_window = crate::apps::private_mode(&app.name).map(|mode| mode.name.to_string());
    let shift = if cfg!(target_os = "macos") {
        "⇧"
    } else {
        "Shift+"
    };
    Suggestion {
        label: app.name.clone(),
        kind: "app".into(),
        hint: if app.id.starts_with(crate::steam::ID_PREFIX) {
            "Game".into()
        } else if let Some(private) = &private_window {
            format!("App · {shift}↵ {private}")
        } else {
            "App".into()
        },
        section: "apps".into(),
        icon: format!("app:{}", app.id),
        app_id: Some(app.id.clone()),
        private_window,
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
            "zephyr"
        } else {
            "glyph:gear"
        }
        .into(),
        setting_id: Some(setting.id.to_string()),
        ..Default::default()
    }
}

/// A page found by a keyword shows that keyword dimmed beside its title ("Appearance
/// dark mode").
fn match_item(found: settings::Match<'_>) -> Suggestion {
    Suggestion {
        subtitle: found.keyword.unwrap_or_default().to_string(),
        ..setting_item(found.setting)
    }
}

fn bookmark_item(bookmark: &Bookmark) -> Suggestion {
    Suggestion {
        label: bookmark.title.clone(),
        query: bookmark.url.clone(),
        kind: "bookmark".into(),
        hint: bookmarks::host(&bookmark.url),
        subtitle: bookmark.folder.clone(),
        section: "bookmarks".into(),
        icon: format!("site:{}", bookmark.url),
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

/// What an empty `!set` lists: the app's own Settings window, then the most-used OS pages.
fn starters(all: &[Setting], limit: usize) -> Vec<&Setting> {
    let own = all.iter().find(|setting| setting.id == "zephyr.settings");
    let system = all
        .iter()
        .filter(|setting| !matches!(setting.target, Target::Zephyr(_)));
    own.into_iter().chain(system).take(limit).collect()
}

/// The empty bar: the last two searches (Recent) on top, then the last three apps opened from
/// the bar, newest first. The bar adds its utilities below, and that is all that fits without
/// scrolling.
fn recent(history: &[HistoryEntry], catalog: &Catalog<'_>) -> SuggestResponse {
    let mut launches: Vec<&apps::LaunchEntry> = catalog.launches.iter().collect();
    launches.sort_by_key(|entry| std::cmp::Reverse(entry.last_used));
    let mut opened: Vec<&App> = launches
        .iter()
        .filter_map(|entry| catalog.apps.iter().find(|app| app.id == entry.app_id))
        .take(3)
        .collect();
    // Until three apps have been opened from the bar (a first launch), common ones fill in,
    // so the empty bar always offers some.
    for app in starter_apps(catalog.apps) {
        if opened.len() >= 3 {
            break;
        }
        if !opened.iter().any(|shown| shown.id == app.id) {
            opened.push(app);
        }
    }
    let apps: Vec<Suggestion> = opened.into_iter().map(app_item).collect();

    let mut searched: Vec<&HistoryEntry> = history.iter().collect();
    searched.sort_by_key(|entry| std::cmp::Reverse(entry.last_used));
    let searches: Vec<Suggestion> = searched
        .into_iter()
        .take(2)
        .map(|entry| history_item(entry.clone()))
        .collect();

    let items = [searches, apps].concat();
    SuggestResponse {
        mode: "recent".into(),
        items,
        ..Default::default()
    }
}

/// Everyday apps that are installed, most likely first, for an empty bar with no history yet.
fn starter_apps(installed: &[App]) -> Vec<&App> {
    const COMMON: &[&str] = &[
        "Google Chrome",
        "Microsoft Edge",
        "Firefox",
        "Safari",
        "File Explorer",
        "Finder",
        "Terminal",
        "Visual Studio Code",
        "Spotify",
        "Discord",
        "Steam",
        "Settings",
        "System Settings",
        "Notepad",
        "Notes",
        "Calculator",
    ];
    COMMON
        .iter()
        .filter_map(|name| {
            installed
                .iter()
                .find(|app| app.name.eq_ignore_ascii_case(name))
        })
        .collect()
}

fn history_items(history: &[HistoryEntry], query: &str, now: i64, limit: usize) -> Vec<Suggestion> {
    history::matching(history, query, now, limit)
        .into_iter()
        .map(history_item)
        .collect()
}

fn history_item(entry: HistoryEntry) -> Suggestion {
    let link = entry.is_link();
    Suggestion {
        label: entry.query.clone(),
        query: entry.query,
        target: if link { "url" } else { "web" }.into(),
        kind: "history".into(),
        hint: if link { "Link" } else { "Google" }.into(),
        section: "recent".into(),
        // Google's G as a silhouette in the row's color.
        icon: if link { "glyph:link" } else { "glyph:google" }.into(),
        ..Default::default()
    }
}

fn merge_remote(items: &mut Vec<Suggestion>, remote: Vec<String>) {
    let mut seen: HashSet<String> = items.iter().map(|item| item.query.to_lowercase()).collect();
    for label in remote {
        if !seen.insert(label.to_lowercase()) {
            continue;
        }
        items.push(Suggestion {
            query: label.clone(),
            label,
            target: "web".into(),
            kind: "remote".into(),
            section: "web".into(),
            icon: "glyph:search".into(),
            ..Default::default()
        });
    }
}

/// Opens the connection to Google's suggestion service ahead of the first keystroke; the
/// first real fetch otherwise pays for DNS and TLS (1.25 s in a capture, vs ~60 ms after).
pub async fn warm() {
    let _ = fetch_remote("").await;
}

async fn fetch_remote(query: &str) -> Vec<String> {
    let url = crate::web::suggest_url(query);
    let Some(client) = http_client() else {
        return Vec::new();
    };
    let response = match client.get(url).timeout(Duration::from_secs(4)).send().await {
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
    string_list(value.get(1), query)
}

fn http_client() -> Option<&'static reqwest::Client> {
    Some(crate::util::http())
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
        // A suggestion that is just the typed text repeats what Enter already does.
        if text.is_empty() || text.eq_ignore_ascii_case(query.trim()) {
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
            setting_opens: &[],
            now: 0,
        };
        tauri::async_runtime::block_on(gather(input, &[], catalog, false))
    }

    fn local_full(
        input: &str,
        apps: &[App],
        history: &[HistoryEntry],
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
            setting_opens: &[],
            now: 1_000_000,
        };
        tauri::async_runtime::block_on(gather(input, history, catalog, include_remote))
    }

    fn searched(query: &str, destination_id: &str) -> HistoryEntry {
        HistoryEntry {
            query: query.into(),
            destination_id: destination_id.into(),
            uses: 1,
            last_used: 1_000_000,
        }
    }

    #[test]
    fn a_first_launch_still_offers_common_apps() {
        static NO_FILES: std::sync::LazyLock<crate::files::FileIndex> =
            std::sync::LazyLock::new(crate::files::FileIndex::default);
        let apps: Vec<App> = ["Zoom", "Steam", "Google Chrome", "Notepad", "Calculator"]
            .iter()
            .map(|name| App {
                id: name.to_lowercase(),
                name: (*name).into(),
            })
            .collect();
        let opened = [apps::LaunchEntry {
            app_id: "zoom".into(),
            uses: 1,
            last_used: 5,
        }];
        let catalog = |launches| Catalog {
            apps: &apps,
            settings: SETTINGS,
            files: &NO_FILES,
            launches,
            file_opens: &[],
            overrides: &[],
            setting_opens: &[],
            now: 100,
        };
        let labels = |response: SuggestResponse| -> Vec<String> {
            response.items.into_iter().map(|item| item.label).collect()
        };
        assert_eq!(
            labels(recent(&[], &catalog(&[]))),
            ["Google Chrome", "Steam", "Notepad"]
        );
        // What was opened comes first; common apps only fill the rest.
        assert_eq!(
            labels(recent(&[], &catalog(&opened))),
            ["Zoom", "Google Chrome", "Steam"]
        );
    }

    #[test]
    fn the_empty_bar_puts_recent_searches_above_apps() {
        let apps: Vec<App> = ["Alpha", "Beta", "Gamma", "Delta"]
            .iter()
            .map(|name| App {
                id: name.to_lowercase(),
                name: (*name).into(),
            })
            .collect();
        let launched = |id: &str, at: i64| apps::LaunchEntry {
            app_id: id.into(),
            uses: 9,
            last_used: at,
        };
        // Delta is the most used but the oldest; the newest three show, newest first.
        let launches = [
            launched("delta", 10),
            launched("alpha", 20),
            launched("beta", 40),
            launched("gamma", 30),
        ];
        let at = |query: &str, when: i64| HistoryEntry {
            last_used: when,
            ..searched(query, "google")
        };
        static NO_FILES: std::sync::LazyLock<crate::files::FileIndex> =
            std::sync::LazyLock::new(crate::files::FileIndex::default);
        let catalog = |launches| Catalog {
            apps: &apps,
            settings: SETTINGS,
            files: &NO_FILES,
            launches,
            file_opens: &[],
            overrides: &[],
            setting_opens: &[],
            now: 100,
        };
        let labels = |response: SuggestResponse| {
            response
                .items
                .into_iter()
                .map(|item| item.label)
                .collect::<Vec<_>>()
        };

        let history = [at("old", 5), at("newest", 50), at("middle", 35)];
        assert_eq!(
            labels(recent(&history, &catalog(&launches))),
            ["newest", "middle", "Beta", "Gamma", "Alpha"]
        );
        // Recent stays on top even when an app was opened since.
        let history = [at("before", 15)];
        assert_eq!(
            labels(recent(&history, &catalog(&launches))),
            ["before", "Beta", "Gamma", "Alpha"]
        );
        assert_eq!(labels(recent(&history, &catalog(&[]))), ["before"]);
    }

    #[test]
    fn says_where_free_text_goes() {
        assert_eq!(
            local_full("example.com", &[], &[], false).target.as_deref(),
            Some("url")
        );
        let plain = local_full("cats !w", &[], &[], false);
        assert_eq!(plain.target.as_deref(), Some("web"));
        assert_eq!(plain.text, "cats !w");
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
        let response = local_full("vs", &apps, &history, false);
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
        let response = local_full("example.com", &[], &[], true);
        assert!(response.items.iter().all(|item| item.kind != "remote"));
    }

    #[test]
    fn rows_carry_their_section() {
        let history = [searched("crispr review", "google")];
        let response = local_full("crispr", &[], &history, false);
        let row = response
            .items
            .iter()
            .find(|item| item.kind == "history")
            .unwrap();
        assert_eq!(
            (row.section.as_str(), row.icon.as_str(), row.target.as_str()),
            ("recent", "glyph:google", "web")
        );
        let history = [searched("crispr.org", "url")];
        let response = local_full("crispr", &[], &history, false);
        let row = response
            .items
            .iter()
            .find(|item| item.kind == "history")
            .unwrap();
        assert_eq!(
            (row.icon.as_str(), row.target.as_str()),
            ("glyph:link", "url")
        );
        let answer = local_full("12 * 7", &[], &[], false);
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
        // The page named exactly sits first (right under the web row), then the file.
        assert_eq!(kinds, ["setting", "file", "setting"]);
        assert_eq!(response.items[0].setting_id.as_deref(), Some("win.display"));
        assert_eq!(response.items[1].path.as_deref(), file.to_str());

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

    fn item_labels(response: &SuggestResponse) -> Vec<&str> {
        response
            .items
            .iter()
            .map(|item| item.label.as_str())
            .collect()
    }

    #[test]
    fn the_app_scope_lists_only_apps_and_says_when_none_match() {
        let apps = [App {
            id: "code".into(),
            name: "Visual Studio Code".into(),
        }];
        let response = local("!app code", &apps);
        assert_eq!(item_labels(&response), ["Visual Studio Code"]);
        assert_eq!(response.preselect, Some(0));
        assert_eq!(response.text, "code");

        let none = local("!app zzz", &apps);
        assert!(none.items.is_empty());
        assert_eq!(none.notice.as_deref(), Some("No app matches zzz"));

        assert_eq!(local("!app", &apps).notice, None);
    }

    #[test]
    fn the_typing_scope_only_offers_the_test() {
        let response = local("!type", &[]);
        assert!(response.items.is_empty());
        assert_eq!(
            response.notice.as_deref(),
            Some("Press Enter to start a typing test")
        );
    }

    #[test]
    fn a_link_keeps_recent_searches_but_no_apps_or_answers() {
        let apps = [App {
            id: "example".into(),
            name: "example.com".into(),
        }];
        let history = [searched("example.com/docs", "url")];
        let response = local_full("example.com", &apps, &history, false);
        assert_eq!(item_labels(&response), ["example.com/docs"]);
        assert_eq!(response.preselect, None);
        assert!(!response.remote);
    }

    #[test]
    fn parses_google_suggestions() {
        let google: Value =
            serde_json::from_str(r#"["crispr",["crispr","crispr cas9"],[],{}]"#).unwrap();
        assert_eq!(string_list(google.get(1), "crispr"), vec!["crispr cas9"]);
    }
}
