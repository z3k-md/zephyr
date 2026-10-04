use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Destination {
    pub id: String,
    pub name: String,
    pub triggers: Vec<String>,
    pub url_template: String,
    pub suggest: SuggestKind,
    pub pinned: bool,
    pub builtin: bool,
    pub disabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestKind {
    None,
    Google,
    Youtube,
    Wikipedia,
    Pubmed,
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
        pinned: true,
        builtin: true,
        disabled: false,
    }
}

pub fn build_url(template: &str, query: &str) -> Result<String, String> {
    if !(template.starts_with("https://") || template.starts_with("http://")) {
        return Err("URL template must start with http:// or https://".into());
    }
    if !template.contains("{query}") {
        return Err("URL template must include {query}".into());
    }
    let encoded = urlencoding::encode(query);
    Ok(template.replace("{query}", encoded.as_ref()))
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
    fn rejects_templates_that_cannot_carry_a_query() {
        assert!(build_url("https://example.com", "q").is_err());
        assert!(build_url("ftp://example.com/?q={query}", "q").is_err());
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
