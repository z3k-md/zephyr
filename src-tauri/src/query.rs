use serde::Serialize;

use crate::apps::{self, Catalog};
use crate::{bookmarks, files, settings};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub query: String,
    /// The bang that narrows the text to one kind of result (`!app`, `!f`, `!set`…), if any.
    pub scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// A web search for the text.
    Search {
        query: String,
    },
    /// A link typed out, opened as it is.
    Link {
        url: String,
    },
    Launch {
        app_id: String,
    },
    OpenSetting {
        setting_id: String,
    },
    OpenFile {
        path: String,
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
    Opened,
    Launched { app_id: String },
    SettingOpened { setting_id: String },
    FileOpened { path: String },
    Empty,
}

/// Bang words that narrow the bar to one kind of result rather than being part of the text.
pub fn is_scope(word: &str) -> bool {
    apps::is_scope(word)
        || settings::is_scope(word)
        || files::is_scope(word)
        || crate::notes::is_scope(word)
        || bookmarks::is_scope(word)
        || word == "type"
}

pub fn parse_input(input: &str) -> Parsed {
    let mut scopes = Vec::new();
    let mut words = Vec::new();
    for token in input.split_whitespace() {
        if let Some(body) = token.strip_prefix('!')
            && is_scope(&body.to_lowercase())
        {
            scopes.push(body.to_lowercase());
            continue;
        }
        words.push(token);
    }
    Parsed {
        query: words.join(" "),
        scope: scopes.pop(),
    }
}

/// What Enter does with `input`. `interpret` is off for a picked suggestion or recent
/// search, whose text goes to the web as it is rather than to an app it happens to name.
pub fn decide(input: &str, interpret: bool, catalog: Catalog<'_>) -> Decision {
    let parsed = parse_input(input);
    if parsed.query.is_empty() {
        return Decision::Empty;
    }
    if !interpret {
        return web(&parsed.query);
    }

    match parsed.scope.as_deref() {
        Some(scope) if apps::is_scope(scope) => {
            return match catalog.ranked(&parsed.query, 1).first() {
                Some((app, _)) => Decision::Launch {
                    app_id: app.id.clone(),
                },
                None => Decision::Rejected {
                    message: format!("No app matches {}", parsed.query),
                },
            };
        }
        Some(scope) if settings::is_scope(scope) => {
            return match settings::ranked(catalog.settings, &parsed.query, 1).first() {
                Some(setting) => Decision::OpenSetting {
                    setting_id: setting.id.to_string(),
                },
                None => Decision::Rejected {
                    message: format!("No setting matches {}", parsed.query),
                },
            };
        }
        Some(scope) if files::is_scope(scope) => {
            return match catalog.files(&parsed.query, 1).into_iter().next() {
                Some(hit) => Decision::OpenFile { path: hit.path },
                None => Decision::Rejected {
                    message: format!("No file matches {}", parsed.query),
                },
            };
        }
        // Notes, bookmarks and the typing test open from their rows; plain Enter has
        // nothing else to do.
        Some(_) => return Decision::Empty,
        None => {}
    }

    if looks_like_url(&parsed.query) {
        return web(&parsed.query);
    }

    // A bare "settings" opens the system's settings app (the app's own Settings when there's none).
    if settings::is_bare(&parsed.query) {
        return match catalog.system_settings_app() {
            Some(app) => Decision::Launch {
                app_id: app.id.clone(),
            },
            None => Decision::OpenSetting {
                setting_id: "zephyr.settings".into(),
            },
        };
    }

    // "wifi settings": the page, unless an app has exactly this name.
    if let Some(found) = catalog.setting_intent(&parsed.query).first() {
        return Decision::OpenSetting {
            setting_id: found.setting.id.to_string(),
        };
    }

    if let Some(app) = catalog.preferred(&parsed.query) {
        return Decision::Launch {
            app_id: app.id.clone(),
        };
    }

    web(&parsed.query)
}

/// A link opens as typed; anything else is searched.
fn web(query: &str) -> Decision {
    if looks_like_url(query) {
        Decision::Link {
            url: ensure_scheme(query),
        }
    } else {
        Decision::Search {
            query: query.to_string(),
        }
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
    use crate::files::FileIndex;
    use crate::settings::{Setting, Target};

    const SETTINGS: &[Setting] = &[
        Setting {
            id: "win.display",
            title: "Display",
            keywords: &["screen", "resolution"],
            target: Target::Uri("ms-settings:display"),
        },
        Setting {
            id: "win.colors",
            title: "Colors",
            keywords: &["dark mode"],
            target: Target::Uri("ms-settings:colors"),
        },
    ];

    fn no_files() -> &'static FileIndex {
        static EMPTY: std::sync::LazyLock<FileIndex> = std::sync::LazyLock::new(FileIndex::default);
        &EMPTY
    }

    fn none() -> Catalog<'static> {
        installed(&[], &[])
    }

    fn installed<'a>(apps: &'a [App], overrides: &'a [String]) -> Catalog<'a> {
        Catalog {
            apps,
            settings: SETTINGS,
            files: no_files(),
            launches: &[],
            file_opens: &[],
            overrides,
            setting_opens: &[],
            now: 0,
        }
    }

    fn excel() -> Vec<App> {
        vec![App {
            id: "excel-id".into(),
            name: "Excel".into(),
        }]
    }

    fn search(query: &str) -> Decision {
        Decision::Search {
            query: query.into(),
        }
    }

    #[test]
    fn an_app_name_launches_and_free_text_still_searches() {
        let apps = excel();
        assert_eq!(
            decide("exc", true, installed(&apps, &[])),
            Decision::Launch {
                app_id: "excel-id".into()
            }
        );
        assert_eq!(
            decide("excel pivot tables", true, installed(&apps, &[])),
            search("excel pivot tables")
        );
    }

    #[test]
    fn a_picked_suggestion_searches_even_when_it_names_an_app() {
        let apps = excel();
        assert_eq!(
            decide("excel", false, installed(&apps, &[])),
            search("excel")
        );
    }

    #[test]
    fn a_learned_override_keeps_the_text_on_the_web() {
        static OVERRIDES: std::sync::LazyLock<Vec<String>> =
            std::sync::LazyLock::new(|| vec!["excel".to_string()]);
        let apps = excel();
        assert_eq!(
            decide("excel", true, installed(&apps, &OVERRIDES)),
            search("excel")
        );
    }

    #[test]
    fn a_settings_word_opens_the_page_unless_sent_to_the_web_before() {
        let decide_with = |text: &str, apps: &[App], overrides: &[String]| {
            decide(text, true, installed(apps, overrides))
        };
        let page = Decision::OpenSetting {
            setting_id: "win.display".into(),
        };
        assert_eq!(decide_with("display settings", &[], &[]), page);
        assert_eq!(decide_with("settings display", &[], &[]), page);
        assert_eq!(decide_with("displays prefs", &[], &[]), page);
        // Picking the web row once keeps this text on the web.
        let sent = ["display settings".to_string()];
        assert_eq!(
            decide_with("display settings", &[], &sent),
            search("display settings")
        );
        assert_eq!(
            decide_with("iphone display settings not working", &[], &[]),
            search("iphone display settings not working")
        );
        // An exact page name without a settings word still searches.
        assert_eq!(decide_with("display", &[], &[]), search("display"));
        // An app named exactly the text wins.
        let named = [App {
            id: "ds".into(),
            name: "Display Settings".into(),
        }];
        assert_eq!(
            decide_with("display settings", &named, &[]),
            Decision::Launch {
                app_id: "ds".into()
            }
        );
    }

    #[test]
    fn a_bare_settings_word_opens_the_system_settings_app() {
        let system = [App {
            id: "system-settings".into(),
            name: if cfg!(windows) {
                "Settings"
            } else {
                "System Settings"
            }
            .into(),
        }];
        assert_eq!(
            decide("settings", true, installed(&system, &[])),
            Decision::Launch {
                app_id: "system-settings".into()
            }
        );
        assert_eq!(
            decide("settings", true, none()),
            Decision::OpenSetting {
                setting_id: "zephyr.settings".into()
            }
        );
    }

    #[test]
    fn the_app_scope_launches_the_best_match_or_says_none() {
        let apps = excel();
        let catalog = installed(&apps, &[]);
        assert_eq!(
            decide("!app xl", true, catalog),
            Decision::Rejected {
                message: "No app matches xl".into()
            }
        );
        assert_eq!(
            decide("cel !app", true, catalog),
            Decision::Launch {
                app_id: "excel-id".into()
            }
        );
        assert_eq!(decide("!app", true, catalog), Decision::Empty);
    }

    #[test]
    fn the_settings_scope_opens_the_best_match_and_free_text_never_does() {
        let decide_text = |text: &str| decide(text, true, none());
        assert_eq!(
            decide_text("!set dark mode"),
            Decision::OpenSetting {
                setting_id: "win.colors".into()
            }
        );
        assert_eq!(
            decide_text("display !settings"),
            Decision::OpenSetting {
                setting_id: "win.display".into()
            }
        );
        assert_eq!(
            decide_text("!set wallpaper"),
            Decision::Rejected {
                message: "No setting matches wallpaper".into()
            }
        );
        assert_eq!(decide_text("!set"), Decision::Empty);
        // Unscoped, Enter keeps searching even when the text names a setting exactly.
        assert_eq!(decide_text("display"), search("display"));
        assert_eq!(decide_text("dark mode"), search("dark mode"));
    }

    #[test]
    fn the_file_scope_opens_the_best_match_and_free_text_never_does() {
        let root = std::env::temp_dir().join("zephyr-query-files");
        let files = FileIndex::with_paths(
            vec![root.clone()],
            &[(
                &*format!("Documents{}budget.xlsx", std::path::MAIN_SEPARATOR),
                false,
            )],
        );
        let catalog = Catalog {
            files: &files,
            ..none()
        };
        let decide_text = |text: &str| decide(text, true, catalog);
        let expected = root
            .join("Documents")
            .join("budget.xlsx")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            decide_text("!f budget"),
            Decision::OpenFile {
                path: expected.clone()
            }
        );
        assert_eq!(
            decide_text("budg !files"),
            Decision::OpenFile { path: expected }
        );
        assert_eq!(
            decide_text("!f taxes"),
            Decision::Rejected {
                message: "No file matches taxes".into()
            }
        );
        assert_eq!(decide_text("!f"), Decision::Empty);
        assert_eq!(decide_text("budget"), search("budget"));
    }

    #[test]
    fn only_scope_words_are_bangs() {
        assert_eq!(
            parse_input("crispr !pm"),
            Parsed {
                query: "crispr !pm".into(),
                scope: None,
            }
        );
        assert_eq!(decide("crispr !wiki", true, none()), search("crispr !wiki"));
        assert_eq!(decide("!", true, none()), search("!"));
    }

    #[test]
    fn urls_open_directly() {
        let link = |text: &str| match decide(text, true, none()) {
            Decision::Link { url } => url,
            other => panic!("expected a link, got {other:?}"),
        };
        assert_eq!(link("example.com/docs"), "https://example.com/docs");
        assert_eq!(link("https://example.com"), "https://example.com");
        assert!(link("localhost:1420").starts_with("https://localhost:1420"));
    }

    #[test]
    fn ordinary_words_and_punctuation_stay_searches() {
        assert_eq!(
            decide("C++ templates", true, none()),
            search("C++ templates")
        );
        assert!(!looks_like_url("3.14"));
        assert!(!looks_like_url("hello!world"));
        assert_eq!(decide("hello!world", true, none()), search("hello!world"));
    }

    #[test]
    fn outcome_json_is_camel_case() {
        let json = serde_json::to_value(DispatchOutcome::SettingOpened {
            setting_id: "win.colors".into(),
        })
        .unwrap();
        assert_eq!(json["kind"], "settingOpened");
        assert_eq!(json["settingId"], "win.colors");
        let opened = serde_json::to_value(DispatchOutcome::Opened).unwrap();
        assert_eq!(opened["kind"], "opened");
    }
}
