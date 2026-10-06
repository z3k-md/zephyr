//! Destination templates: a URL, app deeplink or file path with placeholders.
//!
//! `{query}` is the typed text. `{argument name=... default=...}` is also the typed text,
//! falling back to its default when nothing was typed; a second, differently named argument
//! always takes its default. `{clipboard}` is the clipboard's text and `{date offset=+1d
//! format=%Y-%m-%d}` the local date. Any placeholder can end in pipes: `| trim`,
//! `| lowercase`, `| uppercase`, `| percent-encode` or `| raw`. Values are percent-encoded in
//! URLs and deeplinks unless `| raw` says otherwise, and left as typed in file paths.

use chrono::{DateTime, Duration, Local};

pub struct Context<'a> {
    pub query: &'a str,
    pub now: DateTime<Local>,
    pub clipboard: &'a dyn Fn() -> Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Text(String),
    Slot(Slot),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Slot {
    kind: SlotKind,
    filters: Vec<Filter>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SlotKind {
    Query,
    Argument { name: String, default: String },
    Clipboard,
    Date { offset: Duration, format: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Filter {
    Trim,
    Lowercase,
    Uppercase,
    Encode,
    Raw,
}

/// Checks a template without rendering it.
pub fn validate(template: &str) -> Result<(), String> {
    if template.trim().is_empty() {
        return Err("A destination needs a URL, deeplink or file path".into());
    }
    if !is_path(template) && scheme(template).is_none() {
        return Err(
            "Start the template with a scheme like https:// or obsidian://, or a file path".into(),
        );
    }
    parse(template).map(|_| ())
}

/// Whether the typed text goes anywhere. A template without it is a fixed link that opens
/// as soon as its bang is typed.
pub fn uses_input(template: &str) -> bool {
    parse(template).is_ok_and(|parts| {
        parts.iter().any(|part| {
            matches!(
                part,
                Part::Slot(Slot {
                    kind: SlotKind::Query | SlotKind::Argument { .. },
                    ..
                })
            )
        })
    })
}

/// File paths open with the system's default app; everything else opens as a URL.
pub fn is_path(target: &str) -> bool {
    let bytes = target.as_bytes();
    target.starts_with('/')
        || target.starts_with("~/")
        || target.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'\\' || bytes[2] == b'/'))
}

fn scheme(template: &str) -> Option<&str> {
    let (scheme, _) = template.split_once(':')?;
    let mut chars = scheme.chars();
    let first = chars.next()?;
    (first.is_ascii_alphabetic()
        && scheme.len() > 1
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.')))
    .then_some(scheme)
}

pub fn render(template: &str, context: &Context<'_>) -> Result<String, String> {
    validate(template)?;
    let parts = parse(template)?;
    let path = is_path(template);
    let mut first_argument: Option<String> = None;
    let mut clipboard: Option<Option<String>> = None;
    let mut out = String::new();

    for part in parts {
        let slot = match part {
            Part::Text(text) => {
                out.push_str(&text);
                continue;
            }
            Part::Slot(slot) => slot,
        };
        let mut value = match &slot.kind {
            SlotKind::Query => context.query.to_string(),
            SlotKind::Argument { name, default } => {
                let primary = first_argument.get_or_insert_with(|| name.clone()) == name;
                if primary && !context.query.trim().is_empty() {
                    context.query.to_string()
                } else {
                    default.clone()
                }
            }
            SlotKind::Clipboard => clipboard
                .get_or_insert_with(|| (context.clipboard)())
                .clone()
                .unwrap_or_default(),
            SlotKind::Date { offset, format } => (context.now + *offset).format(format).to_string(),
        };
        let mut encode = !path;
        for filter in &slot.filters {
            match filter {
                Filter::Trim => value = value.trim().to_string(),
                Filter::Lowercase => value = value.to_lowercase(),
                Filter::Uppercase => value = value.to_uppercase(),
                Filter::Encode => encode = true,
                Filter::Raw => encode = false,
            }
        }
        if encode {
            out.push_str(&urlencoding::encode(&value));
        } else {
            out.push_str(&value);
        }
    }

    if let Some(rest) = out.strip_prefix("~/")
        && let Some(home) = home_dir()
    {
        out = format!("{}/{rest}", home.trim_end_matches(['/', '\\']));
    }
    Ok(out)
}

fn home_dir() -> Option<String> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
}

fn parse(template: &str) -> Result<Vec<Part>, String> {
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut chars = template.char_indices().peekable();

    while let Some((start, ch)) = chars.next() {
        if ch != '{' {
            if ch == '}' {
                return Err("A } in the template has no matching {".into());
            }
            text.push(ch);
            continue;
        }
        let mut body = String::new();
        let mut quoted = false;
        let mut closed = false;
        for (_, inner) in chars.by_ref() {
            match inner {
                '"' => {
                    quoted = !quoted;
                    body.push(inner);
                }
                '}' if !quoted => {
                    closed = true;
                    break;
                }
                _ => body.push(inner),
            }
        }
        if !closed {
            return Err(format!(
                "The placeholder starting at character {} isn't closed with }}",
                start + 1
            ));
        }
        if !text.is_empty() {
            parts.push(Part::Text(std::mem::take(&mut text)));
        }
        parts.push(Part::Slot(parse_slot(&body)?));
    }
    if !text.is_empty() {
        parts.push(Part::Text(text));
    }
    Ok(parts)
}

fn parse_slot(body: &str) -> Result<Slot, String> {
    let mut sections = split_unquoted(body, '|').into_iter();
    let head = sections.next().unwrap_or_default();
    let mut words = split_words(&head)?.into_iter();
    let name = words.next().unwrap_or_default();
    let mut attrs = Vec::new();
    for word in words {
        let Some((key, value)) = word.split_once('=') else {
            return Err(format!(
                "In {{{name}}}, {word} needs a value like {word}=..."
            ));
        };
        attrs.push((key.to_string(), unquote(value)));
    }
    let attr = |key: &str| {
        attrs
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value.clone())
    };
    let allow = |keys: &[&str]| -> Result<(), String> {
        match attrs.iter().find(|(key, _)| !keys.contains(&key.as_str())) {
            Some((key, _)) => Err(format!("{{{name}}} doesn't take {key}=")),
            None => Ok(()),
        }
    };

    let kind = match name.as_str() {
        "query" => {
            allow(&[])?;
            SlotKind::Query
        }
        "argument" => {
            allow(&["name", "default", "options"])?;
            SlotKind::Argument {
                name: attr("name").unwrap_or_default(),
                default: attr("default").unwrap_or_default(),
            }
        }
        "clipboard" => {
            allow(&[])?;
            SlotKind::Clipboard
        }
        "date" => {
            allow(&["offset", "format"])?;
            let format = attr("format").unwrap_or_else(|| "%Y-%m-%d".into());
            check_format(&format)?;
            SlotKind::Date {
                offset: attr("offset")
                    .map_or(Ok(Duration::zero()), |offset| parse_offset(&offset))?,
                format,
            }
        }
        "" => return Err("A placeholder is empty: write {query}".into()),
        other => {
            return Err(format!(
                "{{{other}}} isn't a placeholder. Use query, argument, clipboard or date."
            ));
        }
    };

    let mut filters = Vec::new();
    for filter in sections {
        filters.push(match filter.trim() {
            "trim" => Filter::Trim,
            "lowercase" => Filter::Lowercase,
            "uppercase" => Filter::Uppercase,
            "percent-encode" | "encode" => Filter::Encode,
            "raw" => Filter::Raw,
            other => {
                return Err(format!(
                    "| {other} isn't a filter. Use trim, lowercase, uppercase, percent-encode or raw."
                ));
            }
        });
    }
    Ok(Slot { kind, filters })
}

fn split_unquoted(body: &str, separator: char) -> Vec<String> {
    let mut sections = vec![String::new()];
    let mut quoted = false;
    for ch in body.chars() {
        if ch == '"' {
            quoted = !quoted;
        }
        if ch == separator && !quoted {
            sections.push(String::new());
        } else if let Some(last) = sections.last_mut() {
            last.push(ch);
        }
    }
    sections
}

fn split_words(head: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    for ch in head.trim().chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                word.push(ch);
            }
            ch if ch.is_whitespace() && !quoted => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            _ => word.push(ch),
        }
    }
    if quoted {
        return Err("A quote in a placeholder isn't closed".into());
    }
    if !word.is_empty() {
        words.push(word);
    }
    Ok(words)
}

fn unquote(value: &str) -> String {
    value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(value)
        .to_string()
}

/// `+1d`, `-2w`, `3h` or `-30m`.
fn parse_offset(offset: &str) -> Result<Duration, String> {
    let invalid = || format!("offset={offset} should look like +1d, -2w, 3h or -30m");
    let (sign, rest) = match offset.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, offset.strip_prefix('+').unwrap_or(offset)),
    };
    let unit = rest.chars().last().ok_or_else(invalid)?;
    let amount: i64 = rest[..rest.len() - unit.len_utf8()]
        .parse()
        .map_err(|_| invalid())?;
    let amount = sign * amount;
    match unit {
        'm' => Ok(Duration::minutes(amount)),
        'h' => Ok(Duration::hours(amount)),
        'd' => Ok(Duration::days(amount)),
        'w' => Ok(Duration::weeks(amount)),
        _ => Err(invalid()),
    }
}

fn check_format(format: &str) -> Result<(), String> {
    use chrono::format::{Item, StrftimeItems};
    if StrftimeItems::new(format).any(|item| matches!(item, Item::Error)) {
        return Err(format!("format={format} isn't a valid date format"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn render_with(template: &str, query: &str, clipboard: Option<&str>) -> Result<String, String> {
        let clip = clipboard.map(str::to_string);
        let read = move || clip.clone();
        render(
            template,
            &Context {
                query,
                now: Local.with_ymd_and_hms(2026, 10, 5, 12, 0, 0).unwrap(),
                clipboard: &read,
            },
        )
    }

    #[test]
    fn encodes_the_query_in_urls() {
        assert_eq!(
            render_with("https://example.com/search?q={query}", "a b&c", None).unwrap(),
            "https://example.com/search?q=a%20b%26c"
        );
    }

    #[test]
    fn applies_pipes_in_order() {
        assert_eq!(
            render_with(
                "https://x.dev/{query | trim | lowercase}",
                "  Hello World ",
                None
            )
            .unwrap(),
            "https://x.dev/hello%20world"
        );
        assert_eq!(
            render_with("https://x.dev/#{query | raw}", "a/b c", None).unwrap(),
            "https://x.dev/#a/b c"
        );
    }

    #[test]
    fn arguments_fall_back_to_their_default() {
        let template = r#"https://t.co/?lang={argument name="lang" default="en"}&to={argument name="to" default="de"}"#;
        assert_eq!(
            render_with(template, "fr", None).unwrap(),
            "https://t.co/?lang=fr&to=de"
        );
        assert_eq!(
            render_with(template, "", None).unwrap(),
            "https://t.co/?lang=en&to=de"
        );
    }

    #[test]
    fn fills_dates_and_clipboard() {
        assert_eq!(
            render_with("https://cal.dev/{date offset=+1d}", "", None).unwrap(),
            "https://cal.dev/2026-10-06"
        );
        assert_eq!(
            render_with(r#"https://cal.dev/{date format="%d %b"}"#, "", None).unwrap(),
            "https://cal.dev/05%20Oct"
        );
        assert_eq!(
            render_with("https://x.dev/?u={clipboard}", "", Some("https://a.b/c")).unwrap(),
            "https://x.dev/?u=https%3A%2F%2Fa.b%2Fc"
        );
    }

    #[test]
    fn leaves_paths_and_deeplinks_usable() {
        assert!(validate("obsidian://open?vault=Notes&file={query}").is_ok());
        assert!(validate("/Applications/Notes.app").is_ok());
        assert!(validate(r"C:\Users\me\notes.txt").is_ok());
        assert_eq!(
            render_with("/tmp/{query}.md", "my note", None).unwrap(),
            "/tmp/my note.md"
        );
        assert!(!uses_input("https://calendar.google.com"));
        assert!(uses_input("https://x.dev/{argument default=a}"));
    }

    #[test]
    fn explains_bad_templates() {
        assert!(validate("example.com/{query}").is_err());
        assert!(validate("https://x.dev/{query").is_err());
        assert!(validate("https://x.dev/{nope}").is_err());
        assert!(validate("https://x.dev/{query | shout}").is_err());
        assert!(validate("https://x.dev/{date offset=soon}").is_err());
        assert!(validate("https://x.dev/{query name=x}").is_err());
        assert!(validate("").is_err());
    }
}
