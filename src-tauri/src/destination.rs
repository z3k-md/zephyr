use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Destination {
    pub id: String,
    pub name: String,
    pub triggers: Vec<String>,
    pub url_template: String,
    pub suggest: SuggestKind,
    /// For `SuggestKind::Custom`: a URL with `{query}` that returns JSON suggestions.
    #[serde(default)]
    pub suggest_url: String,
    /// For `SuggestKind::Custom`: where the suggestions are in that JSON, e.g. `1` or
    /// `items.*.title`.
    #[serde(default)]
    pub suggest_path: String,
    pub pinned: bool,
    pub builtin: bool,
    pub disabled: bool,
    /// Web destinations open a URL; the AI destination answers in the bar.
    #[serde(default)]
    pub kind: DestinationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DestinationKind {
    #[default]
    Web,
    Ai,
}

impl Destination {
    pub fn is_ai(&self) -> bool {
        self.kind == DestinationKind::Ai
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestKind {
    None,
    Google,
    Youtube,
    Wikipedia,
    Pubmed,
    Custom,
}

impl Destination {
    /// The suggestion URL template and JSON path this destination uses, if any.
    pub fn suggest_source(&self) -> Option<(&str, &str)> {
        Some(match self.suggest {
            SuggestKind::None => return None,
            SuggestKind::Google => (
                "https://suggestqueries.google.com/complete/search?client=firefox&q={query}",
                "1",
            ),
            SuggestKind::Youtube => (
                "https://suggestqueries.google.com/complete/search?client=firefox&ds=yt&q={query}",
                "1",
            ),
            SuggestKind::Wikipedia => (
                "https://en.wikipedia.org/w/api.php?action=opensearch&search={query}&limit=6&namespace=0&format=json",
                "1",
            ),
            SuggestKind::Pubmed => (
                "https://pubmed.ncbi.nlm.nih.gov/suggestions/?term={query}",
                "suggestions",
            ),
            SuggestKind::Custom => (self.suggest_url.as_str(), self.suggest_path.as_str()),
        })
    }
}

/// Checks a custom suggestion source before it is saved.
pub fn validate_suggest_source(url: &str, path: &str) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("The suggestion URL must start with https://".into());
    }
    if !url.contains("{query}") {
        return Err("The suggestion URL must include {query}".into());
    }
    if path.split('.').any(str::is_empty) && !path.is_empty() {
        return Err("The JSON path can't have empty parts, e.g. items.*.title".into());
    }
    Ok(())
}

pub fn builtins() -> Vec<Destination> {
    vec![
        dest(
            "google",
            "Google",
            &["g", "google"],
            "https://www.google.com/search?q={query}",
            SuggestKind::Google,
        ),
        Destination {
            kind: DestinationKind::Ai,
            ..dest("ai", "Ask AI", &["ai"], "", SuggestKind::None)
        },
        dest(
            "chatgpt",
            "ChatGPT",
            &["gpt", "chatgpt"],
            "https://chatgpt.com/?q={query}",
            SuggestKind::None,
        ),
        dest(
            "claude",
            "Claude",
            &["claude"],
            "https://claude.ai/new?q={query}",
            SuggestKind::None,
        ),
        dest(
            "wikipedia",
            "Wikipedia",
            &["w", "wiki", "wikipedia"],
            "https://en.wikipedia.org/wiki/Special:Search?search={query}",
            SuggestKind::Wikipedia,
        ),
        dest(
            "pubmed",
            "PubMed",
            &["pm", "pubmed"],
            "https://pubmed.ncbi.nlm.nih.gov/?term={query}",
            SuggestKind::Pubmed,
        ),
        dest(
            "github",
            "GitHub",
            &["gh", "github"],
            "https://github.com/search?q={query}",
            SuggestKind::None,
        ),
        dest(
            "stackoverflow",
            "Stack Overflow",
            &["so", "stackoverflow"],
            "https://stackoverflow.com/search?q={query}",
            SuggestKind::None,
        ),
        dest(
            "youtube",
            "YouTube",
            &["yt", "youtube"],
            "https://www.youtube.com/results?search_query={query}",
            SuggestKind::Youtube,
        ),
    ]
}

fn dest(
    id: &str,
    name: &str,
    triggers: &[&str],
    url_template: &str,
    suggest: SuggestKind,
) -> Destination {
    Destination {
        id: id.to_string(),
        name: name.to_string(),
        triggers: triggers
            .iter()
            .map(|trigger| (*trigger).to_string())
            .collect(),
        url_template: url_template.to_string(),
        suggest,
        suggest_url: String::new(),
        suggest_path: String::new(),
        pinned: true,
        builtin: true,
        disabled: false,
        kind: DestinationKind::Web,
    }
}

/// Fills a destination's template for `query`, reading the clipboard only if it asks for it.
pub fn build_url(template: &str, query: &str) -> Result<String, String> {
    let clipboard = || {
        arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.get_text())
            .ok()
    };
    crate::template::render(
        template,
        &crate::template::Context {
            query,
            now: chrono::Local::now(),
            clipboard: &clipboard,
        },
    )
}

pub fn normalize_trigger(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_start_matches('!').to_lowercase();
    if is_trigger(&trimmed) {
        Some(trimmed)
    } else {
        None
    }
}

pub fn is_trigger(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && value.len() <= 32
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

pub fn enabled<'a>(destinations: &'a [Destination], id: &str) -> Option<&'a Destination> {
    destinations
        .iter()
        .find(|destination| destination.id == id && !destination.disabled)
}

pub fn exact_trigger<'a>(
    destinations: &'a [Destination],
    trigger: &str,
) -> Option<&'a Destination> {
    let trigger = trigger.to_lowercase();
    destinations.iter().find(|destination| {
        !destination.disabled
            && destination
                .triggers
                .iter()
                .any(|candidate| candidate == &trigger)
    })
}

pub fn filter_destinations<'a>(
    destinations: &'a [Destination],
    prefix: &str,
) -> Vec<&'a Destination> {
    let prefix = prefix.to_lowercase();
    destinations
        .iter()
        .filter(|destination| !destination.disabled && matches_prefix(destination, &prefix))
        .collect()
}

fn matches_prefix(destination: &Destination, prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }
    destination.name.to_lowercase().contains(prefix)
        || destination
            .triggers
            .iter()
            .any(|trigger| trigger.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_the_query_into_the_template() {
        let url = build_url("https://example.com/search?q={query}", "a b&c").unwrap();
        assert_eq!(url, "https://example.com/search?q=a%20b%26c");
    }

    #[test]
    fn rejects_templates_without_a_target() {
        assert!(build_url("example.com/?q={query}", "q").is_err());
        assert!(build_url("https://example.com/?q={query", "q").is_err());
    }

    #[test]
    fn builtin_triggers_do_not_collide() {
        let mut seen = std::collections::HashSet::new();
        for destination in builtins() {
            for trigger in destination.triggers {
                assert!(seen.insert(trigger.clone()), "duplicate trigger {trigger}");
            }
        }
    }
}
