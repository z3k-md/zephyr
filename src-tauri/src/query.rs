use serde::Serialize;

use crate::apps::{self, Catalog};
use crate::destination::{self, Destination};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub query: String,
    pub bang: Option<String>,
    pub only_bang: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Open {
        destination_id: String,
        query: String,
        url: String,
    },
    Arm {
        destination_id: String,
    },
    Launch {
        app_id: String,
    },
    Palette,
    UnknownBang {
        trigger: String,
    },
    Empty,
    Rejected {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum DispatchOutcome {
    Opened { destination_id: String },
    Armed { destination_id: String },
    Launched { app_id: String },
    Palette,
    UnknownBang { trigger: String },
    Empty,
}

pub fn parse_input(input: &str) -> Parsed {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Parsed {
            query: String::new(),
            bang: None,
            only_bang: false,
        };
    }
    if trimmed == "!" {
        return Parsed {
            query: String::new(),
            bang: Some(String::new()),
            only_bang: true,
        };
    }

    let mut bangs = Vec::new();
    let mut words = Vec::new();
    for token in trimmed.split_whitespace() {
        if let Some(body) = token.strip_prefix('!')
            && destination::is_trigger(body)
        {
            bangs.push(body.to_lowercase());
            continue;
        }
        words.push(token);
    }

    Parsed {
        query: words.join(" "),
        bang: bangs.last().cloned(),
        only_bang: words.is_empty() && !bangs.is_empty(),
    }
}

pub fn decide(
    input: &str,
    armed_id: &str,
    explicit_id: &str,
    interpret: bool,
    destinations: &[Destination],
    catalog: Catalog<'_>,
) -> Decision {
    if !interpret {
        return decide_explicit(input, explicit_id, destinations);
    }

    let parsed = parse_input(input);
    if parsed.only_bang {
        let prefix = parsed.bang.unwrap_or_default();
        if prefix.is_empty() {
            return Decision::Palette;
        }
        if apps::is_scope(&prefix) {
            return Decision::Empty;
        }
        return match destination::exact_trigger(destinations, &prefix) {
            Some(destination) => Decision::Arm {
                destination_id: destination.id.clone(),
            },
            None => Decision::Palette,
        };
    }

    if let Some(trigger) = parsed.bang.as_deref()
        && apps::is_scope(trigger)
    {
        return match catalog.ranked(&parsed.query, 1).first() {
            Some((app, _)) => Decision::Launch {
                app_id: app.id.clone(),
            },
            None => Decision::Rejected {
                message: format!("No app matches {}", parsed.query),
            },
        };
    }

    if let Some(trigger) = parsed.bang {
        return match destination::exact_trigger(destinations, &trigger) {
            Some(destination) => open_search(destination, &parsed.query),
            None => Decision::UnknownBang { trigger },
        };
    }

    if parsed.query.is_empty() {
        return Decision::Empty;
    }

    if looks_like_url(&parsed.query) {
        return Decision::Open {
            destination_id: "url".into(),
            query: ensure_scheme(&parsed.query),
            url: ensure_scheme(&parsed.query),
        };
    }

    if let Some(app) = catalog.preferred(&parsed.query) {
        return Decision::Launch {
            app_id: app.id.clone(),
        };
    }

    match find(destinations, armed_id) {
        Found::Ready(destination) => open_search(destination, &parsed.query),
        Found::Disabled(name) => Decision::Rejected {
            message: format!("{name} is turned off"),
        },
        Found::Missing => Decision::Rejected {
            message: "That destination is missing.".into(),
        },
    }
}

fn decide_explicit(input: &str, explicit_id: &str, destinations: &[Destination]) -> Decision {
    // A dispatch key chooses the destination. A bang in the text is syntax, not part of the query.
    let query = parse_input(input).query;
    if query.is_empty() {
        return match find(destinations, explicit_id) {
            Found::Ready(destination) => Decision::Arm {
                destination_id: destination.id.clone(),
            },
            Found::Disabled(name) => Decision::Rejected {
                message: format!("{name} is turned off"),
            },
            Found::Missing => Decision::Empty,
        };
    }

    if explicit_id == "url" {
        let url = ensure_scheme(&query);
        return Decision::Open {
            destination_id: "url".into(),
            query: url.clone(),
            url,
        };
    }

    match find(destinations, explicit_id) {
        Found::Ready(destination) => open_search(destination, &query),
        Found::Disabled(name) => Decision::Rejected {
            message: format!("{name} is turned off"),
        },
        Found::Missing => Decision::Rejected {
            message: "That destination is missing.".into(),
        },
    }
}

fn open_search(destination: &Destination, query: &str) -> Decision {
    if query.is_empty() {
        return Decision::Arm {
            destination_id: destination.id.clone(),
        };
    }
    match destination::build_url(&destination.url_template, query) {
        Ok(url) => Decision::Open {
            destination_id: destination.id.clone(),
            query: query.to_string(),
            url,
        },
        Err(message) => Decision::Rejected { message },
    }
}

enum Found<'a> {
    Ready(&'a Destination),
    Disabled(String),
    Missing,
}

fn find<'a>(destinations: &'a [Destination], id: &str) -> Found<'a> {
    match destinations.iter().find(|destination| destination.id == id) {
        Some(destination) if destination.disabled => Found::Disabled(destination.name.clone()),
        Some(destination) => Found::Ready(destination),
        None => Found::Missing,
    }
}

pub fn looks_like_url(input: &str) -> bool {
    let value = input.trim();
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return false;
    }
    if value.starts_with("http://") || value.starts_with("https://") {
        return true;
    }

    let host = value.split(['/', '?', '#']).next().unwrap_or(value);
    let host_only = match host.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|ch| ch.is_ascii_digit()) => {
            host
        }
        _ => host,
    };

    if host_only.eq_ignore_ascii_case("localhost") {
        return true;
    }

    host_only.contains('.')
        && !host_only.starts_with('.')
        && !host_only.ends_with('.')
        && host_only.chars().any(|ch| ch.is_ascii_alphabetic())
        && !host_only.contains('\\')
}

fn ensure_scheme(url: &str) -> String {
    let url = url.trim();
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        format!("https://{url}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::{App, Catalog};
    use crate::destination::builtins;

    fn none() -> Catalog<'static> {
        Catalog {
            apps: &[],
            launches: &[],
            overrides: &[],
            now: 0,
        }
    }

    fn installed<'a>(apps: &'a [App], overrides: &'a [String]) -> Catalog<'a> {
        Catalog {
            apps,
            launches: &[],
            overrides,
            now: 0,
        }
    }

    fn excel() -> Vec<App> {
        vec![App {
            id: "excel-id".into(),
            name: "Excel".into(),
        }]
    }

    #[test]
    fn an_app_name_launches_and_free_text_still_searches() {
        let apps = excel();
        let destinations = builtins();
        assert_eq!(
            decide(
                "exc",
                "google",
                "google",
                true,
                &destinations,
                installed(&apps, &[])
            ),
            Decision::Launch {
                app_id: "excel-id".into()
            }
        );
        assert!(
            open(&decide(
                "excel pivot tables",
                "google",
                "google",
                true,
                &destinations,
                installed(&apps, &[])
            ))
            .contains("google.com")
        );
    }

    #[test]
    fn explicit_keys_beat_an_app_match() {
        let apps = excel();
        let destinations = builtins();
        assert!(
            open(&decide(
                "excel !g",
                "google",
                "google",
                true,
                &destinations,
                installed(&apps, &[])
            ))
            .contains("google.com")
        );
        assert!(
            open(&decide(
                "excel",
                "google",
                "wikipedia",
                false,
                &destinations,
                installed(&apps, &[])
            ))
            .contains("wikipedia.org")
        );
    }

    #[test]
    fn a_learned_override_keeps_the_text_on_the_web() {
        static OVERRIDES: std::sync::LazyLock<Vec<String>> =
            std::sync::LazyLock::new(|| vec!["excel".to_string()]);
        let apps = excel();
        assert!(
            open(&decide(
                "excel",
                "google",
                "google",
                true,
                &builtins(),
                installed(&apps, &OVERRIDES)
            ))
            .contains("google.com")
        );
    }

    #[test]
    fn the_app_scope_launches_the_best_match_or_says_none() {
        let apps = excel();
        let destinations = builtins();
        assert_eq!(
            decide(
                "!app xl",
                "google",
                "google",
                true,
                &destinations,
                installed(&apps, &[])
            ),
            Decision::Rejected {
                message: "No app matches xl".into()
            }
        );
        assert_eq!(
            decide(
                "cel !app",
                "google",
                "google",
                true,
                &destinations,
                installed(&apps, &[])
            ),
            Decision::Launch {
                app_id: "excel-id".into()
            }
        );
        assert_eq!(
            decide(
                "!app",
                "google",
                "google",
                true,
                &destinations,
                installed(&apps, &[])
            ),
            Decision::Empty
        );
    }

    fn open(decision: &Decision) -> &str {
        match decision {
            Decision::Open { url, .. } => url,
            other => panic!("expected open, got {other:?}"),
        }
    }

    #[test]
    fn enter_uses_the_armed_destination() {
        let decision = decide("crispr", "pubmed", "pubmed", true, &builtins(), none());
        assert!(open(&decision).contains("pubmed.ncbi.nlm.nih.gov"));
        assert!(open(&decision).contains("crispr"));
    }

    #[test]
    fn bangs_work_at_either_end_and_the_last_one_wins() {
        let destinations = builtins();
        assert!(
            open(&decide(
                "!pm crispr",
                "google",
                "google",
                true,
                &destinations,
                none()
            ))
            .contains("pubmed")
        );
        assert!(
            open(&decide(
                "crispr !wiki",
                "google",
                "google",
                true,
                &destinations,
                none()
            ))
            .contains("wikipedia.org")
        );
        assert!(
            open(&decide(
                "crispr !wiki !yt",
                "google",
                "google",
                true,
                &destinations,
                none()
            ))
            .contains("youtube.com")
        );
    }

    #[test]
    fn an_exact_bang_with_no_query_arms_that_destination() {
        let decision = decide("!wiki", "google", "google", true, &builtins(), none());
        assert_eq!(
            decision,
            Decision::Arm {
                destination_id: "wikipedia".into()
            }
        );
    }

    #[test]
    fn a_partial_bang_opens_the_palette() {
        assert_eq!(
            decide("!you", "google", "google", true, &builtins(), none()),
            Decision::Palette
        );
        assert_eq!(
            decide("!", "google", "google", true, &builtins(), none()),
            Decision::Palette
        );
    }

    #[test]
    fn unknown_bangs_do_not_fall_through_to_google() {
        assert_eq!(
            decide(
                "crispr !nope",
                "google",
                "google",
                true,
                &builtins(),
                none()
            ),
            Decision::UnknownBang {
                trigger: "nope".into()
            }
        );
    }

    #[test]
    fn urls_open_directly() {
        let destinations = builtins();
        assert_eq!(
            open(&decide(
                "example.com/docs",
                "google",
                "google",
                true,
                &destinations,
                none()
            )),
            "https://example.com/docs"
        );
        assert_eq!(
            open(&decide(
                "https://example.com",
                "google",
                "google",
                true,
                &destinations,
                none()
            )),
            "https://example.com"
        );
        assert!(
            open(&decide(
                "localhost:1420",
                "google",
                "google",
                true,
                &destinations,
                none()
            ))
            .starts_with("https://localhost:1420")
        );
    }

    #[test]
    fn ordinary_words_and_punctuation_stay_searches() {
        let destinations = builtins();
        let decision = decide(
            "C++ templates",
            "google",
            "google",
            true,
            &destinations,
            none(),
        );
        let url = open(&decision);
        assert!(url.contains("google.com"));
        assert!(!looks_like_url("3.14"));
        assert!(!looks_like_url("hello!world"));
        let decision = decide(
            "hello!world",
            "google",
            "google",
            true,
            &destinations,
            none(),
        );
        let searched = open(&decision);
        assert!(searched.contains("google.com"));
        assert!(searched.contains("hello%21world"));
    }

    #[test]
    fn a_chosen_suggestion_uses_that_destination() {
        let decision = decide("crispr", "google", "pubmed", false, &builtins(), none());
        assert!(open(&decision).contains("pubmed"));
    }

    #[test]
    fn a_dispatch_key_strips_a_bang_and_keeps_the_chosen_destination() {
        let decision = decide(
            "crispr !wiki",
            "wikipedia",
            "pubmed",
            false,
            &builtins(),
            none(),
        );
        let url = open(&decision);
        assert!(url.contains("pubmed"));
        assert!(url.contains("crispr"));
        assert!(!url.contains("wiki"));
    }

    #[test]
    fn outcome_json_is_camel_case() {
        let json = serde_json::to_value(DispatchOutcome::UnknownBang {
            trigger: "nope".into(),
        })
        .unwrap();
        assert_eq!(json["kind"], "unknownBang");
        assert_eq!(json["trigger"], "nope");

        let opened = serde_json::to_value(DispatchOutcome::Opened {
            destination_id: "google".into(),
        })
        .unwrap();
        assert_eq!(opened["kind"], "opened");
        assert_eq!(opened["destinationId"], "google");
    }
}
