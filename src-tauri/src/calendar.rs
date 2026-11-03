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
    DateTime, Datelike, Days, Duration as ChronoDuration, FixedOffset, Local, LocalResult,
    NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc, Weekday,
};
use chrono_tz::Tz;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;
use crate::util::now_ms;

const FRESH_MS: u64 = 5 * 60 * 1000;
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
                results.push((id.clone(), fetch_feed(id)));
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
    let ics = tauri::async_runtime::block_on(fetch_ics(&url))?;
    let (now, offset) = local_clock();
    let mut events = parse_events(&ics, owner.as_deref(), now, offset)?;
    if events.len() > 300 {
        events.truncate(300);
    }
    Ok(events)
}

fn local_clock() -> (DateTime<Utc>, FixedOffset) {
    (Utc::now(), *Local::now().offset())
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
    let bytes = response
        .bytes()
        .await
        .map_err(|_| "Couldn't read the calendar".to_string())?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("The calendar is too large".into());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
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

/// A name and an http(s) link, trimmed. The link is a secret and is not kept in settings.
pub fn validate_link(name: &str, url: &str) -> Result<(String, String), String> {
    let name = validate_name(name)?;
    let url = url.trim();
    if url.chars().count() > 2000 {
        return Err("That link is too long".into());
    }
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("The link has to start with https://".into());
    }
    if url.chars().any(char::is_whitespace) {
        return Err("That link has a space in it".into());
    }
    Ok((name, url.to_string()))
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
    wkst: Weekday,
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
    duration: Option<ChronoDuration>,
    anchor: Anchor,
    rule: Option<Rule>,
    exdates: Vec<Stamp>,
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
    offset: FixedOffset,
) -> Result<Vec<Event>, String> {
    if !looks_like_ics(ics) {
        return Err("That link isn't an iCal feed".into());
    }
    let window = window_for(now, offset);
    let mut events = Vec::new();
    for group in group_events(read_events(ics, owner, offset)) {
        if group.master.declined || is_cancelled(&group.master.status) {
            continue;
        }
        events.extend(expand_group(
            &group.master,
            &group.overrides,
            &window,
            offset,
        ));
    }
    events.sort_by(|a, b| a.start_ms.cmp(&b.start_ms).then(a.title.cmp(&b.title)));
    Ok(events)
}

fn looks_like_ics(text: &str) -> bool {
    let lower = text.trim_start().to_ascii_lowercase();
    lower.contains("begin:vcalendar") || lower.contains("begin:vevent")
}

fn window_for(now: DateTime<Utc>, offset: FixedOffset) -> Window {
    let today = now.with_timezone(&offset).date_naive();
    let start_date = today.checked_sub_days(Days::new(1)).unwrap_or(today);
    let end_date = today.checked_add_days(Days::new(2)).unwrap_or(today);
    Window {
        start: at_local(start_date, NaiveTime::MIN, offset).unwrap_or(now),
        end: at_local(
            end_date,
            NaiveTime::from_hms_opt(23, 59, 59).unwrap_or(NaiveTime::MIN),
            offset,
        )
        .unwrap_or(now),
        start_date,
        end_date,
    }
}

fn at_local(date: NaiveDate, time: NaiveTime, offset: FixedOffset) -> Option<DateTime<Utc>> {
    let naive = date.and_time(time);
    match offset.from_local_datetime(&naive) {
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

fn read_events(ics: &str, owner: Option<&str>, offset: FixedOffset) -> Vec<RawEvent> {
    let mut events = Vec::new();
    for props in extract_vevents(ics) {
        if let Some(raw) = build_raw(&props, owner, offset) {
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
            out.push_str(&line[1..]);
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
        value[1..value.len() - 1].to_string()
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

fn build_raw(props: &[ContentLine], owner: Option<&str>, offset: FixedOffset) -> Option<RawEvent> {
    let start_prop = props
        .iter()
        .find(|(name, _, _)| name.eq_ignore_ascii_case("DTSTART"))?;
    let anchor = anchor_of(&start_prop.1, &start_prop.2);
    let start = parse_stamp(&start_prop.2, &start_prop.1, anchor, offset)?;
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
        recurrence_id: None,
    };
    for (name, params, value) in props {
        if name.eq_ignore_ascii_case("DTSTART") {
            continue;
        } else if name.eq_ignore_ascii_case("DTEND") {
            raw.end = parse_stamp(value, params, anchor, offset);
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
            raw.rule = parse_rule(value, anchor, offset);
        } else if name.eq_ignore_ascii_case("EXDATE") {
            raw.exdates
                .extend(parse_stamp_list(value, params, anchor, offset));
        } else if name.eq_ignore_ascii_case("RECURRENCE-ID") {
            raw.recurrence_id = parse_stamp(value, params, anchor, offset);
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
    value
        .get(7..)
        .filter(|_| value.len() >= 7 && value[..7].eq_ignore_ascii_case("mailto:"))
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
    offset: FixedOffset,
) -> Vec<Stamp> {
    value
        .split(',')
        .filter_map(|part| parse_stamp(part, params, anchor, offset))
        .collect()
}

fn parse_stamp(
    value: &str,
    params: &[(String, String)],
    anchor: Anchor,
    offset: FixedOffset,
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
        return Some(Stamp::Instant(from_naive(naive, Anchor::Zoned(tz), offset)));
    }
    Some(Stamp::Instant(from_naive(naive, anchor, offset)))
}

fn parse_date(value: &str) -> Option<NaiveDate> {
    if value.len() < 8 {
        return None;
    }
    let year: i32 = value[0..4].parse().ok()?;
    let month: u32 = value[4..6].parse().ok()?;
    let day: u32 = value[6..8].parse().ok()?;
    NaiveDate::from_ymd_opt(year, month, day)
}

fn parse_naive(value: &str) -> Option<NaiveDateTime> {
    let value = value.trim_end_matches(['Z', 'z']);
    let (date, time) = value.split_once('T')?;
    let date = parse_date(date)?;
    if time.len() < 6 {
        return None;
    }
    let hour: u32 = time[0..2].parse().ok()?;
    let minute: u32 = time[2..4].parse().ok()?;
    let second: u32 = time[4..6].parse().ok()?;
    date.and_hms_opt(hour, minute, second)
}

fn parse_duration(value: &str) -> Option<ChronoDuration> {
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
            rest = &rest[1..];
            continue;
        }
        let digits = rest
            .find(|ch: char| !ch.is_ascii_digit())
            .unwrap_or(rest.len());
        if digits == 0 {
            return None;
        }
        let count: i64 = rest[..digits].parse().ok()?;
        let unit = rest[digits..].chars().next()?;
        rest = &rest[digits + unit.len_utf8()..];
        match unit.to_ascii_uppercase() {
            'W' => weeks = count,
            'D' => days = count,
            'H' => hours = count,
            'M' if time => minutes = count,
            'M' => days += count.saturating_mul(30),
            'S' => seconds = count,
            'Y' => days += count.saturating_mul(365),
            _ => return None,
        }
    }
    Some(
        ChronoDuration::days(weeks.saturating_mul(7).saturating_add(days))
            + ChronoDuration::hours(hours)
            + ChronoDuration::minutes(minutes)
            + ChronoDuration::seconds(seconds),
    )
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

fn from_naive(naive: NaiveDateTime, anchor: Anchor, offset: FixedOffset) -> DateTime<Utc> {
    match anchor {
        Anchor::Zoned(tz) => zone_to_utc(naive, &tz),
        Anchor::Utc => naive.and_utc(),
        Anchor::Floating => match offset.from_local_datetime(&naive) {
            LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc),
            LocalResult::None => naive.and_utc(),
        },
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

fn to_naive(instant: DateTime<Utc>, anchor: Anchor, offset: FixedOffset) -> NaiveDateTime {
    match anchor {
        Anchor::Zoned(tz) => instant.with_timezone(&tz).naive_local(),
        Anchor::Utc => instant.naive_utc(),
        Anchor::Floating => instant.with_timezone(&offset).naive_local(),
    }
}

fn parse_rule(value: &str, anchor: Anchor, offset: FixedOffset) -> Option<Rule> {
    let mut freq = None;
    let mut interval = 1i64;
    let mut count = None;
    let mut until = None;
    let mut by_day = Vec::new();
    let mut by_month_day = Vec::new();
    let mut by_month = Vec::new();
    let mut wkst = Weekday::Mon;
    for part in value.split(';') {
        let Some((key, val)) = part.split_once('=') else {
            continue;
        };
        match key.to_ascii_uppercase().as_str() {
            "FREQ" => freq = parse_freq(val),
            "INTERVAL" => interval = val.parse::<i64>().unwrap_or(1).max(1),
            "COUNT" => count = val.parse().ok(),
            "UNTIL" => until = parse_stamp(val, &[], anchor, offset),
            "BYDAY" => by_day = parse_byday(val),
            "BYMONTHDAY" => by_month_day = parse_ints(val),
            "BYMONTH" => {
                by_month = parse_ints(val)
                    .into_iter()
                    .filter(|month| (1..=12).contains(month))
                    .map(|month| month as u32)
                    .collect();
            }
            "WKST" => wkst = weekday_token(val).unwrap_or(Weekday::Mon),
            _ => {}
        }
    }
    Some(Rule {
        freq: freq?,
        interval,
        count,
        until,
        by_day,
        by_month_day,
        by_month,
        wkst,
    })
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
    if token.len() < 2 {
        return None;
    }
    if token.len() == 2 {
        return Some(ByDay {
            nth: None,
            weekday: weekday_token(token)?,
        });
    }
    let (nth, day) = token.split_at(token.len() - 2);
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
    offset: FixedOffset,
) -> Vec<Event> {
    let Some((start, end)) = span(master) else {
        return Vec::new();
    };
    let duration = positive_duration(duration_between(start, end));
    let mut produced = match &master.rule {
        Some(rule) => generate(master, rule, window, offset),
        None => vec![start],
    };
    produced.retain(|stamp| {
        !master
            .exdates
            .iter()
            .any(|ex| same_stamp(*stamp, *ex, master.anchor, offset))
    });
    let mut events = Vec::new();
    for stamp in produced {
        if overrides.iter().any(|extra| {
            extra
                .recurrence_id
                .is_some_and(|id| same_stamp(stamp, id, master.anchor, offset))
        }) {
            continue;
        }
        let end = add_duration(stamp, duration);
        if let Some(event) = build_event(master, stamp, end, window, offset) {
            events.push(event);
        }
    }
    for extra in overrides {
        if is_cancelled(&extra.status) || extra.declined {
            continue;
        }
        let merged = overlay(master, extra);
        let Some((start, end)) = span(&merged) else {
            continue;
        };
        let end = if extra.end.is_none() && extra.duration.is_none() {
            add_duration(start, duration)
        } else {
            end
        };
        if let Some(event) = build_event(&merged, start, end, window, offset) {
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
    event
}

fn span(raw: &RawEvent) -> Option<(Stamp, Stamp)> {
    let start = raw.start;
    if let Some(end) = raw.end {
        return Some((start, end));
    }
    if let Some(duration) = raw.duration {
        return Some((start, add_duration(start, duration)));
    }
    let end = match start {
        Stamp::Date(date) => Stamp::Date(date.checked_add_days(Days::new(1)).unwrap_or(date)),
        Stamp::Instant(instant) => Stamp::Instant(instant),
    };
    Some((start, end))
}

fn duration_between(start: Stamp, end: Stamp) -> ChronoDuration {
    match (start, end) {
        (Stamp::Instant(start), Stamp::Instant(end)) => end.signed_duration_since(start),
        (Stamp::Date(start), Stamp::Date(end)) => end.signed_duration_since(start),
        _ => ChronoDuration::zero(),
    }
}

fn positive_duration(duration: ChronoDuration) -> ChronoDuration {
    if duration < ChronoDuration::zero() {
        ChronoDuration::zero()
    } else {
        duration
    }
}

fn add_duration(start: Stamp, duration: ChronoDuration) -> Stamp {
    match start {
        Stamp::Instant(instant) => Stamp::Instant(instant + duration),
        Stamp::Date(date) => {
            let days = duration.num_days().max(0) as u64;
            let days = if days == 0 && duration > ChronoDuration::zero() {
                1
            } else {
                days
            };
            Stamp::Date(date.checked_add_days(Days::new(days)).unwrap_or(date))
        }
    }
}

fn same_stamp(a: Stamp, b: Stamp, anchor: Anchor, offset: FixedOffset) -> bool {
    match (a, b) {
        (Stamp::Date(a), Stamp::Date(b)) => a == b,
        (Stamp::Instant(a), Stamp::Instant(b)) => a.timestamp() == b.timestamp(),
        (Stamp::Date(date), Stamp::Instant(instant))
        | (Stamp::Instant(instant), Stamp::Date(date)) => {
            to_naive(instant, anchor, offset).date() == date
        }
    }
}

fn generate(master: &RawEvent, rule: &Rule, window: &Window, offset: FixedOffset) -> Vec<Stamp> {
    let Some(start_naive) = stamp_naive(master.start, master.anchor, offset) else {
        return vec![master.start];
    };
    let hard_end = rule
        .until
        .and_then(|until| stamp_naive(until, master.anchor, offset))
        .unwrap_or_else(|| to_naive(window.end, master.anchor, offset));
    let hard_end = hard_end.min(to_naive(window.end, master.anchor, offset));
    let not_before = to_naive(
        window.start - ChronoDuration::days(2),
        master.anchor,
        offset,
    );
    let mut times = match rule.freq {
        Freq::Minutely => gen_stepped(
            start_naive,
            hard_end,
            not_before,
            ChronoDuration::minutes(rule.interval),
            rule,
            true,
        ),
        Freq::Hourly => gen_stepped(
            start_naive,
            hard_end,
            not_before,
            ChronoDuration::hours(rule.interval),
            rule,
            true,
        ),
        Freq::Daily => gen_stepped(
            start_naive,
            hard_end,
            not_before,
            ChronoDuration::days(rule.interval),
            rule,
            false,
        ),
        Freq::Weekly => gen_weekly(start_naive, hard_end, rule),
        Freq::Monthly => gen_monthly(start_naive, hard_end, rule),
        Freq::Yearly => gen_yearly(start_naive, hard_end, rule),
    };
    ensure_start(&mut times, start_naive, rule.count, hard_end);
    times.sort();
    times.dedup();
    times
        .into_iter()
        .map(|naive| {
            if matches!(master.start, Stamp::Date(_)) {
                Stamp::Date(naive.date())
            } else {
                Stamp::Instant(from_naive(naive, master.anchor, offset))
            }
        })
        .collect()
}

fn stamp_naive(stamp: Stamp, anchor: Anchor, offset: FixedOffset) -> Option<NaiveDateTime> {
    Some(match stamp {
        Stamp::Instant(instant) => to_naive(instant, anchor, offset),
        Stamp::Date(date) => date.and_time(NaiveTime::MIN),
    })
}

fn ensure_start(
    times: &mut Vec<NaiveDateTime>,
    start: NaiveDateTime,
    count: Option<u32>,
    hard_end: NaiveDateTime,
) {
    if start > hard_end || times.contains(&start) {
        return;
    }
    times.insert(0, start);
    if let Some(max) = count {
        times.truncate(max as usize);
    }
}

fn filters_empty(rule: &Rule) -> bool {
    rule.by_day.is_empty() && rule.by_month_day.is_empty() && rule.by_month.is_empty()
}

fn gen_stepped(
    start: NaiveDateTime,
    hard_end: NaiveDateTime,
    not_before: NaiveDateTime,
    step: ChronoDuration,
    rule: &Rule,
    jump: bool,
) -> Vec<NaiveDateTime> {
    let mut out = Vec::new();
    let mut seen = 0u32;
    let mut cursor = start;
    if jump && filters_empty(rule) && start < not_before {
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
        if day_ok(cursor, rule) {
            seen += 1;
            if rule.count.is_some_and(|max| seen > max) {
                break;
            }
            out.push(cursor);
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

fn day_ok(time: NaiveDateTime, rule: &Rule) -> bool {
    if !rule.by_month.is_empty() && !rule.by_month.contains(&time.month()) {
        return false;
    }
    if rule.by_day.is_empty() {
        return true;
    }
    rule.by_day.iter().any(|day| day.weekday == time.weekday())
}

fn gen_weekly(start: NaiveDateTime, hard_end: NaiveDateTime, rule: &Rule) -> Vec<NaiveDateTime> {
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
            if time >= start && time <= hard_end && month_ok(time, rule) {
                batch.push(time);
            }
        }
        batch.sort();
        batch.dedup();
        let mut stop = false;
        for time in batch {
            seen += 1;
            if rule.count.is_some_and(|max| seen > max) {
                stop = true;
                break;
            }
            out.push(time);
        }
        if stop {
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

fn month_ok(time: NaiveDateTime, rule: &Rule) -> bool {
    rule.by_month.is_empty() || rule.by_month.contains(&time.month())
}

fn gen_monthly(start: NaiveDateTime, hard_end: NaiveDateTime, rule: &Rule) -> Vec<NaiveDateTime> {
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
        if month_ok_month(month, rule)
            && !push_month(&mut out, &mut seen, year, month, start, hard_end, rule)
        {
            break;
        }
        step += rule.interval as i32;
    }
    out
}

fn gen_yearly(start: NaiveDateTime, hard_end: NaiveDateTime, rule: &Rule) -> Vec<NaiveDateTime> {
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
        let mut stop = false;
        for month in months {
            if !push_month(&mut out, &mut seen, year, month, start, hard_end, rule) {
                stop = true;
                break;
            }
        }
        if stop {
            break;
        }
        year += rule.interval as i32;
    }
    out
}

fn month_ok_month(month: u32, rule: &Rule) -> bool {
    rule.by_month.is_empty() || rule.by_month.contains(&month)
}

fn push_month(
    out: &mut Vec<NaiveDateTime>,
    seen: &mut u32,
    year: i32,
    month: u32,
    start: NaiveDateTime,
    hard_end: NaiveDateTime,
    rule: &Rule,
) -> bool {
    let mut batch = Vec::new();
    for date in month_dates(year, month, start, rule) {
        let time = date.and_time(start.time());
        if time >= start && time <= hard_end {
            batch.push(time);
        }
    }
    batch.sort();
    batch.dedup();
    for time in batch {
        *seen += 1;
        if rule.count.is_some_and(|max| *seen > max) {
            return false;
        }
        out.push(time);
    }
    true
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

fn build_event(
    raw: &RawEvent,
    start: Stamp,
    end: Stamp,
    window: &Window,
    offset: FixedOffset,
) -> Option<Event> {
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
            let _ = offset;
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
        let Some(at) = next_marker(&text[from..]) else {
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
    if start >= 8 && text[start - 8..start].eq_ignore_ascii_case("https://") {
        start -= 8;
    } else if start >= 7 && text[start - 7..start].eq_ignore_ascii_case("http://") {
        start -= 7;
    }
    let mut end = start;
    while end < bytes.len() && !is_url_end(bytes[end]) {
        end += 1;
    }
    while end > start && matches!(bytes[end - 1], b'.' | b',' | b';' | b')' | b']') {
        end -= 1;
    }
    let raw = &text[start..end];
    let url = if raw.len() >= 7 && raw[..7].eq_ignore_ascii_case("http://")
        || raw.len() >= 8 && raw[..8].eq_ignore_ascii_case("https://")
    {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };
    (url, end.max(marker_at.saturating_add(1)))
}

fn is_url_host(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')
}

fn is_url_end(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'<' | b'>' | b'"' | b'\'' | b'|')
}

fn classify(url: &str) -> Option<Meeting> {
    let rest = if url.len() >= 8 && url[..8].eq_ignore_ascii_case("https://") {
        &url[8..]
    } else if url.len() >= 7 && url[..7].eq_ignore_ascii_case("http://") {
        &url[7..]
    } else {
        return None;
    };
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.split('@').next_back().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    let host = host.to_ascii_lowercase();
    let path_l = path.to_ascii_lowercase();
    let provider = if host == "meet.google.com" && !path.is_empty() {
        "Google Meet"
    } else if (host == "zoom.us" || host.ends_with(".zoom.us"))
        && (path_l.starts_with("j/")
            || path_l.starts_with("my/")
            || path_l.starts_with("s/")
            || path_l.starts_with("w/")
            || path_l.starts_with("wc/"))
    {
        "Zoom"
    } else if (host == "teams.microsoft.com" && path_l.starts_with("l/meetup-join/"))
        || (host == "teams.live.com" && path_l.starts_with("meet/"))
    {
        "Teams"
    } else if (host == "webex.com" || host.ends_with(".webex.com")) && !path.is_empty() {
        "Webex"
    } else {
        return None;
    };
    Some(Meeting {
        url: url.to_string(),
        provider,
    })
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
            let local = &text[start..index];
            let domain = &text[index + 1..end];
            if !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
            {
                return Some(text[start..end].to_ascii_lowercase());
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

    fn parse_at(ics: &str, owner: Option<&str>, offset_hours: i32) -> Vec<Event> {
        let now = utc(2026, 11, 3, 18, 0, 0);
        let offset = FixedOffset::east_opt(offset_hours * 3600).unwrap();
        parse_events(ics, owner, now, offset).unwrap()
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
        let events = parse_at(ics, None, 0);
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
        let events = parse_at(ics, None, 0);
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
        let events = parse_at(new_york, None, 0);
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
        let events = parse_at(pacific, None, 0);
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
        let events = parse_at(floating, None, -5);
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
        let events = parse_at(ics, None, 0);
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
RRULE:FREQ=WEEKLY;BYDAY=MO,WE\n\
EXDATE:20261109T150000Z\n\
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
        let events = parse_at(ics, None, 0);
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
        let events = parse_at(ics, None, 0);
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
        let with_owner = parse_at(ics, Some("me@example.com"), 0);
        assert_eq!(titles(&with_owner), ["Keep other"]);
        let without = parse_at(ics, None, 0);
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
        let events = parse_at(ics, None, 0);
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
        assert_eq!(titles(&parse_at(ics, None, 0)), ["Soon"]);
    }

    #[test]
    fn a_page_that_is_not_a_feed_is_an_error() {
        let err = parse_events(
            "<html>login</html>",
            None,
            utc(2026, 11, 3, 18, 0, 0),
            FixedOffset::east_opt(0).unwrap(),
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
        assert!(validate_link("Work", "https://example.com/cal.ics").is_ok());
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
}
