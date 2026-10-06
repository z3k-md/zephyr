//! `zephyr://` links, for scripts, shortcuts and other apps.
//!
//! - `zephyr://open?q=text` shows the bar with `text` typed in.
//! - `zephyr://dispatch?d=pubmed&q=text` sends `text` to a destination (by id or trigger);
//!   without `d` it does what Enter would.
//! - `zephyr://ask?q=question` asks AI.

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BarInput {
    pub query: String,
    pub destination: Option<String>,
    pub run: bool,
}

pub fn parse(url: &Url) -> Option<BarInput> {
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
    match action.as_str() {
        "open" | "show" => Some(BarInput {
            query,
            destination: None,
            run: false,
        }),
        "dispatch" | "search" => Some(BarInput {
            run: !query.is_empty(),
            query,
            destination: param("d"),
        }),
        "ask" => Some(BarInput {
            run: !query.is_empty(),
            query,
            destination: Some("ai".into()),
        }),
        _ => None,
    }
}

pub fn handle(app: &AppHandle, url: &Url) {
    let Some(input) = parse(url) else {
        log::info!("ignored link {}", url.scheme());
        return;
    };
    crate::window::show_bar(app);
    if let Err(err) = app.emit("bar-input", &input) {
        log::error!("couldn't hand the link to the bar: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(link: &str) -> Option<BarInput> {
        parse(&Url::parse(link).unwrap())
    }

    #[test]
    fn reads_each_action() {
        assert_eq!(
            input("zephyr://open?q=hello%20there"),
            Some(BarInput {
                query: "hello there".into(),
                destination: None,
                run: false
            })
        );
        assert_eq!(
            input("zephyr://dispatch?d=pubmed&q=crispr"),
            Some(BarInput {
                query: "crispr".into(),
                destination: Some("pubmed".into()),
                run: true
            })
        );
        assert_eq!(
            input("zephyr://ask?q=why%3F").map(|input| input.destination),
            Some(Some("ai".into()))
        );
        assert_eq!(input("zephyr://").map(|input| input.run), Some(false));
        assert_eq!(input("zephyr://delete?q=x"), None);
        assert_eq!(input("https://open?q=x"), None);
    }
}
