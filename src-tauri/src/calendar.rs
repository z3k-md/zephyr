//! Agendas from iCalendar feeds (a secret Google, Outlook or iCloud link).
//!
//! The link stays in the keychain. Settings keep the name, color and order. Events are
//! held in memory only, from local midnight a day ago through the end of the day two days
//! ahead, so today and tomorrow are complete. A background thread refreshes every five
//! minutes, and again when the bar is summoned if the copy is older than that. A feed that
//! fails waits longer each time, up to five minutes.

use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

use chrono::{
    DateTime, Datelike, Days, Duration as ChronoDuration, LocalResult, NaiveDate, NaiveDateTime,
    NaiveTime, TimeZone, Timelike, Utc, Weekday,
};
use chrono_tz::Tz;
use futures_util::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;
use crate::util::now_ms;

const FRESH_MS: u64 = 5 * 60 * 1000;
/// A feed larger than this is refused. Checked on Content-Length, then again while streaming.
const MAX_FEED_BYTES: usize = 8 * 1024 * 1024;
/// The keychain service. The account is the feed's id.
pub const SECRET_SERVICE: &str = "com.zephyr.app.calendar";

const COLORS: &[&str] = &[
    "#5b8def", "#3cba8b", "#e0a045", "#d46a6a", "#9b7ed9", "#4aa8b5",
];

/// A feed as Settings stores it. The link is not here; it lives in the keychain.
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Feed {
    pub id: String,
    pub name: String,
    pub color: String,
    pub order: u32,
}

/// One occurrence, already limited to the window the bar shows.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    pub feed_id: String,
    pub color: String,
    pub title: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub all_day: bool,
    /// Inclusive first day, `YYYY-MM-DD`, for an all-day event.
    pub start_date: String,
    /// Exclusive last day, `YYYY-MM-DD`, for an all-day event.
    pub end_date: String,
    pub location: String,
    pub url: String,
    pub provider: String,
}

/// A feed plus when it last updated, or the error, for Settings.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FeedStatus {
    pub id: String,
    pub name: String,
    pub color: String,
    pub order: u32,
    pub updated_at: u64,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Agenda {
    pub events: Vec<Event>,
    pub feeds: Vec<FeedStatus>,
}

struct FeedMem {
    id: String,
    name: String,
    color: String,
    order: u32,
    updated_at: u64,
    error: String,
    failures: u32,
    tried_at: u64,
    events: Vec<Event>,
}

struct Shared {
    feeds: Vec<FeedMem>,
    /// Bumped when settings change, so a refresh that lands mid-fetch is not missed.
    generation: u64,
    /// The bar was summoned.
    summon: bool,
}

static SHARED: OnceLock<(Mutex<Shared>, Condvar)> = OnceLock::new();

/// Starts the thread that keeps feeds fresh. Events are never written to disk.
pub fn start(app: &AppHandle) {
    let shared = Shared {
        feeds: Vec::new(),
        generation: 0,
        summon: false,
    };
    if SHARED.set((Mutex::new(shared), Condvar::new())).is_err() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || run(app));
}

/// The events and feed statuses held in memory.
pub fn agenda() -> Agenda {
    let Some((shared, _)) = SHARED.get() else {
        return Agenda::default();
    };
    let Ok(guard) = shared.lock() else {
        return Agenda::default();
    };
    snapshot(&guard.feeds)
}

/// Settings changed: read the feed list again and fetch anything new.
pub fn refresh() {
    wake(|shared| shared.generation = shared.generation.wrapping_add(1));
}

/// The bar was summoned. A feed older than five minutes is fetched, unless it's waiting
/// out a failure.
pub fn on_summon() {
    wake(|shared| shared.summon = true);
}

fn wake(mark: impl FnOnce(&mut Shared)) {
    let Some((shared, wake)) = SHARED.get() else {
        return;
    };
    if let Ok(mut guard) = shared.lock() {
        mark(&mut guard);
        wake.notify_one();
    }
}

fn run(app: AppHandle) {
    let Some((shared, wake)) = SHARED.get() else {
        return;
    };
    loop {
        let seen = {
            let Ok(guard) = shared.lock() else { return };
            guard.generation
        };
        let settings = app
            .state::<AppState>()
            .snapshot()
            .map(|persisted| persisted.calendar_feeds.clone())
            .unwrap_or_default();
        let now = now_ms();
        let (due, changed) = {
            let Ok(mut guard) = shared.lock() else { return };
            guard.summon = false;
            let changed = reconcile(&mut guard.feeds, &settings);
            let due = guard
                .feeds
                .iter()
                .filter(|feed| should_fetch(feed, now))
                .map(|feed| feed.id.clone())
                .collect::<Vec<_>>();
            (due, changed)
        };
        if !due.is_empty() {
            let mut results = Vec::new();
            for id in &due {
                // A panic in one feed is that feed's error. The thread keeps refreshing the rest.
                let result = std::panic::catch_unwind(|| fetch_feed(id));
                let result = match result {
                    Ok(result) => result,
                    Err(_) => {
                        log::warn!("calendar feed panicked");
                        Err("Couldn't read this calendar".into())
                    }
                };
                results.push((id.clone(), result));
            }
            if let Ok(mut guard) = shared.lock() {
                let fetched = now_ms();
                for (id, result) in results {
                    apply_fetch(&mut guard.feeds, &id, result, fetched);
                }
            }
        }
        if !due.is_empty() || changed {
            let _ = app.emit("calendar", &agenda());
        }
        let Ok(guard) = shared.lock() else { return };
        if guard.generation != seen || guard.summon {
            continue;
        }
        let _ = wake.wait_timeout(guard, Duration::from_secs(15));
    }
}

/// A feed is fetched when it has never loaded, is older than five minutes, or last failed
/// and its backoff has passed. A summon does not skip the backoff.
fn should_fetch(feed: &FeedMem, now: u64) -> bool {
    if feed.failures > 0 && now.saturating_sub(feed.tried_at) < backoff_ms(feed.failures) {
        return false;
    }
    feed.updated_at == 0
        || now.saturating_sub(feed.updated_at) >= FRESH_MS
        || !feed.error.is_empty()
}

fn backoff_ms(failures: u32) -> u64 {
    match failures {
        0 | 1 => 30_000,
        2 => 60_000,
        3 => 120_000,
        _ => FRESH_MS,
    }
}

/// Drops removed feeds, copies names and colors, and adds feeds that are new.
/// True when the list the bar shows changed.
fn reconcile(mem: &mut Vec<FeedMem>, settings: &[Feed]) -> bool {
    let mut changed = false;
    let before = mem.len();
    mem.retain(|feed| settings.iter().any(|saved| saved.id == feed.id));
    changed |= mem.len() != before;
    for saved in settings {
        if let Some(feed) = mem.iter_mut().find(|feed| feed.id == saved.id) {
            if feed.name != saved.name || feed.color != saved.color || feed.order != saved.order {
                changed = true;
            }
            feed.name.clone_from(&saved.name);
            if feed.color != saved.color {
                feed.color.clone_from(&saved.color);
                for event in &mut feed.events {
                    event.color.clone_from(&saved.color);
                }
            }
            feed.order = saved.order;
        } else {
            changed = true;
            mem.push(FeedMem {
                id: saved.id.clone(),
                name: saved.name.clone(),
                color: saved.color.clone(),
                order: saved.order,
                updated_at: 0,
                error: String::new(),
                failures: 0,
                tried_at: 0,
                events: Vec::new(),
            });
        }
    }
    changed
}

fn apply_fetch(feeds: &mut [FeedMem], id: &str, result: Result<Vec<Event>, String>, fetched: u64) {
    let Some(feed) = feeds.iter_mut().find(|feed| feed.id == id) else {
        return;
    };
    feed.tried_at = fetched;
    match result {
        Ok(mut events) => {
            for event in &mut events {
                event.feed_id.clone_from(&feed.id);
                event.color.clone_from(&feed.color);
            }
            feed.events = events;
            feed.updated_at = fetched;
            feed.error.clear();
            feed.failures = 0;
        }
        Err(err) => {
            log::warn!("calendar {}: {err}", feed.name);
            feed.failures = feed.failures.saturating_add(1);
            feed.error = err;
        }
    }
}

fn snapshot(feeds: &[FeedMem]) -> Agenda {
    let mut feeds: Vec<&FeedMem> = feeds.iter().collect();
    feeds.sort_by_key(|feed| (feed.order, feed.name.as_str()));
    let mut events = Vec::new();
    let mut status = Vec::new();
    for feed in feeds {
        events.extend(feed.events.iter().cloned());
        status.push(FeedStatus {
            id: feed.id.clone(),
            name: feed.name.clone(),
            color: feed.color.clone(),
            order: feed.order,
            updated_at: feed.updated_at,
            error: feed.error.clone(),
        });
    }
    Agenda {
        events,
        feeds: status,
    }
}

fn fetch_feed(id: &str) -> Result<Vec<Event>, String> {
    let url = crate::secrets::get(SECRET_SERVICE, id)
        .map_err(|_| "Couldn't read the saved link".to_string())?
        .filter(|url| !url.is_empty())
        .ok_or_else(|| "The calendar link is missing".to_string())?;
    let owner = owner_email(&url);
    let url =
        normalize_feed_url(url.trim()).ok_or_else(|| "The calendar link is missing".to_string())?;
    let ics = tauri::async_runtime::block_on(fetch_ics(&url))?;
    let (now, zone) = local_clock();
    let mut events = parse_events(&ics, owner.as_deref(), now, zone)?;
    if events.len() > 300 {
        events.truncate(300);
    }
    Ok(events)
}

fn local_clock() -> (DateTime<Utc>, Tz) {
    (Utc::now(), local_zone())
}

/// The machine's zone. Floating times use the offset in effect on their own date.
fn local_zone() -> Tz {
    iana_time_zone::get_timezone()
        .ok()
        .and_then(|name| name.parse().ok())
        .unwrap_or(chrono_tz::UTC)
}

async fn fetch_ics(url: &str) -> Result<String, String> {
    let response = crate::util::http()
        .get(url)
        .timeout(Duration::from_secs(20))
        .header("accept", "text/calendar, text/plain, */*")
        .send()
        .await
        .map_err(|_| "Couldn't reach the calendar".to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("The calendar replied {status}"));
    }
    if response
        .content_length()
        .is_some_and(|len| len > MAX_FEED_BYTES as u64)
    {
        return Err("The calendar is too large".into());
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "Couldn't read the calendar".to_string())?;
        if body.len().saturating_add(chunk.len()) > MAX_FEED_BYTES {
            return Err("The calendar is too large".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// The next palette color that no feed is using.
pub fn next_color(feeds: &[Feed]) -> String {
    COLORS
        .iter()
        .copied()
        .find(|color| feeds.iter().all(|feed| feed.color != *color))
        .unwrap_or(COLORS[feeds.len() % COLORS.len()])
        .to_string()
}

/// A name and an http(s) link, trimmed. `webcal://` and `webcals://` are stored as `https://`.
/// The link is a secret and is not kept in settings.
pub fn validate_link(name: &str, url: &str) -> Result<(String, String), String> {
    let name = validate_name(name)?;
    let url = url.trim();
    if url.chars().count() > 2000 {
        return Err("That link is too long".into());
    }
    if url.chars().any(char::is_whitespace) {
        return Err("That link has a space in it".into());
    }
    let url = normalize_feed_url(url).ok_or("The link has to start with https://")?;
    Ok((name, url))
}

/// `webcal://` and `webcals://` become `https://`. Any other non-http scheme is refused.
fn normalize_feed_url(url: &str) -> Option<String> {
    if let Some(rest) = strip_prefix_ignore_ascii(url, "webcal://")
        .or_else(|| strip_prefix_ignore_ascii(url, "webcals://"))
    {
        return Some(format!("https://{rest}"));
    }
    if strip_prefix_ignore_ascii(url, "https://").is_some()
        || strip_prefix_ignore_ascii(url, "http://").is_some()
    {
        return Some(url.to_string());
    }
    None
}

fn strip_prefix_ignore_ascii<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .and_then(|_| text.get(prefix.len()..))
}

pub fn validate_name(name: &str) -> Result<String, String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err("Name the calendar".into());
    }
    if name.chars().count() > 80 {
        return Err("That name is too long".into());
    }
    Ok(name)
}

/// Swaps a feed with the one above or below it. The ends stay put.
pub fn move_feed(feeds: &mut [Feed], id: &str, up: bool) -> Result<(), String> {
    let mut order: Vec<usize> = (0..feeds.len()).collect();
    order.sort_by_key(|index| (feeds[*index].order, *index));
    let place = order
        .iter()
        .position(|index| feeds[*index].id == id)
        .ok_or("That calendar is gone")?;
    let Some(other) = (if up {
        place.checked_sub(1)
    } else {
        place.checked_add(1).filter(|index| *index < order.len())
    }) else {
        return Ok(());
    };
    let a = order[place];
    let b = order[other];
    let left = feeds[a].order;
    let right = feeds[b].order;
    if left == right {
        feeds[a].order = if up {
            right.saturating_sub(1)
        } else {
            right.saturating_add(1)
        };
    } else {
        feeds[a].order = right;
        feeds[b].order = left;
    }
    Ok(())
}

// --- iCalendar ----------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stamp {
    Instant(DateTime<Utc>),
    Date(NaiveDate),
}

#[derive(Clone, Copy, Debug)]
enum Anchor {
    Zoned(Tz),
    Utc,
    Floating,
}

#[derive(Clone, Debug)]
struct ByDay {
    nth: Option<i8>,
    weekday: Weekday,
}

#[derive(Clone, Copy, Debug)]
enum Freq {
    Minutely,
    Hourly,
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Clone, Debug)]
struct Rule {
    freq: Freq,
    interval: i64,
    count: Option<u32>,
    until: Option<Stamp>,
    by_day: Vec<ByDay>,
    by_month_day: Vec<i32>,
    by_month: Vec<u32>,
    by_year_day: Vec<i32>,
    by_week_no: Vec<i32>,
    by_hour: Vec<u32>,
    by_minute: Vec<u32>,
    by_set_pos: Vec<i32>,
    wkst: Weekday,
    /// A part this parser does not expand. The series then keeps its first time, plus any RDATE.
    unhonored: Option<String>,
}

/// Weeks and days are civil days. Hours, minutes and seconds are exact.
#[derive(Clone, Copy, Debug)]
struct CalDuration {
    days: i64,
    seconds: i64,
}

#[derive(Clone, Debug)]
struct RawEvent {
    uid: String,
    summary: String,
    location: String,
    description: String,
    url: String,
    conference: String,
    status: String,
    declined: bool,
    start: Stamp,
    end: Option<Stamp>,
    duration: Option<CalDuration>,
    anchor: Anchor,
    rule: Option<Rule>,
    exdates: Vec<Stamp>,
    rdates: Vec<Stamp>,
    recurrence_id: Option<Stamp>,
}

struct Window {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    start_date: NaiveDate,
    end_date: NaiveDate,
}

/// Events in the window around `now`. `owner`, when known, drops events that address declined.
fn parse_events(
    ics: &str,
    owner: Option<&str>,
    now: DateTime<Utc>,
    zone: Tz,
) -> Result<Vec<Event>, String> {
    if !looks_like_ics(ics) {
        return Err("That link isn't an iCal feed".into());
    }
    let window = window_for(now, zone);
    let mut events = Vec::new();
    for group in group_events(read_events(ics, owner, zone)) {
        if group.master.declined || is_cancelled(&group.master.status) {
            continue;
        }
        events.extend(expand_group(&group.master, &group.overrides, &window, zone));
    }
    events.sort_by(|a, b| a.start_ms.cmp(&b.start_ms).then(a.title.cmp(&b.title)));
    Ok(events)
}

fn looks_like_ics(text: &str) -> bool {
    let lower = text.trim_start().to_ascii_lowercase();
    lower.contains("begin:vcalendar") || lower.contains("begin:vevent")
}

fn window_for(now: DateTime<Utc>, zone: Tz) -> Window {
    let today = now.with_timezone(&zone).date_naive();
    let start_date = today.checked_sub_days(Days::new(1)).unwrap_or(today);
    let end_date = today.checked_add_days(Days::new(2)).unwrap_or(today);
    Window {
        start: at_local(start_date, NaiveTime::MIN, zone).unwrap_or(now),
        end: at_local(
            end_date,
            NaiveTime::from_hms_opt(23, 59, 59).unwrap_or(NaiveTime::MIN),
            zone,
        )
        .unwrap_or(now),
        start_date,
        end_date,
    }
}

fn at_local(date: NaiveDate, time: NaiveTime, zone: Tz) -> Option<DateTime<Utc>> {
    let naive = date.and_time(time);
    match zone.from_local_datetime(&naive) {
        LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => Some(dt.with_timezone(&Utc)),
        LocalResult::None => None,
    }
}

struct Group {
    master: RawEvent,
    overrides: Vec<RawEvent>,
}

fn group_events(raws: Vec<RawEvent>) -> Vec<Group> {
    let mut groups = Vec::new();
    let mut overrides = Vec::new();
    for raw in raws {
        if raw.recurrence_id.is_some() {
            overrides.push(raw);
        } else {
            groups.push(Group {
                master: raw,
                overrides: Vec::new(),
            });
        }
    }
    for extra in overrides {
        if let Some(group) = groups
            .iter_mut()
            .find(|group| group.master.uid == extra.uid)
        {
            group.overrides.push(extra);
        } else {
            let mut master = extra;
            master.rule = None;
            master.recurrence_id = None;
            groups.push(Group {
                master,
                overrides: Vec::new(),
            });
        }
    }
    groups
}

fn read_events(ics: &str, owner: Option<&str>, zone: Tz) -> Vec<RawEvent> {
    let mut events = Vec::new();
    for props in extract_vevents(ics) {
        if let Some(raw) = build_raw(&props, owner, zone) {
            events.push(raw);
        }
    }
    events
}

/// One unfolded content line: the name, its parameters, and the value.
type ContentLine = (String, Vec<(String, String)>, String);

fn extract_vevents(text: &str) -> Vec<Vec<ContentLine>> {
    let mut events = Vec::new();
    let mut current: Option<Vec<ContentLine>> = None;
    let mut nested = 0i32;
    for line in unfold(text).lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((name, params, value)) = parse_content_line(line) else {
            continue;
        };
        if name.eq_ignore_ascii_case("BEGIN") {
            if value.eq_ignore_ascii_case("VEVENT") && current.is_none() {
                current = Some(Vec::new());
            } else if current.is_some() {
                nested += 1;
            }
            continue;
        }
        if name.eq_ignore_ascii_case("END") {
            if current.is_some() && nested == 0 && value.eq_ignore_ascii_case("VEVENT") {
                if let Some(event) = current.take() {
                    events.push(event);
                }
            } else if nested > 0 {
                nested -= 1;
            }
            continue;
        }
        if nested == 0
            && let Some(event) = current.as_mut()
        {
            event.push((name, params, value));
        }
    }
    events
}

fn unfold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut lines = text.split('\n');
    if let Some(first) = lines.next() {
        out.push_str(first.trim_end_matches('\r'));
    }
    for line in lines {
        let line = line.trim_end_matches('\r');
        if line.starts_with([' ', '\t']) {
            out.push_str(line.get(1..).unwrap_or(""));
        } else {
            out.push('\n');
            out.push_str(line);
        }
    }
    out
}

fn parse_content_line(line: &str) -> Option<ContentLine> {
    let (left, value) = split_unquoted(line, ':')?;
    let mut parts = split_unquoted_all(left, ';');
    if parts.is_empty() {
        return None;
    }
    let name = parts.remove(0);
    if name.is_empty() {
        return None;
    }
    let mut params = Vec::new();
    for part in parts {
        if let Some((key, val)) = part.split_once('=') {
            params.push((key.to_string(), unquote(val)));
        }
    }
    Some((name, params, value))
}

fn split_unquoted(text: &str, sep: char) -> Option<(&str, String)> {
    let mut quote = false;
    for (index, ch) in text.char_indices() {
        match ch {
            '"' => quote = !quote,
            found if found == sep && !quote => {
                return Some((&text[..index], text[index + ch.len_utf8()..].to_string()));
            }
            _ => {}
        }
    }
    None
}

fn split_unquoted_all(text: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote = false;
    for ch in text.chars() {
        match ch {
            '"' => {
                quote = !quote;
                cur.push(ch);
            }
            found if found == sep && !quote => out.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    out.push(cur);
    out
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        value.get(1..value.len() - 1).unwrap_or(value).to_string()
    } else {
        value.to_string()
    }
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn param<'a>(params: &'a [(String, String)], name: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn build_raw(props: &[ContentLine], owner: Option<&str>, zone: Tz) -> Option<RawEvent> {
    let start_prop = props
        .iter()
        .find(|(name, _, _)| name.eq_ignore_ascii_case("DTSTART"))?;
    let anchor = anchor_of(&start_prop.1, &start_prop.2);
    let start = parse_stamp(&start_prop.2, &start_prop.1, anchor, zone)?;
    let mut raw = RawEvent {
        uid: String::new(),
        summary: String::new(),
        location: String::new(),
        description: String::new(),
        url: String::new(),
        conference: String::new(),
        status: String::new(),
        declined: false,
        start,
        end: None,
        duration: None,
        anchor,
        rule: None,
        exdates: Vec::new(),
        rdates: Vec::new(),
        recurrence_id: None,
    };
    for (name, params, value) in props {
        if name.eq_ignore_ascii_case("DTSTART") {
            continue;
        } else if name.eq_ignore_ascii_case("DTEND") {
            raw.end = parse_stamp(value, params, anchor, zone);
        } else if name.eq_ignore_ascii_case("DURATION") {
            raw.duration = parse_duration(value);
        } else if name.eq_ignore_ascii_case("UID") {
            raw.uid = value.trim().to_string();
        } else if name.eq_ignore_ascii_case("SUMMARY") && raw.summary.is_empty() {
            raw.summary = unescape(value);
        } else if name.eq_ignore_ascii_case("LOCATION") && raw.location.is_empty() {
            raw.location = unescape(value);
        } else if name.eq_ignore_ascii_case("DESCRIPTION") && raw.description.is_empty() {
            raw.description = unescape(value);
        } else if name.eq_ignore_ascii_case("URL") && raw.url.is_empty() {
            raw.url = unescape(value).trim().to_string();
        } else if name.eq_ignore_ascii_case("X-GOOGLE-CONFERENCE") && raw.conference.is_empty() {
            raw.conference = unescape(value).trim().to_string();
        } else if name.eq_ignore_ascii_case("STATUS") {
            raw.status = value.trim().to_string();
        } else if name.eq_ignore_ascii_case("RRULE") && raw.rule.is_none() {
            raw.rule = parse_rule(value, anchor, zone);
        } else if name.eq_ignore_ascii_case("EXDATE") {
            raw.exdates
                .extend(parse_stamp_list(value, params, anchor, zone));
        } else if name.eq_ignore_ascii_case("RDATE") {
            raw.rdates
                .extend(parse_stamp_list(value, params, anchor, zone));
        } else if name.eq_ignore_ascii_case("RECURRENCE-ID") {
            raw.recurrence_id = parse_stamp(value, params, anchor, zone);
        } else if name.eq_ignore_ascii_case("ATTENDEE") && attendee_declined(params, value, owner) {
            raw.declined = true;
        }
    }
    Some(raw)
}

fn anchor_of(params: &[(String, String)], value: &str) -> Anchor {
    if value.trim().ends_with(['Z', 'z']) {
        return Anchor::Utc;
    }
    if let Some(tzid) = param(params, "TZID")
        && let Some(tz) = zone_from_tzid(tzid)
    {
        return Anchor::Zoned(tz);
    }
    Anchor::Floating
}

fn attendee_declined(params: &[(String, String)], value: &str, owner: Option<&str>) -> bool {
    let Some(owner) = owner else {
        return false;
    };
    let partstat = param(params, "PARTSTAT").unwrap_or("");
    if !partstat.eq_ignore_ascii_case("DECLINED") {
        return false;
    }
    email_of(value).eq_ignore_ascii_case(owner)
}

fn email_of(value: &str) -> &str {
    let value = value.trim();
    strip_prefix_ignore_ascii(value, "mailto:")
        .unwrap_or(value)
        .trim()
}

fn is_cancelled(status: &str) -> bool {
    status.eq_ignore_ascii_case("CANCELLED")
}

fn parse_stamp_list(
    value: &str,
    params: &[(String, String)],
    anchor: Anchor,
    zone: Tz,
) -> Vec<Stamp> {
    value
        .split(',')
        .filter_map(|part| parse_stamp(part, params, anchor, zone))
        .collect()
}

fn parse_stamp(
    value: &str,
    params: &[(String, String)],
    anchor: Anchor,
    zone: Tz,
) -> Option<Stamp> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let date_only = param(params, "VALUE").is_some_and(|kind| kind.eq_ignore_ascii_case("DATE"))
        || (!value.contains('T') && value.len() >= 8);
    if date_only {
        return Some(Stamp::Date(parse_date(value)?));
    }
    let naive = parse_naive(value)?;
    if value.ends_with(['Z', 'z']) {
        return Some(Stamp::Instant(naive.and_utc()));
    }
    if let Some(tzid) = param(params, "TZID")
        && let Some(tz) = zone_from_tzid(tzid)
    {
        return Some(Stamp::Instant(from_naive(naive, Anchor::Zoned(tz), zone)));
    }
    Some(Stamp::Instant(from_naive(naive, anchor, zone)))
}

fn parse_date(value: &str) -> Option<NaiveDate> {
    let year: i32 = value.get(0..4)?.parse().ok()?;
    let month: u32 = value.get(4..6)?.parse().ok()?;
    let day: u32 = value.get(6..8)?.parse().ok()?;
    NaiveDate::from_ymd_opt(year, month, day)
}

fn parse_naive(value: &str) -> Option<NaiveDateTime> {
    let value = value.trim_end_matches(['Z', 'z']);
    let (date, time) = value.split_once('T')?;
    let date = parse_date(date)?;
    let hour: u32 = time.get(0..2)?.parse().ok()?;
    let minute: u32 = time.get(2..4)?.parse().ok()?;
    let second: u32 = time.get(4..6)?.parse().ok()?;
    date.and_hms_opt(hour, minute, second)
}

fn parse_duration(value: &str) -> Option<CalDuration> {
    let mut rest = value.trim().strip_prefix(['P', 'p'])?;
    let mut weeks = 0i64;
    let mut days = 0i64;
    let mut hours = 0i64;
    let mut minutes = 0i64;
    let mut seconds = 0i64;
    let mut time = false;
    while !rest.is_empty() {
        if rest.starts_with(['T', 't']) {
            time = true;
            rest = rest.get(1..)?;
            continue;
        }
        let digits = rest
            .find(|ch: char| !ch.is_ascii_digit())
            .unwrap_or(rest.len());
        if digits == 0 {
            return None;
        }
        let count: i64 = rest.get(..digits)?.parse().ok()?;
        let unit = rest.get(digits..)?.chars().next()?;
        rest = rest.get(digits + unit.len_utf8()..)?;
        match unit.to_ascii_uppercase() {
            'W' => weeks = count,
            'D' => days = count,
            'H' => hours = count,
            'M' if time => minutes = count,
            'M' => days = days.saturating_add(count.saturating_mul(30)),
            'S' => seconds = count,
            'Y' => days = days.saturating_add(count.saturating_mul(365)),
            _ => return None,
        }
    }
    Some(CalDuration {
        days: weeks.saturating_mul(7).saturating_add(days),
        seconds: hours
            .saturating_mul(3600)
            .saturating_add(minutes.saturating_mul(60))
            .saturating_add(seconds),
    })
}

fn zone_from_tzid(tzid: &str) -> Option<Tz> {
    let tzid = tzid.trim().trim_matches('"');
    if let Ok(tz) = tzid.parse::<Tz>() {
        return Some(tz);
    }
    if tzid.eq_ignore_ascii_case("GMT") || tzid.eq_ignore_ascii_case("UTC") {
        return "UTC".parse().ok();
    }
    if let Some(mapped) = windows_zone(tzid)
        && let Ok(tz) = mapped.parse::<Tz>()
    {
        return Some(tz);
    }
    let parts: Vec<&str> = tzid.split('/').filter(|part| !part.is_empty()).collect();
    if parts.len() >= 2 {
        let candidate = format!("{}/{}", parts[parts.len() - 2], parts[parts.len() - 1]);
        if let Ok(tz) = candidate.parse::<Tz>() {
            return Some(tz);
        }
    }
    None
}

fn windows_zone(name: &str) -> Option<&'static str> {
    Some(match name {
        "Aleutian Standard Time" => "America/Adak",
        "Hawaiian Standard Time" => "Pacific/Honolulu",
        "Alaskan Standard Time" => "America/Anchorage",
        "Pacific Standard Time" => "America/Los_Angeles",
        "US Mountain Standard Time" => "America/Phoenix",
        "Mountain Standard Time" => "America/Denver",
        "Central Standard Time" => "America/Chicago",
        "Eastern Standard Time" => "America/New_York",
        "US Eastern Standard Time" => "America/Indiana/Indianapolis",
        "Atlantic Standard Time" => "America/Halifax",
        "SA Pacific Standard Time" => "America/Bogota",
        "SA Western Standard Time" => "America/La_Paz",
        "Pacific SA Standard Time" => "America/Santiago",
        "E. South America Standard Time" => "America/Sao_Paulo",
        "Argentina Standard Time" => "America/Argentina/Buenos_Aires",
        "Newfoundland Standard Time" => "America/St_Johns",
        "GMT Standard Time" => "Europe/London",
        "Greenwich Standard Time" => "Atlantic/Reykjavik",
        "W. Europe Standard Time" => "Europe/Berlin",
        "Central Europe Standard Time" => "Europe/Budapest",
        "Romance Standard Time" => "Europe/Paris",
        "Central European Standard Time" => "Europe/Warsaw",
        "W. Central Africa Standard Time" => "Africa/Lagos",
        "GTB Standard Time" => "Europe/Bucharest",
        "Egypt Standard Time" => "Africa/Cairo",
        "South Africa Standard Time" => "Africa/Johannesburg",
        "FLE Standard Time" => "Europe/Kyiv",
        "Israel Standard Time" => "Asia/Jerusalem",
        "Russian Standard Time" => "Europe/Moscow",
        "Arab Standard Time" => "Asia/Riyadh",
        "E. Africa Standard Time" => "Africa/Nairobi",
        "India Standard Time" => "Asia/Kolkata",
        "SE Asia Standard Time" => "Asia/Bangkok",
        "China Standard Time" => "Asia/Shanghai",
        "Singapore Standard Time" => "Asia/Singapore",
        "Taipei Standard Time" => "Asia/Taipei",
        "W. Australia Standard Time" => "Australia/Perth",
        "Tokyo Standard Time" => "Asia/Tokyo",
        "Korea Standard Time" => "Asia/Seoul",
        "AUS Central Standard Time" => "Australia/Darwin",
        "Cen. Australia Standard Time" => "Australia/Adelaide",
        "AUS Eastern Standard Time" => "Australia/Sydney",
        "E. Australia Standard Time" => "Australia/Brisbane",
        "Tasmania Standard Time" => "Australia/Hobart",
        "New Zealand Standard Time" => "Pacific/Auckland",
        "UTC" | "Coordinated Universal Time" => "UTC",
        _ => return None,
    })
}

fn from_naive(naive: NaiveDateTime, anchor: Anchor, zone: Tz) -> DateTime<Utc> {
    match anchor {
        Anchor::Zoned(tz) => zone_to_utc(naive, &tz),
        Anchor::Utc => naive.and_utc(),
        Anchor::Floating => zone_to_utc(naive, &zone),
    }
}

fn zone_to_utc(naive: NaiveDateTime, tz: &Tz) -> DateTime<Utc> {
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt.with_timezone(&Utc),
        LocalResult::Ambiguous(early, _) => early.with_timezone(&Utc),
        LocalResult::None => naive
            .checked_add_signed(ChronoDuration::hours(1))
            .and_then(|next| match tz.from_local_datetime(&next) {
                LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => {
                    Some(dt.with_timezone(&Utc))
                }
                LocalResult::None => None,
            })
            .unwrap_or_else(|| naive.and_utc()),
    }
}

fn to_naive(instant: DateTime<Utc>, anchor: Anchor, zone: Tz) -> NaiveDateTime {
    match anchor {
        Anchor::Zoned(tz) => instant.with_timezone(&tz).naive_local(),
        Anchor::Utc => instant.naive_utc(),
        Anchor::Floating => instant.with_timezone(&zone).naive_local(),
    }
}

fn parse_rule(value: &str, anchor: Anchor, zone: Tz) -> Option<Rule> {
    let mut freq = None;
    let mut interval = 1i64;
    let mut count = None;
    let mut until = None;
    let mut by_day = Vec::new();
    let mut by_month_day = Vec::new();
    let mut by_month = Vec::new();
    let mut by_year_day = Vec::new();
    let mut by_week_no = Vec::new();
    let mut by_hour = Vec::new();
    let mut by_minute = Vec::new();
    let mut by_set_pos = Vec::new();
    let mut wkst = Weekday::Mon;
    let mut unhonored = None;
    for part in value.split(';') {
        let Some((key, val)) = part.split_once('=') else {
            continue;
        };
        let key = key.to_ascii_uppercase();
        match key.as_str() {
            "FREQ" => freq = parse_freq(val),
            "INTERVAL" => interval = val.parse::<i64>().unwrap_or(1).max(1),
            "COUNT" => count = val.parse().ok(),
            "UNTIL" => until = parse_stamp(val, &[], anchor, zone),
            "BYDAY" => match parse_byday(val) {
                days if days.is_empty() => note_unhonored(&mut unhonored, &key),
                days => by_day = days,
            },
            "BYMONTHDAY" => match kept_ints(val, |day| day != 0 && (-31..=31).contains(&day)) {
                Some(days) => by_month_day = days,
                None => note_unhonored(&mut unhonored, &key),
            },
            "BYMONTH" => match kept_ints(val, |month| (1..=12).contains(&month)) {
                Some(months) => by_month = months.into_iter().map(|month| month as u32).collect(),
                None => note_unhonored(&mut unhonored, &key),
            },
            "BYYEARDAY" => match kept_ints(val, |day| day != 0 && (-366..=366).contains(&day)) {
                Some(days) => by_year_day = days,
                None => note_unhonored(&mut unhonored, &key),
            },
            "BYWEEKNO" => match kept_ints(val, |week| week != 0 && (-53..=53).contains(&week)) {
                Some(weeks) => by_week_no = weeks,
                None => note_unhonored(&mut unhonored, &key),
            },
            "BYHOUR" => match kept_ints(val, |hour| (0..24).contains(&hour)) {
                Some(hours) => by_hour = hours.into_iter().map(|hour| hour as u32).collect(),
                None => note_unhonored(&mut unhonored, &key),
            },
            "BYMINUTE" => match kept_ints(val, |minute| (0..60).contains(&minute)) {
                Some(minutes) => {
                    by_minute = minutes.into_iter().map(|minute| minute as u32).collect();
                }
                None => note_unhonored(&mut unhonored, &key),
            },
            "BYSETPOS" => match kept_ints(val, |pos| pos != 0) {
                Some(positions) => by_set_pos = positions,
                None => note_unhonored(&mut unhonored, &key),
            },
            "WKST" => wkst = weekday_token(val).unwrap_or(Weekday::Mon),
            _ => note_unhonored(&mut unhonored, &key),
        }
    }
    let freq = freq?;
    // A numbered weekday is only defined for months and years. Elsewhere it would match
    // every one of those weekdays, which is a wider series than the rule asked for.
    if by_day.iter().any(|day| day.nth.is_some()) && !matches!(freq, Freq::Monthly | Freq::Yearly) {
        note_unhonored(&mut unhonored, "BYDAY");
    }
    Some(Rule {
        freq,
        interval,
        count,
        until,
        by_day,
        by_month_day,
        by_month,
        by_year_day,
        by_week_no,
        by_hour,
        by_minute,
        by_set_pos,
        wkst,
        unhonored,
    })
}

fn note_unhonored(slot: &mut Option<String>, part: &str) {
    if slot.is_none() {
        *slot = Some(part.to_string());
    }
}

fn kept_ints(value: &str, keep: impl Fn(i32) -> bool) -> Option<Vec<i32>> {
    let parsed = parse_ints(value);
    if parsed.is_empty() {
        return None;
    }
    let kept: Vec<i32> = parsed.into_iter().filter(|number| keep(*number)).collect();
    if kept.is_empty() { None } else { Some(kept) }
}

fn parse_freq(value: &str) -> Option<Freq> {
    Some(match value.to_ascii_uppercase().as_str() {
        "MINUTELY" => Freq::Minutely,
        "HOURLY" => Freq::Hourly,
        "DAILY" => Freq::Daily,
        "WEEKLY" => Freq::Weekly,
        "MONTHLY" => Freq::Monthly,
        "YEARLY" => Freq::Yearly,
        _ => return None,
    })
}

fn parse_byday(value: &str) -> Vec<ByDay> {
    value.split(',').filter_map(parse_byday_token).collect()
}

fn parse_byday_token(token: &str) -> Option<ByDay> {
    let token = token.trim();
    if let Some(weekday) = weekday_token(token) {
        return Some(ByDay { nth: None, weekday });
    }
    let split = token.len().checked_sub(2)?;
    let nth = token.get(..split)?;
    let day = token.get(split..)?;
    Some(ByDay {
        nth: Some(nth.parse().ok()?),
        weekday: weekday_token(day)?,
    })
}

fn weekday_token(token: &str) -> Option<Weekday> {
    Some(match token.to_ascii_uppercase().as_str() {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        _ => return None,
    })
}

fn parse_ints(value: &str) -> Vec<i32> {
    value
        .split(',')
        .filter_map(|part| part.trim().parse().ok())
        .collect()
}

fn expand_group(
    master: &RawEvent,
    overrides: &[RawEvent],
    window: &Window,
    zone: Tz,
) -> Vec<Event> {
    let Some((start, end)) = span(master, zone) else {
        return Vec::new();
    };
    // A DURATION's days stay civil. An end time is a fixed length, measured on the first instance.
    let duration = match master.duration {
        Some(duration) if master.end.is_none() => positive_duration(duration),
        _ => positive_duration(duration_between(start, end)),
    };
    let mut produced = match &master.rule {
        Some(rule) => generate(master, rule, window, zone),
        None => vec![start],
    };
    produced.extend(master.rdates.iter().copied());
    produced = dedup_stamps(produced, master.anchor, zone);
    produced.retain(|stamp| {
        !master
            .exdates
            .iter()
            .any(|ex| same_stamp(*stamp, *ex, master.anchor, zone))
    });
    let mut events = Vec::new();
    for stamp in produced {
        if overrides.iter().any(|extra| {
            extra
                .recurrence_id
                .is_some_and(|id| same_stamp(stamp, id, master.anchor, zone))
        }) {
            continue;
        }
        let end = add_duration(stamp, duration, master.anchor, zone);
        if let Some(event) = build_event(master, stamp, end, window) {
            events.push(event);
        }
    }
    for extra in overrides {
        if is_cancelled(&extra.status) || extra.declined {
            continue;
        }
        let merged = overlay(master, extra);
        let Some((start, end)) = span(&merged, zone) else {
            continue;
        };
        let end = if extra.end.is_none() && extra.duration.is_none() {
            add_duration(start, duration, merged.anchor, zone)
        } else {
            end
        };
        if let Some(event) = build_event(&merged, start, end, window) {
            events.push(event);
        }
    }
    events
}

fn overlay(master: &RawEvent, extra: &RawEvent) -> RawEvent {
    let mut event = master.clone();
    event.start = extra.start;
    event.end = extra.end;
    event.duration = extra.duration.or(master.duration);
    if !extra.summary.is_empty() {
        event.summary.clone_from(&extra.summary);
    }
    if !extra.location.is_empty() {
        event.location.clone_from(&extra.location);
    }
    if !extra.description.is_empty() {
        event.description.clone_from(&extra.description);
    }
    if !extra.url.is_empty() {
        event.url.clone_from(&extra.url);
    }
    if !extra.conference.is_empty() {
        event.conference.clone_from(&extra.conference);
    }
    if !extra.status.is_empty() {
        event.status.clone_from(&extra.status);
    }
    event.declined = extra.declined;
    event.rule = None;
    event.recurrence_id = extra.recurrence_id;
    event.exdates.clear();
    event.rdates.clear();
    event
}

fn span(raw: &RawEvent, zone: Tz) -> Option<(Stamp, Stamp)> {
    let start = raw.start;
    if let Some(end) = raw.end {
        return Some((start, end));
    }
    if let Some(duration) = raw.duration {
        return Some((start, add_duration(start, duration, raw.anchor, zone)));
    }
    let end = match start {
        Stamp::Date(date) => Stamp::Date(date.checked_add_days(Days::new(1)).unwrap_or(date)),
        Stamp::Instant(instant) => Stamp::Instant(instant),
    };
    Some((start, end))
}

fn duration_between(start: Stamp, end: Stamp) -> CalDuration {
    match (start, end) {
        (Stamp::Instant(start), Stamp::Instant(end)) => CalDuration {
            days: 0,
            seconds: end.signed_duration_since(start).num_seconds(),
        },
        (Stamp::Date(start), Stamp::Date(end)) => CalDuration {
            days: end.signed_duration_since(start).num_days(),
            seconds: 0,
        },
        _ => CalDuration {
            days: 0,
            seconds: 0,
        },
    }
}

fn positive_duration(duration: CalDuration) -> CalDuration {
    if duration.days < 0 || (duration.days == 0 && duration.seconds < 0) {
        CalDuration {
            days: 0,
            seconds: 0,
        }
    } else {
        duration
    }
}

fn add_duration(start: Stamp, duration: CalDuration, anchor: Anchor, zone: Tz) -> Stamp {
    match start {
        Stamp::Date(date) => {
            let days = if duration.days > 0 {
                duration.days as u64
            } else if duration.seconds > 0 {
                1
            } else {
                0
            };
            Stamp::Date(date.checked_add_days(Days::new(days)).unwrap_or(date))
        }
        Stamp::Instant(instant) => {
            if duration.days == 0 {
                return Stamp::Instant(instant + ChronoDuration::seconds(duration.seconds));
            }
            let naive = to_naive(instant, anchor, zone);
            let Some(shifted) = naive
                .date()
                .checked_add_signed(ChronoDuration::days(duration.days))
                .and_then(|date| {
                    date.and_time(naive.time())
                        .checked_add_signed(ChronoDuration::seconds(duration.seconds))
                })
            else {
                return Stamp::Instant(instant);
            };
            Stamp::Instant(from_naive(shifted, anchor, zone))
        }
    }
}

fn dedup_stamps(stamps: Vec<Stamp>, anchor: Anchor, zone: Tz) -> Vec<Stamp> {
    let mut kept = Vec::new();
    for stamp in stamps {
        if !kept
            .iter()
            .any(|have| same_stamp(*have, stamp, anchor, zone))
        {
            kept.push(stamp);
        }
    }
    kept
}

fn same_stamp(a: Stamp, b: Stamp, anchor: Anchor, zone: Tz) -> bool {
    match (a, b) {
        (Stamp::Date(a), Stamp::Date(b)) => a == b,
        (Stamp::Instant(a), Stamp::Instant(b)) => a.timestamp() == b.timestamp(),
        (Stamp::Date(date), Stamp::Instant(instant))
        | (Stamp::Instant(instant), Stamp::Date(date)) => {
            to_naive(instant, anchor, zone).date() == date
        }
    }
}

struct When {
    anchor: Anchor,
    zone: Tz,
    date_only: bool,
}

fn generate(master: &RawEvent, rule: &Rule, window: &Window, zone: Tz) -> Vec<Stamp> {
    if let Some(part) = &rule.unhonored {
        log::warn!("calendar recurrence skips {part}");
        return vec![master.start];
    }
    let Some(start_naive) = stamp_naive(master.start, master.anchor, zone) else {
        return vec![master.start];
    };
    let hard_end = rule
        .until
        .and_then(|until| stamp_naive(until, master.anchor, zone))
        .unwrap_or_else(|| to_naive(window.end, master.anchor, zone));
    let hard_end = hard_end.min(to_naive(window.end, master.anchor, zone));
    let not_before = to_naive(window.start - ChronoDuration::days(2), master.anchor, zone);
    let when = When {
        anchor: master.anchor,
        zone,
        date_only: matches!(master.start, Stamp::Date(_)),
    };
    let mut times = match rule.freq {
        Freq::Minutely => gen_stepped(
            start_naive,
            hard_end,
            not_before,
            ChronoDuration::minutes(rule.interval),
            rule,
            &when,
            true,
        ),
        Freq::Hourly => gen_stepped(
            start_naive,
            hard_end,
            not_before,
            ChronoDuration::hours(rule.interval),
            rule,
            &when,
            true,
        ),
        Freq::Daily => gen_stepped(
            start_naive,
            hard_end,
            not_before,
            ChronoDuration::days(rule.interval),
            rule,
            &when,
            false,
        ),
        Freq::Weekly => gen_weekly(start_naive, hard_end, rule, &when),
        Freq::Monthly => gen_monthly(start_naive, hard_end, rule, &when),
        Freq::Yearly => gen_yearly(start_naive, hard_end, rule, &when),
    };
    times.sort();
    times.dedup();
    times
        .into_iter()
        .map(|naive| {
            if when.date_only {
                Stamp::Date(naive.date())
            } else {
                Stamp::Instant(from_naive(naive, master.anchor, zone))
            }
        })
        .collect()
}

fn stamp_naive(stamp: Stamp, anchor: Anchor, zone: Tz) -> Option<NaiveDateTime> {
    Some(match stamp {
        Stamp::Instant(instant) => to_naive(instant, anchor, zone),
        Stamp::Date(date) => date.and_time(NaiveTime::MIN),
    })
}

fn filters_empty(rule: &Rule) -> bool {
    rule.by_day.is_empty()
        && rule.by_month_day.is_empty()
        && rule.by_month.is_empty()
        && rule.by_year_day.is_empty()
        && rule.by_week_no.is_empty()
        && rule.by_hour.is_empty()
        && rule.by_minute.is_empty()
        && rule.by_set_pos.is_empty()
}

fn gen_stepped(
    start: NaiveDateTime,
    hard_end: NaiveDateTime,
    not_before: NaiveDateTime,
    step: ChronoDuration,
    rule: &Rule,
    when: &When,
    jump: bool,
) -> Vec<NaiveDateTime> {
    let mut out = Vec::new();
    let mut seen = 0u32;
    let mut cursor = start;
    // Jumping counts every step, including a local time that does not exist, so only UTC uses it.
    if jump && filters_empty(rule) && matches!(when.anchor, Anchor::Utc) && start < not_before {
        let steps = steps_between(start, not_before, step);
        if rule.count.is_some_and(|max| steps as u32 >= max) {
            return out;
        }
        if let Some(next) = add_steps(start, step, steps) {
            cursor = next;
            seen = steps as u32;
        }
    }
    let mut guard = 0u32;
    while cursor <= hard_end && guard < 100_000 {
        guard += 1;
        if occurrence_ok(cursor, rule) && time_exists(cursor, when) {
            for time in select_setpos(vec![cursor], &rule.by_set_pos) {
                seen += 1;
                if rule.count.is_some_and(|max| seen > max) {
                    return out;
                }
                out.push(time);
            }
        }
        let Some(next) = cursor.checked_add_signed(step) else {
            break;
        };
        if next <= cursor {
            break;
        }
        cursor = next;
    }
    out
}

fn steps_between(start: NaiveDateTime, target: NaiveDateTime, step: ChronoDuration) -> i64 {
    let step_secs = step.num_seconds();
    if step_secs <= 0 || target <= start {
        return 0;
    }
    (target - start).num_seconds() / step_secs
}

fn add_steps(start: NaiveDateTime, step: ChronoDuration, steps: i64) -> Option<NaiveDateTime> {
    let secs = step.num_seconds().checked_mul(steps)?;
    start.checked_add_signed(ChronoDuration::seconds(secs))
}

fn gen_weekly(
    start: NaiveDateTime,
    hard_end: NaiveDateTime,
    rule: &Rule,
    when: &When,
) -> Vec<NaiveDateTime> {
    let days: Vec<Weekday> = if rule.by_day.is_empty() {
        vec![start.weekday()]
    } else {
        rule.by_day.iter().map(|day| day.weekday).collect()
    };
    let origin = week_start(start.date(), rule.wkst);
    let mut out = Vec::new();
    let mut seen = 0u32;
    let mut week = 0i64;
    while week < 20_000 {
        let Some(week_date) = origin.checked_add_days(Days::new((week * 7) as u64)) else {
            break;
        };
        if week_date > hard_end.date() {
            break;
        }
        let mut batch = Vec::new();
        for day in &days {
            let offset = weekday_offset(*day, rule.wkst);
            let Some(date) = origin.checked_add_days(Days::new((week * 7 + offset) as u64)) else {
                continue;
            };
            let time = date.and_time(start.time());
            if occurrence_ok(time, rule) {
                batch.push(time);
            }
        }
        batch.sort();
        batch.dedup();
        let batch = select_setpos(batch, &rule.by_set_pos);
        if !accept_times(&mut out, &mut seen, batch, start, hard_end, rule, when) {
            break;
        }
        week += rule.interval;
    }
    out
}

fn week_start(date: NaiveDate, wkst: Weekday) -> NaiveDate {
    let back = weekday_offset(date.weekday(), wkst) as u64;
    date.checked_sub_days(Days::new(back)).unwrap_or(date)
}

fn weekday_offset(day: Weekday, wkst: Weekday) -> i64 {
    let day = day.num_days_from_monday() as i64;
    let start = wkst.num_days_from_monday() as i64;
    (day - start).rem_euclid(7)
}

fn gen_monthly(
    start: NaiveDateTime,
    hard_end: NaiveDateTime,
    rule: &Rule,
    when: &When,
) -> Vec<NaiveDateTime> {
    let mut out = Vec::new();
    let mut seen = 0u32;
    let mut step = 0i32;
    while step < 2_400 {
        let (year, month) = add_months(start.year(), start.month(), step);
        let Some(month_start) = NaiveDate::from_ymd_opt(year, month, 1) else {
            break;
        };
        if month_start > hard_end.date() {
            break;
        }
        if month_ok_month(month, rule) {
            let batch = select_setpos(month_times(year, month, start, rule), &rule.by_set_pos);
            if !accept_times(&mut out, &mut seen, batch, start, hard_end, rule, when) {
                break;
            }
        }
        step += rule.interval as i32;
    }
    out
}

fn gen_yearly(
    start: NaiveDateTime,
    hard_end: NaiveDateTime,
    rule: &Rule,
    when: &When,
) -> Vec<NaiveDateTime> {
    let mut out = Vec::new();
    let mut seen = 0u32;
    let mut year = start.year();
    let mut guard = 0i32;
    while year <= hard_end.year() && guard < 500 {
        guard += 1;
        let months = if rule.by_month.is_empty() {
            vec![start.month()]
        } else {
            rule.by_month.clone()
        };
        let mut batch = Vec::new();
        for month in months {
            batch.extend(month_times(year, month, start, rule));
        }
        batch.sort();
        batch.dedup();
        let batch = select_setpos(batch, &rule.by_set_pos);
        if !accept_times(&mut out, &mut seen, batch, start, hard_end, rule, when) {
            break;
        }
        year += rule.interval as i32;
    }
    out
}

fn month_ok_month(month: u32, rule: &Rule) -> bool {
    rule.by_month.is_empty() || rule.by_month.contains(&month)
}

fn month_times(year: i32, month: u32, start: NaiveDateTime, rule: &Rule) -> Vec<NaiveDateTime> {
    let mut batch = Vec::new();
    for date in month_dates(year, month, start, rule) {
        let time = date.and_time(start.time());
        if occurrence_ok(time, rule) {
            batch.push(time);
        }
    }
    batch.sort();
    batch.dedup();
    batch
}

/// Keeps occurrences inside the window and the count. Times the zone skips are not counted.
/// False means the series is finished.
fn accept_times(
    out: &mut Vec<NaiveDateTime>,
    seen: &mut u32,
    times: Vec<NaiveDateTime>,
    start: NaiveDateTime,
    hard_end: NaiveDateTime,
    rule: &Rule,
    when: &When,
) -> bool {
    for time in times {
        if time < start {
            continue;
        }
        if time > hard_end {
            return false;
        }
        if !time_exists(time, when) {
            continue;
        }
        *seen += 1;
        if rule.count.is_some_and(|max| *seen > max) {
            return false;
        }
        out.push(time);
    }
    true
}

fn time_exists(naive: NaiveDateTime, when: &When) -> bool {
    if when.date_only {
        return true;
    }
    let zone = match when.anchor {
        Anchor::Utc => return true,
        Anchor::Zoned(tz) => tz,
        Anchor::Floating => when.zone,
    };
    !matches!(zone.from_local_datetime(&naive), LocalResult::None)
}

fn select_setpos(times: Vec<NaiveDateTime>, positions: &[i32]) -> Vec<NaiveDateTime> {
    if positions.is_empty() {
        return times;
    }
    let len = times.len() as i32;
    let mut picked = Vec::new();
    for pos in positions {
        let index = if *pos > 0 { *pos - 1 } else { len + *pos };
        if (0..len).contains(&index) {
            picked.push(times[index as usize]);
        }
    }
    picked.sort();
    picked.dedup();
    picked
}

fn occurrence_ok(time: NaiveDateTime, rule: &Rule) -> bool {
    (rule.by_month.is_empty() || rule.by_month.contains(&time.month()))
        && month_day_matches(time.date(), &rule.by_month_day)
        && year_day_matches(time.date(), &rule.by_year_day)
        && week_matches(time.date(), &rule.by_week_no, rule.wkst)
        && (rule.by_hour.is_empty() || rule.by_hour.contains(&time.hour()))
        && (rule.by_minute.is_empty() || rule.by_minute.contains(&time.minute()))
        && byday_matches(time.date(), rule)
}

fn month_day_matches(date: NaiveDate, days: &[i32]) -> bool {
    days.is_empty()
        || days
            .iter()
            .any(|day| month_day(date.year(), date.month(), *day) == Some(date))
}

fn year_day_matches(date: NaiveDate, days: &[i32]) -> bool {
    if days.is_empty() {
        return true;
    }
    let last = NaiveDate::from_ymd_opt(date.year(), 12, 31).map_or(365, |end| end.ordinal() as i32);
    let ordinal = date.ordinal() as i32;
    days.iter().any(|day| {
        let want = if *day > 0 { *day } else { last + *day + 1 };
        want == ordinal
    })
}

fn week_matches(date: NaiveDate, weeks: &[i32], wkst: Weekday) -> bool {
    if weeks.is_empty() {
        return true;
    }
    let Some((year, week)) = week_no(date, wkst) else {
        return false;
    };
    let total = weeks_in_year(year, wkst).unwrap_or(week);
    weeks.iter().any(|want| {
        let number = if *want > 0 { *want } else { total + *want + 1 };
        number == week
    })
}

fn byday_matches(date: NaiveDate, rule: &Rule) -> bool {
    rule.by_day.is_empty()
        || rule
            .by_day
            .iter()
            .any(|day| day.weekday == date.weekday() && nth_matches(date, day.nth))
}

fn nth_matches(date: NaiveDate, nth: Option<i8>) -> bool {
    let Some(nth) = nth else {
        return true;
    };
    nth_weekday(date.year(), date.month(), nth, date.weekday()) == Some(date)
}

/// Week 1 is the first week, starting on `wkst`, with at least four days in the year.
fn week_no(date: NaiveDate, wkst: Weekday) -> Option<(i32, i32)> {
    for year in [date.year() + 1, date.year(), date.year() - 1] {
        let start = week1_start(year, wkst)?;
        let next = week1_start(year + 1, wkst)?;
        if date >= start && date < next {
            let week = ((date - start).num_days() / 7) as i32 + 1;
            return Some((year, week));
        }
    }
    None
}

fn weeks_in_year(year: i32, wkst: Weekday) -> Option<i32> {
    let start = week1_start(year, wkst)?;
    let next = week1_start(year + 1, wkst)?;
    Some(((next - start).num_days() / 7) as i32)
}

fn week1_start(year: i32, wkst: Weekday) -> Option<NaiveDate> {
    let jan1 = NaiveDate::from_ymd_opt(year, 1, 1)?;
    let start = week_start(jan1, wkst);
    if weekday_offset(jan1.weekday(), wkst) <= 3 {
        Some(start)
    } else {
        start.checked_add_days(Days::new(7))
    }
}

fn month_dates(year: i32, month: u32, start: NaiveDateTime, rule: &Rule) -> Vec<NaiveDate> {
    if !rule.by_day.is_empty() {
        let mut dates = Vec::new();
        for day in &rule.by_day {
            if let Some(nth) = day.nth {
                if let Some(date) = nth_weekday(year, month, nth, day.weekday) {
                    dates.push(date);
                }
            } else {
                dates.extend(weekdays_in_month(year, month, day.weekday));
            }
        }
        dates.sort();
        dates.dedup();
        return dates;
    }
    if !rule.by_month_day.is_empty() {
        return rule
            .by_month_day
            .iter()
            .filter_map(|day| month_day(year, month, *day))
            .collect();
    }
    NaiveDate::from_ymd_opt(year, month, start.day())
        .into_iter()
        .collect()
}

fn nth_weekday(year: i32, month: u32, nth: i8, weekday: Weekday) -> Option<NaiveDate> {
    if nth == 0 {
        return None;
    }
    if nth > 0 {
        let mut date = NaiveDate::from_ymd_opt(year, month, 1)?;
        let mut seen = 0i8;
        while date.month() == month {
            if date.weekday() == weekday {
                seen += 1;
                if seen == nth {
                    return Some(date);
                }
            }
            date = date.checked_add_days(Days::new(1))?;
        }
        return None;
    }
    let mut date = last_of_month(year, month)?;
    let mut seen = 0i8;
    let want = -nth;
    loop {
        if date.weekday() == weekday {
            seen += 1;
            if seen == want {
                return Some(date);
            }
        }
        if date.day() == 1 {
            return None;
        }
        date = date.checked_sub_days(Days::new(1))?;
    }
}

fn weekdays_in_month(year: i32, month: u32, weekday: Weekday) -> Vec<NaiveDate> {
    let mut out = Vec::new();
    let Some(mut date) = NaiveDate::from_ymd_opt(year, month, 1) else {
        return out;
    };
    while date.month() == month {
        if date.weekday() == weekday {
            out.push(date);
        }
        let Some(next) = date.checked_add_days(Days::new(1)) else {
            break;
        };
        date = next;
    }
    out
}

fn month_day(year: i32, month: u32, day: i32) -> Option<NaiveDate> {
    if day > 0 {
        return NaiveDate::from_ymd_opt(year, month, day as u32);
    }
    if day == 0 {
        return None;
    }
    let last = last_of_month(year, month)?;
    let number = last.day() as i32 + day + 1;
    NaiveDate::from_ymd_opt(year, month, number as u32)
}

fn last_of_month(year: i32, month: u32) -> Option<NaiveDate> {
    let (year, month) = add_months(year, month, 1);
    NaiveDate::from_ymd_opt(year, month, 1)?.checked_sub_days(Days::new(1))
}

fn add_months(year: i32, month: u32, add: i32) -> (i32, u32) {
    let total = year as i64 * 12 + month as i64 - 1 + add as i64;
    let year = total.div_euclid(12) as i32;
    let month = (total.rem_euclid(12) + 1) as u32;
    (year, month)
}

fn build_event(raw: &RawEvent, start: Stamp, end: Stamp, window: &Window) -> Option<Event> {
    let meeting = meeting_link(&raw.conference, &raw.url, &raw.location, &raw.description);
    let title = if raw.summary.trim().is_empty() {
        "(No title)".to_string()
    } else {
        raw.summary.trim().chars().take(500).collect()
    };
    let location: String = raw.location.trim().chars().take(500).collect();
    match start {
        Stamp::Date(start_date) => {
            let end_date = match end {
                Stamp::Date(date) if date > start_date => date,
                _ => start_date
                    .checked_add_days(Days::new(1))
                    .unwrap_or(start_date),
            };
            if start_date > window.end_date || end_date <= window.start_date {
                return None;
            }
            Some(Event {
                id: event_id(&raw.uid, start),
                feed_id: String::new(),
                color: String::new(),
                title,
                start_ms: start_date
                    .and_time(NaiveTime::MIN)
                    .and_utc()
                    .timestamp_millis(),
                end_ms: end_date
                    .and_time(NaiveTime::MIN)
                    .and_utc()
                    .timestamp_millis(),
                all_day: true,
                start_date: ymd(start_date),
                end_date: ymd(end_date),
                location,
                url: meeting
                    .as_ref()
                    .map(|item| item.url.clone())
                    .unwrap_or_default(),
                provider: meeting
                    .map(|item| item.provider.to_string())
                    .unwrap_or_default(),
            })
        }
        Stamp::Instant(start_at) => {
            let end_at = match end {
                Stamp::Instant(instant) => instant,
                Stamp::Date(_) => start_at,
            };
            if start_at > window.end || end_at < window.start {
                return None;
            }
            Some(Event {
                id: event_id(&raw.uid, start),
                feed_id: String::new(),
                color: String::new(),
                title,
                start_ms: start_at.timestamp_millis(),
                end_ms: end_at.timestamp_millis(),
                all_day: false,
                start_date: String::new(),
                end_date: String::new(),
                location,
                url: meeting
                    .as_ref()
                    .map(|item| item.url.clone())
                    .unwrap_or_default(),
                provider: meeting
                    .map(|item| item.provider.to_string())
                    .unwrap_or_default(),
            })
        }
    }
}

fn event_id(uid: &str, start: Stamp) -> String {
    let key = match start {
        Stamp::Instant(instant) => instant.timestamp().to_string(),
        Stamp::Date(date) => date.format("%Y%m%d").to_string(),
    };
    if uid.is_empty() {
        format!("event-{key}")
    } else {
        format!("{uid}@{key}")
    }
}

fn ymd(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Meeting {
    url: String,
    provider: &'static str,
}

fn meeting_link(conference: &str, url: &str, location: &str, description: &str) -> Option<Meeting> {
    for text in [conference, url, location, description] {
        if let Some(found) = first_meeting(text) {
            return Some(found);
        }
    }
    None
}

fn first_meeting(text: &str) -> Option<Meeting> {
    let mut from = 0;
    while from < text.len() {
        let Some(slice) = text.get(from..) else {
            break;
        };
        let Some(at) = next_marker(slice) else {
            break;
        };
        let at = from + at;
        let (url, next) = take_url(text, at);
        if let Some(meeting) = classify(&url) {
            return Some(meeting);
        }
        if next <= from {
            break;
        }
        from = next;
    }
    None
}

fn next_marker(text: &str) -> Option<usize> {
    let lower = text.to_ascii_lowercase();
    let mut best = None;
    for marker in [
        "https://",
        "http://",
        "meet.google.com",
        "teams.microsoft.com",
        "teams.live.com",
        "zoom.us",
        "webex.com",
    ] {
        if let Some(at) = lower.find(marker) {
            best = Some(best.map_or(at, |so_far: usize| so_far.min(at)));
        }
    }
    best
}

fn take_url(text: &str, marker_at: usize) -> (String, usize) {
    let bytes = text.as_bytes();
    let mut start = marker_at.min(bytes.len());
    while start > 0 && is_url_host(bytes[start - 1]) {
        start -= 1;
    }
    if ends_with_ignore_ascii(text, start, "https://") {
        start -= 8;
    } else if ends_with_ignore_ascii(text, start, "http://") {
        start -= 7;
    }
    let mut end = start;
    while end < bytes.len() && !is_url_end(bytes[end]) {
        end += 1;
    }
    while end > start && matches!(bytes[end - 1], b'.' | b',' | b';' | b')' | b']') {
        end -= 1;
    }
    let Some(raw) = text.get(start..end) else {
        return (String::new(), marker_at.saturating_add(1).min(text.len()));
    };
    let url = if strip_prefix_ignore_ascii(raw, "http://").is_some()
        || strip_prefix_ignore_ascii(raw, "https://").is_some()
    {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };
    (url, end.max(marker_at.saturating_add(1)))
}

fn ends_with_ignore_ascii(text: &str, end: usize, prefix: &str) -> bool {
    end >= prefix.len()
        && text
            .get(end - prefix.len()..end)
            .is_some_and(|slice| slice.eq_ignore_ascii_case(prefix))
}

fn is_url_host(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')
}

fn is_url_end(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'<' | b'>' | b'"' | b'\'' | b'|')
}

fn classify(url: &str) -> Option<Meeting> {
    let url = unwrap_redirects(url);
    let parsed = http_url(&url)?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    let path = parsed.path().trim_start_matches('/').to_ascii_lowercase();
    let provider = if host == "meet.google.com" && !path.is_empty() {
        "Google Meet"
    } else if (host == "zoom.us" || host.ends_with(".zoom.us")) && zoom_path(&path) {
        "Zoom"
    } else if (host == "teams.microsoft.com" && teams_path(&path))
        || (host == "teams.live.com" && path_after(&path, "meet/"))
    {
        "Teams"
    } else if (host == "webex.com" || host.ends_with(".webex.com")) && !path.is_empty() {
        "Webex"
    } else {
        return None;
    };
    Some(Meeting { url, provider })
}

fn zoom_path(path: &str) -> bool {
    ["j/", "my/", "s/", "w/", "wc/"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

fn teams_path(path: &str) -> bool {
    path_after(path, "l/meetup-join/") || path_after(path, "meet/")
}

fn path_after(path: &str, prefix: &str) -> bool {
    path.starts_with(prefix) && path.len() > prefix.len()
}

/// An http(s) URL with no userinfo and no backslash. A browser reads `\` as `/`, so a
/// backslash would name a different host than a textual split.
fn http_url(url: &str) -> Option<url::Url> {
    if url.contains('\\') || url.chars().any(char::is_whitespace) {
        return None;
    }
    let parsed = url::Url::parse(url).ok()?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return None;
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return None;
    }
    parsed.host_str()?;
    Some(parsed)
}

/// Outlook Safe Links and Google's `/url?q=` wrapper, a few layers deep.
fn unwrap_redirects(url: &str) -> String {
    let mut current = url.to_string();
    for _ in 0..3 {
        let Some(next) = redirect_target(&current) else {
            break;
        };
        if next == current || next.len() > 2000 {
            break;
        }
        current = next;
    }
    current
}

fn redirect_target(url: &str) -> Option<String> {
    let parsed = http_url(url)?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    let key = if host == "www.google.com" && parsed.path() == "/url" {
        "q"
    } else if host == "safelinks.protection.outlook.com"
        || host.ends_with(".safelinks.protection.outlook.com")
    {
        "url"
    } else {
        return None;
    };
    parsed
        .query_pairs()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.into_owned())
}

fn owner_email(url: &str) -> Option<String> {
    let decoded = urlencoding::decode(url)
        .map(|text| text.into_owned())
        .unwrap_or_else(|_| url.to_string());
    find_email(&decoded)
}

fn find_email(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'@' {
            let mut start = index;
            while start > 0 && is_email_local(bytes[start - 1]) {
                start -= 1;
            }
            let mut end = index + 1;
            while end < bytes.len() && is_email_domain(bytes[end]) {
                end += 1;
            }
            let Some(local) = text.get(start..index) else {
                index += 1;
                continue;
            };
            let Some(domain) = text.get(index + 1..end) else {
                index += 1;
                continue;
            };
            if !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && let Some(address) = text.get(start..end)
            {
                return Some(address.to_ascii_lowercase());
            }
        }
        index += 1;
    }
    None
}

fn is_email_local(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'%' | b'+' | b'-')
}

fn is_email_domain(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
            .unwrap()
    }

    fn parse_at(ics: &str, owner: Option<&str>, zone: Tz) -> Vec<Event> {
        parse_in(ics, owner, utc(2026, 11, 3, 18, 0, 0), zone)
    }

    fn parse_in(ics: &str, owner: Option<&str>, now: DateTime<Utc>, zone: Tz) -> Vec<Event> {
        parse_events(ics, owner, now, zone).unwrap()
    }

    fn titles(events: &[Event]) -> Vec<&str> {
        events.iter().map(|event| event.title.as_str()).collect()
    }

    #[test]
    fn folded_lines_and_escaped_text() {
        let ics = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:fold\n\
DTSTART:20261103T150000Z\n\
DTEND:20261103T160000Z\n\
SUMMARY:Design\n  \
review\\, part\\n2\n\
END:VEVENT\n\
END:VCALENDAR\n";
        let events = parse_at(ics, None, chrono_tz::UTC);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "Design review, part\n2");
    }

    #[test]
    fn utc_event_with_duration() {
        let ics = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:dur\n\
DTSTART:20261103T150000Z\n\
DURATION:PT45M\n\
SUMMARY:Design review\n\
LOCATION:Room 4\n\
END:VEVENT\n\
END:VCALENDAR\n";
        let events = parse_at(ics, None, chrono_tz::UTC);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "Design review");
        assert_eq!(events[0].location, "Room 4");
        assert_eq!(
            events[0].start_ms,
            utc(2026, 11, 3, 15, 0, 0).timestamp_millis()
        );
        assert_eq!(
            events[0].end_ms,
            utc(2026, 11, 3, 15, 45, 0).timestamp_millis()
        );
        assert!(!events[0].all_day);
    }

    #[test]
    fn tzid_and_windows_zone_and_floating() {
        let new_york = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:ny\n\
DTSTART;TZID=America/New_York:20261103T090000\n\
DTEND;TZID=America/New_York:20261103T100000\n\
SUMMARY:East\n\
END:VEVENT\n\
END:VCALENDAR\n";
        let events = parse_at(new_york, None, chrono_tz::UTC);
        assert_eq!(
            events[0].start_ms,
            utc(2026, 11, 3, 14, 0, 0).timestamp_millis()
        );
        assert_eq!(
            events[0].end_ms,
            utc(2026, 11, 3, 15, 0, 0).timestamp_millis()
        );

        let pacific = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:pt\n\
DTSTART;TZID=Pacific Standard Time:20261103T090000\n\
DTEND;TZID=Pacific Standard Time:20261103T100000\n\
SUMMARY:West\n\
END:VEVENT\n\
END:VCALENDAR\n";
        let events = parse_at(pacific, None, chrono_tz::UTC);
        assert_eq!(
            events[0].start_ms,
            utc(2026, 11, 3, 17, 0, 0).timestamp_millis()
        );

        let floating = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:float\n\
DTSTART:20261103T090000\n\
DTEND:20261103T100000\n\
SUMMARY:Float\n\
END:VEVENT\n\
END:VCALENDAR\n";
        let events = parse_at(floating, None, chrono_tz::America::New_York);
        assert_eq!(
            events[0].start_ms,
            utc(2026, 11, 3, 14, 0, 0).timestamp_millis()
        );
        assert_eq!(
            events[0].end_ms,
            utc(2026, 11, 3, 15, 0, 0).timestamp_millis()
        );
    }

    #[test]
    fn all_day_event_covers_its_dates() {
        let ics = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:day\n\
DTSTART;VALUE=DATE:20261103\n\
DTEND;VALUE=DATE:20261105\n\
SUMMARY:Offsite\n\
END:VEVENT\n\
END:VCALENDAR\n";
        let events = parse_at(ics, None, chrono_tz::UTC);
        assert_eq!(events.len(), 1);
        assert!(events[0].all_day);
        assert_eq!(events[0].start_date, "2026-11-03");
        assert_eq!(events[0].end_date, "2026-11-05");
        assert_eq!(events[0].title, "Offsite");
    }

    #[test]
    fn recurrence_exdate_override_and_count() {
        assert_eq!(
            NaiveDate::from_ymd_opt(2026, 11, 3).unwrap().weekday(),
            Weekday::Tue
        );
        let ics = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:series\n\
DTSTART:20261102T150000Z\n\
DTEND:20261102T160000Z\n\
RRULE:FREQ=WEEKLY;BYDAY=MO,WE,TH\n\
EXDATE:20261105T150000Z\n\
SUMMARY:Standup\n\
END:VEVENT\n\
BEGIN:VEVENT\n\
UID:series\n\
RECURRENCE-ID:20261102T150000Z\n\
DTSTART:20261102T170000Z\n\
DTEND:20261102T180000Z\n\
SUMMARY:Standup moved\n\
END:VEVENT\n\
BEGIN:VEVENT\n\
UID:daily\n\
DTSTART:20261104T120000Z\n\
DTEND:20261104T123000Z\n\
RRULE:FREQ=DAILY;COUNT=1\n\
SUMMARY:Once\n\
END:VEVENT\n\
END:VCALENDAR\n";
        let events = parse_at(ics, None, chrono_tz::UTC);
        assert_eq!(titles(&events), ["Standup moved", "Once", "Standup"]);
        let moved = events
            .iter()
            .find(|event| event.title == "Standup moved")
            .unwrap();
        assert_eq!(
            moved.start_ms,
            utc(2026, 11, 2, 17, 0, 0).timestamp_millis()
        );
        let standup = events
            .iter()
            .find(|event| event.title == "Standup")
            .unwrap();
        assert_eq!(
            standup.start_ms,
            utc(2026, 11, 4, 15, 0, 0).timestamp_millis()
        );
        assert!(
            events
                .iter()
                .all(|event| event.start_ms != utc(2026, 11, 2, 15, 0, 0).timestamp_millis())
        );
        assert!(
            events
                .iter()
                .all(|event| event.start_ms != utc(2026, 11, 5, 15, 0, 0).timestamp_millis())
        );
    }

    #[test]
    fn monthly_first_tuesday_and_cancelled_instance() {
        let ics = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:first\n\
DTSTART:20261103T150000Z\n\
DTEND:20261103T160000Z\n\
RRULE:FREQ=MONTHLY;BYDAY=1TU\n\
SUMMARY:First Tuesday\n\
END:VEVENT\n\
BEGIN:VEVENT\n\
UID:daily\n\
DTSTART:20261102T120000Z\n\
DTEND:20261102T130000Z\n\
RRULE:FREQ=DAILY\n\
SUMMARY:Daily\n\
END:VEVENT\n\
BEGIN:VEVENT\n\
UID:daily\n\
RECURRENCE-ID:20261103T120000Z\n\
DTSTART:20261103T120000Z\n\
STATUS:CANCELLED\n\
END:VEVENT\n\
END:VCALENDAR\n";
        let events = parse_at(ics, None, chrono_tz::UTC);
        assert!(events.iter().any(|event| event.title == "First Tuesday"));
        let daily: Vec<i64> = events
            .iter()
            .filter(|event| event.title == "Daily")
            .map(|event| event.start_ms)
            .collect();
        assert!(daily.contains(&utc(2026, 11, 2, 12, 0, 0).timestamp_millis()));
        assert!(daily.contains(&utc(2026, 11, 4, 12, 0, 0).timestamp_millis()));
        assert!(!daily.contains(&utc(2026, 11, 3, 12, 0, 0).timestamp_millis()));
    }

    #[test]
    fn cancelled_series_and_declined_attendees() {
        let ics = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:gone\n\
DTSTART:20261103T150000Z\n\
DTEND:20261103T160000Z\n\
STATUS:CANCELLED\n\
SUMMARY:Cancelled\n\
END:VEVENT\n\
BEGIN:VEVENT\n\
UID:me\n\
DTSTART:20261103T151500Z\n\
DTEND:20261103T161500Z\n\
SUMMARY:Mine\n\
ATTENDEE;PARTSTAT=DECLINED:mailto:me@example.com\n\
END:VEVENT\n\
BEGIN:VEVENT\n\
UID:other\n\
DTSTART:20261103T170000Z\n\
DTEND:20261103T180000Z\n\
SUMMARY:Keep other\n\
ATTENDEE;PARTSTAT=DECLINED:mailto:other@example.com\n\
END:VEVENT\n\
END:VCALENDAR\n";
        let with_owner = parse_at(ics, Some("me@example.com"), chrono_tz::UTC);
        assert_eq!(titles(&with_owner), ["Keep other"]);
        let without = parse_at(ics, None, chrono_tz::UTC);
        assert_eq!(titles(&without), ["Mine", "Keep other"]);
        assert_eq!(
            owner_email(
                "https://calendar.google.com/calendar/ical/me%40example.com/private-abc/basic.ics"
            )
            .as_deref(),
            Some("me@example.com")
        );
    }

    #[test]
    fn meeting_links_prefer_the_conference_property() {
        assert_eq!(
            meeting_link(
                "https://meet.google.com/abc-defg-hij",
                "",
                "https://zoom.us/j/111",
                "https://teams.microsoft.com/l/meetup-join/abc"
            ),
            Some(Meeting {
                url: "https://meet.google.com/abc-defg-hij".into(),
                provider: "Google Meet",
            })
        );
        assert_eq!(
            meeting_link("", "https://us02web.zoom.us/j/999?pwd=ab", "", "")
                .unwrap()
                .provider,
            "Zoom"
        );
        assert_eq!(
            meeting_link("", "", "https://teams.live.com/meet/abc", "")
                .unwrap()
                .provider,
            "Teams"
        );
        assert_eq!(
            meeting_link("", "", "", "Join https://acme.webex.com/meet/ada today.")
                .unwrap()
                .provider,
            "Webex"
        );
        assert_eq!(
            meeting_link("", "", "meet.google.com/abc-defg-hij", "")
                .unwrap()
                .url,
            "https://meet.google.com/abc-defg-hij"
        );
        assert!(meeting_link("", "", "Room 4", "bring notes").is_none());

        let ics = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:meet\n\
DTSTART:20261103T150000Z\n\
DTEND:20261103T160000Z\n\
SUMMARY:Design review\n\
X-GOOGLE-CONFERENCE:https://meet.google.com/abc-defg-hij\n\
DESCRIPTION:backup https://zoom.us/j/1\n\
END:VEVENT\n\
END:VCALENDAR\n";
        let events = parse_at(ics, None, chrono_tz::UTC);
        assert_eq!(events[0].provider, "Google Meet");
        assert_eq!(events[0].url, "https://meet.google.com/abc-defg-hij");
    }

    #[test]
    fn events_outside_the_window_are_left_out() {
        let ics = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:old\n\
DTSTART:20261101T150000Z\n\
DTEND:20261101T160000Z\n\
SUMMARY:Old\n\
END:VEVENT\n\
BEGIN:VEVENT\n\
UID:soon\n\
DTSTART:20261104T150000Z\n\
DTEND:20261104T160000Z\n\
SUMMARY:Soon\n\
END:VEVENT\n\
BEGIN:VEVENT\n\
UID:far\n\
DTSTART:20261106T150000Z\n\
DTEND:20261106T160000Z\n\
SUMMARY:Far\n\
END:VEVENT\n\
END:VCALENDAR\n";
        assert_eq!(titles(&parse_at(ics, None, chrono_tz::UTC)), ["Soon"]);
    }

    #[test]
    fn a_page_that_is_not_a_feed_is_an_error() {
        let err = parse_events(
            "<html>login</html>",
            None,
            utc(2026, 11, 3, 18, 0, 0),
            chrono_tz::UTC,
        );
        assert!(err.is_err());
    }

    #[test]
    fn nth_weekday_and_feed_helpers() {
        assert_eq!(
            nth_weekday(2026, 11, 1, Weekday::Tue),
            NaiveDate::from_ymd_opt(2026, 11, 3)
        );
        assert_eq!(
            nth_weekday(2026, 11, -1, Weekday::Fri),
            NaiveDate::from_ymd_opt(2026, 11, 27)
        );
        let name = validate_name("  Work   calendar ").unwrap();
        assert_eq!(name, "Work calendar");
        assert!(validate_link("Work", "not a link").is_err());
        assert!(validate_link("Work", "ftp://example.com/cal.ics").is_err());
        assert_eq!(
            validate_link("Work", "https://example.com/cal.ics")
                .unwrap()
                .1,
            "https://example.com/cal.ics"
        );
        assert_eq!(
            validate_link("Work", "webcal://example.com/cal.ics")
                .unwrap()
                .1,
            "https://example.com/cal.ics"
        );
        assert_eq!(
            validate_link("Work", "webcals://example.com/secret.ics")
                .unwrap()
                .1,
            "https://example.com/secret.ics"
        );
        let mut feeds = vec![
            Feed {
                id: "a".into(),
                name: "A".into(),
                color: "#5b8def".into(),
                order: 0,
            },
            Feed {
                id: "b".into(),
                name: "B".into(),
                color: "#3cba8b".into(),
                order: 1,
            },
        ];
        assert_eq!(next_color(&feeds), "#e0a045");
        move_feed(&mut feeds, "b", true).unwrap();
        assert!(
            feeds.iter().find(|feed| feed.id == "b").unwrap().order
                < feeds.iter().find(|feed| feed.id == "a").unwrap().order
        );
    }

    fn zoned(
        zone: Tz,
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> DateTime<Utc> {
        zone.with_ymd_and_hms(year, month, day, hour, minute, second)
            .earliest()
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn cjk_and_emoji_around_a_link_do_not_panic() {
        assert_eq!(
            first_meeting("参加：https://meet.google.com/abc-defg-hij")
                .unwrap()
                .provider,
            "Google Meet"
        );
        assert_eq!(
            first_meeting("🎉🎉 https://meet.google.com/abc-defg-hij")
                .unwrap()
                .url,
            "https://meet.google.com/abc-defg-hij"
        );
        let ics = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:bad\n\
DTSTART:参加参加参加\n\
SUMMARY:壊れ\n\
END:VEVENT\n\
BEGIN:VEVENT\n\
UID:cjk\n\
DTSTART:20261103T150000Z\n\
DTEND:20261103T160000Z\n\
SUMMARY:参加 🎉\n\
LOCATION:参加：https://meet.google.com/abc-defg-hij\n\
DESCRIPTION:🎉🎉 bring notes\n\
END:VEVENT\n\
END:VCALENDAR\n";
        let events = parse_at(ics, None, chrono_tz::UTC);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "参加 🎉");
        assert_eq!(events[0].provider, "Google Meet");
        assert_eq!(events[0].url, "https://meet.google.com/abc-defg-hij");
    }

    #[test]
    fn meeting_links_follow_the_parsed_host() {
        assert!(meeting_link("", "", "https://evil.example\\@meet.google.com/abc", "").is_none());
        assert!(
            meeting_link(
                "",
                "",
                "https://evil.example@meet.google.com/abc-defg-hij",
                ""
            )
            .is_none()
        );
        assert!(classify("ftp://meet.google.com/abc-defg-hij").is_none());
        assert!(classify("javascript:alert(1)").is_none());
        assert_eq!(
            meeting_link("", "", "https://teams.microsoft.com/meet/abc?p=xyz", "")
                .unwrap()
                .provider,
            "Teams"
        );
        let safe = "https://nam10.safelinks.protection.outlook.com/?url=https%3A%2F%2Fmeet.google.com%2Fabc-defg-hij&data=05";
        let unwrapped = meeting_link("", "", safe, "").unwrap();
        assert_eq!(unwrapped.provider, "Google Meet");
        assert_eq!(unwrapped.url, "https://meet.google.com/abc-defg-hij");
        let google =
            "https://www.google.com/url?q=https%3A%2F%2Fteams.microsoft.com%2Fmeet%2Fabc%3Fp%3D1";
        let unwrapped = meeting_link("", "", google, "").unwrap();
        assert_eq!(unwrapped.provider, "Teams");
        assert_eq!(unwrapped.url, "https://teams.microsoft.com/meet/abc?p=1");
    }

    #[test]
    fn a_missing_local_time_is_skipped_before_count() {
        let ics = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:gap\n\
DTSTART;TZID=America/New_York:20260307T023000\n\
DTEND;TZID=America/New_York:20260307T033000\n\
RRULE:FREQ=DAILY;COUNT=3\n\
SUMMARY:Early\n\
END:VEVENT\n\
END:VCALENDAR\n";
        let events = parse_in(
            ics,
            None,
            utc(2026, 3, 8, 18, 0, 0),
            chrono_tz::America::New_York,
        );
        let starts: Vec<i64> = events.iter().map(|event| event.start_ms).collect();
        let zone = chrono_tz::America::New_York;
        assert_eq!(
            starts,
            vec![
                zoned(zone, 2026, 3, 7, 2, 30, 0).timestamp_millis(),
                zoned(zone, 2026, 3, 9, 2, 30, 0).timestamp_millis(),
                zoned(zone, 2026, 3, 10, 2, 30, 0).timestamp_millis(),
            ]
        );
    }

    #[test]
    fn an_ambiguous_time_uses_the_earlier_offset() {
        let zone = chrono_tz::America::New_York;
        let naive = NaiveDate::from_ymd_opt(2026, 11, 1)
            .unwrap()
            .and_hms_opt(1, 30, 0)
            .unwrap();
        let chrono::LocalResult::Ambiguous(early, late) = zone.from_local_datetime(&naive) else {
            panic!("01:30 should happen twice");
        };
        assert!(early < late);
        let ics = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:fall\n\
DTSTART;TZID=America/New_York:20261101T013000\n\
DTEND;TZID=America/New_York:20261101T023000\n\
SUMMARY:Fallback\n\
END:VEVENT\n\
END:VCALENDAR\n";
        let events = parse_in(ics, None, utc(2026, 11, 1, 18, 0, 0), zone);
        assert_eq!(events[0].start_ms, early.timestamp_millis());
    }

    #[test]
    fn a_one_day_duration_ends_at_the_same_local_time() {
        let ics = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:p1d\n\
DTSTART;TZID=America/Denver:20260307T090000\n\
DURATION:P1D\n\
SUMMARY:Day\n\
END:VEVENT\n\
END:VCALENDAR\n";
        let zone = chrono_tz::America::Denver;
        let events = parse_in(ics, None, utc(2026, 3, 7, 18, 0, 0), zone);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].start_ms,
            zoned(zone, 2026, 3, 7, 9, 0, 0).timestamp_millis()
        );
        assert_eq!(
            events[0].end_ms,
            zoned(zone, 2026, 3, 8, 9, 0, 0).timestamp_millis()
        );
    }

    #[test]
    fn floating_times_use_the_offset_on_their_own_date() {
        let ics = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:float\n\
DTSTART:20260307T090000\n\
DTEND:20260308T090000\n\
SUMMARY:Float\n\
END:VEVENT\n\
END:VCALENDAR\n";
        let zone = chrono_tz::America::Denver;
        let events = parse_in(ics, None, utc(2026, 3, 7, 18, 0, 0), zone);
        assert_eq!(
            events[0].start_ms,
            zoned(zone, 2026, 3, 7, 9, 0, 0).timestamp_millis()
        );
        assert_eq!(
            events[0].end_ms,
            zoned(zone, 2026, 3, 8, 9, 0, 0).timestamp_millis()
        );
    }

    #[test]
    fn selectors_limit_the_series() {
        let last = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:last\n\
DTSTART:20261102T150000Z\n\
DTEND:20261102T160000Z\n\
RRULE:FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1\n\
SUMMARY:Last weekday\n\
END:VEVENT\n\
END:VCALENDAR\n";
        let events = parse_in(last, None, utc(2026, 11, 30, 18, 0, 0), chrono_tz::UTC);
        assert_eq!(
            events
                .iter()
                .map(|event| event.start_ms)
                .collect::<Vec<_>>(),
            vec![utc(2026, 11, 30, 15, 0, 0).timestamp_millis()]
        );

        let firsts = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:firsts\n\
DTSTART:20261101T150000Z\n\
DTEND:20261101T160000Z\n\
RRULE:FREQ=DAILY;BYMONTHDAY=1\n\
SUMMARY:Month day\n\
END:VEVENT\n\
END:VCALENDAR\n";
        let events = parse_in(firsts, None, utc(2026, 11, 1, 18, 0, 0), chrono_tz::UTC);
        assert_eq!(
            events
                .iter()
                .map(|event| event.start_ms)
                .collect::<Vec<_>>(),
            vec![utc(2026, 11, 1, 15, 0, 0).timestamp_millis()]
        );

        let both = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:both\n\
DTSTART:20261102T150000Z\n\
DTEND:20261102T160000Z\n\
RRULE:FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYMONTHDAY=3\n\
SUMMARY:Third\n\
END:VEVENT\n\
END:VCALENDAR\n";
        let events = parse_at(both, None, chrono_tz::UTC);
        assert_eq!(
            events
                .iter()
                .map(|event| event.start_ms)
                .collect::<Vec<_>>(),
            vec![utc(2026, 11, 3, 15, 0, 0).timestamp_millis()]
        );

        let hours = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:hour\n\
DTSTART:20261102T150000Z\n\
DTEND:20261102T160000Z\n\
RRULE:FREQ=DAILY;BYHOUR=9\n\
SUMMARY:Morning\n\
END:VEVENT\n\
END:VCALENDAR\n";
        assert!(parse_at(hours, None, chrono_tz::UTC).is_empty());

        assert_eq!(NaiveDate::from_ymd_opt(2026, 11, 3).unwrap().ordinal(), 307);
        let year_day = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:yday\n\
DTSTART:20261101T150000Z\n\
DTEND:20261101T160000Z\n\
RRULE:FREQ=DAILY;BYYEARDAY=307\n\
SUMMARY:Ordinal\n\
END:VEVENT\n\
END:VCALENDAR\n";
        let events = parse_at(year_day, None, chrono_tz::UTC);
        assert_eq!(
            events
                .iter()
                .map(|event| event.start_ms)
                .collect::<Vec<_>>(),
            vec![utc(2026, 11, 3, 15, 0, 0).timestamp_millis()]
        );
    }

    #[test]
    fn an_unknown_recurrence_part_keeps_the_first_time_and_rdates() {
        let ics = "\
BEGIN:VCALENDAR\n\
BEGIN:VEVENT\n\
UID:part\n\
DTSTART:20261102T150000Z\n\
DTEND:20261102T160000Z\n\
RRULE:FREQ=DAILY;BYSECOND=0\n\
RDATE:20261104T150000Z\n\
SUMMARY:Limited\n\
END:VEVENT\n\
END:VCALENDAR\n";
        let events = parse_at(ics, None, chrono_tz::UTC);
        assert_eq!(
            events
                .iter()
                .map(|event| event.start_ms)
                .collect::<Vec<_>>(),
            vec![
                utc(2026, 11, 2, 15, 0, 0).timestamp_millis(),
                utc(2026, 11, 4, 15, 0, 0).timestamp_millis(),
            ]
        );
    }

    #[test]
    fn week_numbers_match_iso_when_weeks_start_on_monday() {
        for (year, month, day) in [(2023, 1, 1), (2025, 12, 29), (2026, 1, 1), (2026, 11, 3)] {
            let date = NaiveDate::from_ymd_opt(year, month, day).unwrap();
            let iso = date.iso_week();
            assert_eq!(
                week_no(date, Weekday::Mon),
                Some((iso.year(), iso.week() as i32))
            );
        }
    }
}
