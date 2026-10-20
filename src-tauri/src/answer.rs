//! Inline answers: math, unit conversions and time zones, worked out on the keystroke and
//! shown as the top row. Enter copies the answer.

use std::time::{Duration, Instant};

use chrono::{
    DateTime, Datelike, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeZone, Utc,
};
use chrono_tz::Tz;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub value: String,
    pub hint: &'static str,
}

/// Answers `input` if it is clearly a calculation or a time question. Plain searches,
/// including ones that happen to contain numbers, get nothing.
pub fn inline(input: &str, now: DateTime<Utc>) -> Option<Answer> {
    let input = input.trim();
    if input.is_empty() || input.len() > 200 {
        return None;
    }
    let local = chrono::Local
        .offset_from_utc_datetime(&now.naive_utc())
        .fix();
    clock_answer(input, now, local)
        .or_else(|| time_answer(input, now))
        .or_else(|| calculate(input))
}

// Timestamps, week numbers and the like

/// `unix`, `1760000000 unix`, `epoch 1760000000123`, `2026-10-13 to unix`, `week`,
/// `day of year`, `now iso`, `timestamp filename`. `local` is this computer's UTC offset.
fn clock_answer(input: &str, now: DateTime<Utc>, local: FixedOffset) -> Option<Answer> {
    let lower = input.to_lowercase();
    let lower = lower.trim();
    let here = now.with_timezone(&local);
    let answer = |value: String| {
        Some(Answer {
            value,
            hint: "Time",
        })
    };
    match lower {
        "unix" | "epoch" | "timestamp" | "unix time" | "epoch time" | "unix timestamp" => {
            return answer(now.timestamp().to_string());
        }
        "week" | "week number" | "what week" | "what week is it" => {
            let week = here.iso_week();
            return answer(format!("Week {}, {}", week.week(), week.year()));
        }
        "day of year" | "day number" => {
            let days = if here.date_naive().leap_year() {
                366
            } else {
                365
            };
            return answer(format!("Day {} of {days}", here.ordinal()));
        }
        "now iso" | "iso now" | "iso time" | "iso 8601" => {
            return answer(here.format("%Y-%m-%dT%H:%M:%S%:z").to_string());
        }
        "timestamp filename" | "filename timestamp" => {
            return answer(here.format("%Y-%m-%d_%H-%M-%S").to_string());
        }
        _ => {}
    }

    // A number read as a Unix time, only when it says so: a bare 10-digit number is as
    // likely a phone number.
    let number = lower
        .strip_suffix(" unix")
        .or_else(|| lower.strip_suffix(" epoch"))
        .or_else(|| lower.strip_prefix("epoch "))
        .or_else(|| lower.strip_prefix("unix "))
        .map(str::trim);
    if let Some(number) = number
        && !number.is_empty()
        && number.bytes().all(|b| b.is_ascii_digit())
    {
        let seconds: i64 = match number.len() {
            10 => number.parse().ok()?,
            13 => number.parse::<i64>().ok()? / 1000,
            _ => return None,
        };
        let at = DateTime::<Utc>::from_timestamp(seconds, 0)?;
        let local_at = at.with_timezone(&local);
        return answer(format!(
            "{} ({})",
            local_at.format("%a %b %-d %Y, %-I:%M:%S %p"),
            at.format("%Y-%m-%dT%H:%M:%SZ")
        ));
    }

    // A date (or date and time, in this computer's time zone) as Unix seconds.
    let date = lower
        .strip_suffix(" to unix")
        .or_else(|| lower.strip_suffix(" to epoch"))
        .or_else(|| lower.strip_suffix(" in unix"))?
        .trim();
    let parsed = NaiveDateTime::parse_from_str(date, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(date, "%Y-%m-%d %H:%M"))
        .or_else(|_| NaiveDateTime::parse_from_str(date, "%Y-%m-%dt%H:%M:%S"))
        .or_else(|_| NaiveDateTime::parse_from_str(date, "%Y-%m-%dt%H:%M"))
        .or_else(|_| {
            NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .map(|day| day.and_hms_opt(0, 0, 0).unwrap_or_default())
        })
        .ok()?;
    let at = local.from_local_datetime(&parsed).single()?;
    answer(at.timestamp().to_string())
}

// Calculator

struct Deadline(Instant);

impl fend_core::Interrupt for Deadline {
    fn should_interrupt(&self) -> bool {
        Instant::now() >= self.0
    }
}

fn calculate(input: &str) -> Option<Answer> {
    if !looks_like_math(input) {
        return None;
    }
    let context = fend_core::Context::new();
    let deadline = Deadline(Instant::now() + Duration::from_millis(40));
    let result = fend_core::evaluate_preview_with_interrupt(input, &context, &deadline);
    if result.output_is_empty() {
        return None;
    }
    let value = tidy(result.get_main_result());
    let echoed =
        value.eq_ignore_ascii_case(input) || value.replace(' ', "") == input.replace(' ', "");
    if value.is_empty() || echoed || value.contains('\n') {
        return None;
    }
    let hint = if is_conversion(input) {
        "Conversion"
    } else {
        "Calculator"
    };
    Some(Answer { value, hint })
}

/// Drops fend's "approx." and rounds long decimals so the answer reads like one.
fn tidy(raw: &str) -> String {
    let raw = raw.trim();
    let raw = raw.strip_prefix("approx. ").unwrap_or(raw);
    let (number, rest) = raw.split_once(' ').unwrap_or((raw, ""));
    let long_decimal = number.split_once('.').is_some_and(|(_, decimals)| {
        decimals.len() > 4 && decimals.chars().all(|ch| ch.is_ascii_digit())
    });
    let number = match number.parse::<f64>() {
        Ok(parsed) if long_decimal => {
            let rounded = format!("{parsed:.4}");
            rounded
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string()
        }
        _ => number.to_string(),
    };
    if rest.is_empty() {
        number
    } else {
        format!("{number} {rest}")
    }
}

/// A digit plus an operator, function or conversion. Phone numbers, years, version strings
/// and part numbers are left for the search they were typed for.
fn looks_like_math(input: &str) -> bool {
    if !input.chars().any(|ch| ch.is_ascii_digit()) {
        return false;
    }
    let lower = input.to_lowercase();
    if is_conversion(&lower) {
        return true;
    }
    if input
        .chars()
        .all(|ch| ch.is_ascii_digit() || matches!(ch, '-' | ' ' | '(' | ')' | '.' | '+'))
        && input.matches('-').count() >= 2
    {
        return false; // 1-800-555-0100, 2026-10-05
    }
    if input.chars().filter(|ch| *ch == '.').count() >= 2 {
        return false; // 1.2.3, 192.168.0.1
    }
    let operator = input
        .chars()
        .any(|ch| matches!(ch, '+' | '*' | '/' | '^' | '%' | '×' | '÷'))
        || input.contains(" - ")
        || input.char_indices().any(|(at, ch)| {
            ch == '-' && at > 0 && input[..at].ends_with(|c: char| c.is_ascii_digit() || c == ')')
        });
    let function = ["sqrt", "sin", "cos", "tan", "log", "ln", "abs", "pi"]
        .iter()
        .any(|name| lower.contains(name));
    operator || function
}

fn is_conversion(lower: &str) -> bool {
    lower.contains(" to ") || lower.contains(" in ") || lower.contains(" as ")
}

// Time zones

/// `time in tokyo`, `tokyo time`, `3pm pst to cet`, `15:30 london in new york`.
fn time_answer(input: &str, now: DateTime<Utc>) -> Option<Answer> {
    let lower = input.to_lowercase();
    let place = lower
        .strip_prefix("time in ")
        .or_else(|| lower.strip_prefix("now in "))
        .or_else(|| lower.strip_suffix(" time"));
    if let Some(place) = place {
        let zone = zone(place.trim())?;
        let local = now.with_timezone(&zone);
        return Some(Answer {
            value: format!(
                "{} {}",
                local.format("%-I:%M %p, %a %b %-d"),
                local.format("%Z")
            ),
            hint: "Time",
        });
    }

    let (from, to) = lower
        .split_once(" to ")
        .or_else(|| lower.split_once(" in "))?;
    let (clock, from_zone) = split_clock(from.trim())?;
    let from_zone = zone(from_zone)?;
    let to_zone = zone(to.trim())?;
    let today = now.with_timezone(&from_zone).date_naive();
    let start = from_zone
        .from_local_datetime(&today.and_time(clock))
        .earliest()?;
    let converted = start.with_timezone(&to_zone);
    let day = if converted.date_naive() == start.date_naive() {
        String::new()
    } else {
        format!(", {}", converted.format("%a"))
    };
    Some(Answer {
        value: format!(
            "{}{day} {}",
            converted.format("%-I:%M %p"),
            converted.format("%Z")
        ),
        hint: "Time",
    })
}

/// Splits `3pm pst` or `15:30 london` into the time and the place.
fn split_clock(text: &str) -> Option<(NaiveTime, &str)> {
    let (first, rest) = text.split_once(' ')?;
    let (clock, rest) = match rest.split_once(' ') {
        Some((meridiem @ ("am" | "pm"), place)) => (format!("{first}{meridiem}"), place),
        _ => (first.to_string(), rest),
    };
    Some((parse_clock(&clock)?, rest.trim()))
}

fn parse_clock(clock: &str) -> Option<NaiveTime> {
    let (digits, offset) = if let Some(digits) = clock.strip_suffix("pm") {
        (digits, 12)
    } else if let Some(digits) = clock.strip_suffix("am") {
        (digits, 0)
    } else {
        (clock, -1)
    };
    let (hour, minute) = match digits.split_once(':') {
        Some((hour, minute)) => (hour.parse::<u32>().ok()?, minute.parse::<u32>().ok()?),
        None => (digits.parse::<u32>().ok()?, 0),
    };
    let hour = match offset {
        -1 if hour < 24 => hour,
        12 if (1..=12).contains(&hour) => hour % 12 + 12,
        0 if (1..=12).contains(&hour) => hour % 12,
        _ => return None,
    };
    NaiveTime::from_hms_opt(hour, minute, 0)
}

const ABBREVIATIONS: &[(&str, Tz)] = &[
    ("utc", Tz::UTC),
    ("gmt", Tz::Etc__GMT),
    ("pst", Tz::America__Los_Angeles),
    ("pdt", Tz::America__Los_Angeles),
    ("pt", Tz::America__Los_Angeles),
    ("mst", Tz::America__Denver),
    ("mdt", Tz::America__Denver),
    ("mt", Tz::America__Denver),
    ("cst", Tz::America__Chicago),
    ("cdt", Tz::America__Chicago),
    ("ct", Tz::America__Chicago),
    ("est", Tz::America__New_York),
    ("edt", Tz::America__New_York),
    ("et", Tz::America__New_York),
    ("bst", Tz::Europe__London),
    ("cet", Tz::Europe__Paris),
    ("cest", Tz::Europe__Paris),
    ("eet", Tz::Europe__Athens),
    ("ist", Tz::Asia__Kolkata),
    ("jst", Tz::Asia__Tokyo),
    ("kst", Tz::Asia__Seoul),
    ("aest", Tz::Australia__Sydney),
    ("sf", Tz::America__Los_Angeles),
    ("nyc", Tz::America__New_York),
    ("san francisco", Tz::America__Los_Angeles),
    ("seattle", Tz::America__Los_Angeles),
    ("boston", Tz::America__New_York),
    ("washington", Tz::America__New_York),
    ("dc", Tz::America__New_York),
    ("miami", Tz::America__New_York),
    ("atlanta", Tz::America__New_York),
    ("dallas", Tz::America__Chicago),
    ("houston", Tz::America__Chicago),
    ("austin", Tz::America__Chicago),
    ("salt lake city", Tz::America__Denver),
    ("beijing", Tz::Asia__Shanghai),
    ("mumbai", Tz::Asia__Kolkata),
    ("delhi", Tz::Asia__Kolkata),
    ("bangalore", Tz::Asia__Kolkata),
];

/// An abbreviation, a well-known city, or the city part of any IANA zone name.
fn zone(place: &str) -> Option<Tz> {
    let place = place.trim().trim_end_matches('?');
    if place.is_empty() {
        return None;
    }
    if let Some((_, zone)) = ABBREVIATIONS.iter().find(|(name, _)| *name == place) {
        return Some(*zone);
    }
    chrono_tz::TZ_VARIANTS.iter().copied().find(|zone| {
        let name = zone.name();
        name.eq_ignore_ascii_case(place)
            || name
                .rsplit('/')
                .next()
                .is_some_and(|city| city.replace('_', " ").eq_ignore_ascii_case(place))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 6, 3, 0, 0).unwrap()
    }

    fn value(input: &str) -> Option<String> {
        inline(input, now()).map(|answer| answer.value)
    }

    #[test]
    fn calculates() {
        assert_eq!(value("2+2").as_deref(), Some("4"));
        assert_eq!(value("12 * (3 + 4)").as_deref(), Some("84"));
        assert_eq!(value("sqrt 16").as_deref(), Some("4"));
    }

    #[test]
    fn converts_units() {
        assert_eq!(value("5 km to miles").as_deref(), Some("3.1069 miles"));
        assert_eq!(value("1 / 3").as_deref(), Some("0.3333"));
    }

    #[test]
    fn leaves_searches_alone() {
        for search in [
            "iphone 15",
            "covid-19",
            "2026",
            "1-800-555-0100",
            "2026-10-05",
            "192.168.0.1",
            "python 3.12",
            "hello world",
            "rust to go",
            "100 usd to eur",
        ] {
            assert_eq!(value(search), None, "{search}");
        }
    }

    #[test]
    fn tells_the_time_elsewhere() {
        assert_eq!(
            value("time in tokyo").as_deref(),
            Some("12:00 PM, Tue Oct 6 JST")
        );
        assert_eq!(
            value("london time").as_deref(),
            Some("4:00 AM, Tue Oct 6 BST")
        );
        assert_eq!(
            value("3pm pst to cet").as_deref(),
            Some("12:00 AM, Tue CEST")
        );
        assert_eq!(
            value("9:30 am new york in london").as_deref(),
            Some("2:30 PM BST")
        );
        assert_eq!(value("time in narnia"), None);
    }

    /// Clock answers as seen from a computer seven hours behind UTC (Denver in summer).
    fn clock(input: &str) -> Option<String> {
        let denver = FixedOffset::west_opt(7 * 3600).unwrap();
        clock_answer(input, now(), denver).map(|answer| answer.value)
    }

    #[test]
    fn gives_unix_times_both_ways() {
        assert_eq!(clock("unix").as_deref(), Some("1791255600"));
        assert_eq!(clock("Epoch").as_deref(), Some("1791255600"));
        assert_eq!(
            clock("1791255600 unix").as_deref(),
            Some("Mon Oct 5 2026, 8:00:00 PM (2026-10-06T03:00:00Z)")
        );
        assert_eq!(
            clock("epoch 1791255600123").as_deref(),
            Some("Mon Oct 5 2026, 8:00:00 PM (2026-10-06T03:00:00Z)")
        );
        assert_eq!(
            clock("2026-10-05 20:00 to unix").as_deref(),
            Some("1791255600")
        );
        assert_eq!(clock("2026-10-06 to unix").as_deref(), Some("1791270000"));
    }

    #[test]
    fn a_bare_long_number_is_not_a_unix_time() {
        for search in [
            "1791255600",
            "8015550100",
            "unix 123",
            "epoch 12345678901",
            "18005550100 unix",
        ] {
            assert_eq!(clock(search), None, "{search}");
        }
        // Nor does any other answer claim a phone number.
        assert_eq!(value("8015550100").as_deref(), None);
    }

    #[test]
    fn tells_the_week_day_and_iso_time() {
        assert_eq!(clock("week").as_deref(), Some("Week 41, 2026"));
        assert_eq!(clock("day of year").as_deref(), Some("Day 278 of 365"));
        assert_eq!(
            clock("now iso").as_deref(),
            Some("2026-10-05T20:00:00-07:00")
        );
        assert_eq!(
            clock("timestamp filename").as_deref(),
            Some("2026-10-05_20-00-00")
        );
    }
}
