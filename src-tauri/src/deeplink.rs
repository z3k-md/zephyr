//! `zephyr://` links, for scripts, shortcuts and other apps.
//!
//! - `zephyr://open?q=text` shows the bar with `text` typed in.
//! - `zephyr://search?q=text` searches the web for `text`, or opens it when it's a web address.
//!
//! Any web page can follow these links, so they never launch apps, open files or settings, or
//! reach the shell: a search link only ever opens an http(s) page, and an open link only types
//! text in for the user to act on.

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_opener::OpenerExt;
use url::Url;

use crate::query::{self, Decision};

/// What the bar is handed: text to type in, or why a link was refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BarInput {
    pub query: String,
    pub notice: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// Show the bar with this in it.
    Show(BarInput),
    /// Open this http(s) page in the browser.
    Open(String),
}

/// Text that would open the shell, which no link may stage, even for the user to confirm.
fn opens_shell(text: &str) -> bool {
    let text = text.trim_start();
    if text.starts_with('>') {
        return true;
    }
    let first = text.split_whitespace().next().unwrap_or("");
    first.eq_ignore_ascii_case("!sh") || first.eq_ignore_ascii_case("!shell")
}

/// The page a search link opens: the text as a web address when it is an http(s) one,
/// otherwise a web search for it.
fn web_page(text: &str) -> String {
    match query::web(text) {
        Decision::Link { url } if Url::parse(&url).is_ok_and(|url| is_web(&url)) => url,
        _ => crate::web::search_url(text),
    }
}

fn is_web(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
}

pub fn parse(url: &Url) -> Option<Link> {
    if url.scheme() != "zephyr" {
        return None;
    }
    let param = |name: &str| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };
    let query = param("q").unwrap_or_default();
    // zephyr://open and zephyr:open both name the action.
    let action = url
        .host_str()
        .map(str::to_string)
        .or_else(|| {
            url.path()
                .split('/')
                .find(|part| !part.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "open".into());
    if !matches!(action.as_str(), "open" | "show" | "search" | "dispatch") {
        return None;
    }
    if opens_shell(&query) {
        return Some(Link::Show(BarInput {
            query: String::new(),
            notice: Some("Links can't open the shell".into()),
        }));
    }
    match action.as_str() {
        "search" | "dispatch" if !query.is_empty() => Some(Link::Open(web_page(&query))),
        _ => Some(Link::Show(BarInput {
            query,
            notice: None,
        })),
    }
}

pub fn handle(app: &AppHandle, url: &Url) {
    let Some(link) = parse(url) else {
        log::info!("ignored link {}", url.scheme());
        return;
    };
    match link {
        Link::Open(page) => {
            if let Err(err) = app.opener().open_url(&page, None::<&str>) {
                log::error!("couldn't open a link's page: {err}");
            }
        }
        Link::Show(input) => {
            crate::window::show_bar(app);
            if let Err(err) = app.emit("bar-input", &input) {
                log::error!("couldn't hand the link to the bar: {err}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(link: &str) -> Option<Link> {
        parse(&Url::parse(link).unwrap())
    }

    fn shown(query: &str) -> Option<Link> {
        Some(Link::Show(BarInput {
            query: query.into(),
            notice: None,
        }))
    }

    #[test]
    fn reads_each_action() {
        assert_eq!(link("zephyr://open?q=hello%20there"), shown("hello there"));
        assert_eq!(
            link("zephyr://search?q=crispr"),
            Some(Link::Open("https://www.google.com/search?q=crispr".into()))
        );
        assert_eq!(link("zephyr://"), shown(""));
        assert_eq!(link("zephyr:open?q=x"), shown("x"));
        assert_eq!(link("zephyr://search"), shown(""));
        assert_eq!(link("zephyr://delete?q=x"), None);
        assert_eq!(link("https://open?q=x"), None);
    }

    #[test]
    fn search_opens_web_addresses() {
        assert_eq!(
            link("zephyr://search?q=example.com%2Fdocs"),
            Some(Link::Open("https://example.com/docs".into()))
        );
        assert_eq!(
            link("zephyr://dispatch?q=http%3A%2F%2Flocalhost%3A3000"),
            Some(Link::Open("http://localhost:3000".into()))
        );
    }

    #[test]
    fn search_never_launches_or_opens_files() {
        // Scoped searches and app names would launch or open the top match from the bar;
        // from a link they're only ever searched for.
        for text in [
            "f setup.exe",
            "!f setup",
            "app calculator",
            "!app terminal",
            "settings",
        ] {
            let Some(Link::Open(page)) =
                link(&format!("zephyr://search?q={}", urlencoding::encode(text)))
            else {
                panic!("{text} should open a page");
            };
            assert!(
                page.starts_with("https://www.google.com/search?q="),
                "{page}"
            );
        }
        for text in [
            "file:///C:/Windows/system32/calc.exe",
            "javascript:alert(1).x",
        ] {
            let Some(Link::Open(page)) =
                link(&format!("zephyr://search?q={}", urlencoding::encode(text)))
            else {
                panic!("{text} should open a page");
            };
            assert!(Url::parse(&page).is_ok_and(|url| is_web(&url)), "{page}");
        }
    }

    #[test]
    fn refuses_the_shell() {
        let refused = Some(Link::Show(BarInput {
            query: String::new(),
            notice: Some("Links can't open the shell".into()),
        }));
        for link_text in [
            "zephyr://open?q=%3Erm%20-rf%20~",
            "zephyr://open?q=%20%20%3E%20whoami",
            "zephyr://search?q=%3Ecalc",
            "zephyr://open?q=!sh%20whoami",
            "zephyr://open?q=!SHELL",
        ] {
            assert_eq!(link(link_text), refused, "{link_text}");
        }
        assert_eq!(link("zephyr://open?q=!shout"), shown("!shout"));
        assert_eq!(link("zephyr://open?q=a%20%3E%20b"), shown("a > b"));
    }
}
