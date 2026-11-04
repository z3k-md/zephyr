//! Countdown timers started from the bar ("timer 5m", "timer 90 tea", "focus 50m deep work").
//! Kept in state.json so the bar can list and cancel them and a restart picks them back up;
//! each waits on its own thread by the wall clock and ends in a notification. A focus session
//! is one of these timers: only one runs at a time, and ending it can start a plain break.

use std::time::Duration;

use chrono::Local;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

/// The longest timer: a week, in seconds.
pub const MAX_SECONDS: i64 = 7 * 24 * 3600;
/// A timer that ran out longer ago than this while Zephyr was closed is dropped, not
/// announced.
const STALE_SECS: i64 = 3600;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Countdown {
    pub id: u64,
    /// When it runs out, in unix seconds.
    pub at: i64,
    /// How long it was set for, for its name ("5m timer").
    pub seconds: i64,
    /// What it's for ("tea"); may be empty.
    pub label: String,
    /// A focus session, rather than a plain timer. Saved timers from before focus existed
    /// read as plain ones.
    #[serde(default)]
    pub focus: bool,
}

impl Countdown {
    fn title(&self) -> String {
        if self.focus {
            if self.label.is_empty() {
                "Focus done".into()
            } else {
                format!("{} — focus done", self.label)
            }
        } else if self.label.is_empty() {
            "Timer done".into()
        } else {
            format!("{} — timer done", self.label)
        }
    }

    /// `break_seconds` is the break a focus session starts; 0 starts none. Plain timers
    /// ignore it.
    fn body(&self, break_seconds: i64) -> String {
        if self.focus {
            if break_seconds > 0 {
                format!("{} break", span(break_seconds))
            } else {
                format!("{} focus", span(self.seconds))
            }
        } else {
            format!("{} timer", span(self.seconds))
        }
    }
}

/// "45s", "5m", "1h 30m", "2m 15s".
fn span(seconds: i64) -> String {
    let (hours, minutes, secs) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    let mut parts = Vec::new();
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes}m"));
    }
    if secs > 0 || parts.is_empty() {
        parts.push(format!("{secs}s"));
    }
    parts.join(" ")
}

/// Changes the saved timers, telling every window. False when the file couldn't be written,
/// in which case the change was not kept.
fn save(app: &AppHandle, change: impl FnOnce(&mut Vec<Countdown>)) -> bool {
    let state = app.state::<crate::state::AppState>();
    let saved = state.update(|persisted| {
        change(&mut persisted.timers);
        Ok(())
    });
    match saved {
        Ok(snapshot) => {
            let _ = app.emit("state-changed", &snapshot);
            true
        }
        Err(err) => {
            log::error!("couldn't save the timers: {err}");
            false
        }
    }
}

fn pending(app: &AppHandle) -> Vec<Countdown> {
    app.state::<crate::state::AppState>()
        .snapshot()
        .map(|snapshot| snapshot.timers.clone())
        .unwrap_or_default()
}

/// The timer to arm. The save closure records it before the file is written, so a write
/// that failed must not arm it or start a focus session.
fn ready_to_arm(saved: bool, timer: Option<Countdown>) -> Result<Countdown, String> {
    if !saved {
        return Err("couldn't save the timer".into());
    }
    timer.ok_or_else(|| "couldn't save the timer".into())
}

/// Starts a timer for `seconds`. A focus session replaces the one already running.
pub fn start(
    app: &AppHandle,
    seconds: i64,
    label: String,
    focus: bool,
) -> Result<Countdown, String> {
    if !(1..=MAX_SECONDS).contains(&seconds) {
        return Err("A timer runs from a second to a week".into());
    }
    let label: String = label.trim().chars().take(80).collect();
    let mut timer = None;
    let saved = save(app, |timers| {
        let id = timers.iter().map(|timer| timer.id).max().unwrap_or(0) + 1;
        let countdown = Countdown {
            id,
            at: Local::now().timestamp() + seconds,
            seconds,
            label,
            focus,
        };
        if let Some(replaced) = place(timers, countdown.clone()) {
            log::info!("focus: replaced {}", replaced.label);
        }
        timer = Some(countdown);
    });
    let timer = ready_to_arm(saved, timer)?;
    log::info!(
        "{}: {} started",
        if focus { "focus" } else { "timer" },
        span(seconds)
    );
    arm(app, timer.clone());
    if focus {
        crate::focus::begin(app);
    }
    Ok(timer)
}

/// Adds `next`. A focus session replaces the one already running; plain timers stack.
/// Returns the session it replaced.
fn place(timers: &mut Vec<Countdown>, next: Countdown) -> Option<Countdown> {
    let replaced = if next.focus {
        let old = timers.iter().find(|timer| timer.focus).cloned();
        timers.retain(|timer| !timer.focus);
        old
    } else {
        None
    };
    timers.push(next);
    replaced
}

/// Stops a timer before it runs out. Ending a focus session lifts the site block and does
/// not start a break.
pub fn cancel(app: &AppHandle, id: u64) {
    let mut ended_focus = false;
    let saved = save(app, |timers| {
        ended_focus = timers.iter().any(|timer| timer.id == id && timer.focus);
        timers.retain(|timer| timer.id != id);
    });
    if saved && ended_focus {
        crate::focus::publish(app);
    }
}

/// Whether this very timer is still saved. Ids come back after the newest timer is cancelled,
/// so a new timer can carry a cancelled one's id; the whole timer has to match.
fn still_set(pending: &[Countdown], timer: &Countdown) -> bool {
    pending.contains(timer)
}

/// Drops `timer` only when that same one is still saved. A newer timer that reused its id
/// stays, so a focus session that was replaced is not announced and does not start a break.
fn take(timers: &mut Vec<Countdown>, timer: &Countdown) -> bool {
    if !still_set(timers, timer) {
        return false;
    }
    timers.retain(|pending| pending != timer);
    true
}

/// Waits for the timer by the wall clock, so time asleep counts, then announces it.
fn arm(app: &AppHandle, timer: Countdown) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("countdown".into())
        .spawn(move || {
            loop {
                // Cancelled, or already announced by another thread.
                if !still_set(&pending(&app), &timer) {
                    return;
                }
                let left = timer.at - Local::now().timestamp();
                if left <= 0 {
                    break;
                }
                // Short sleeps, so a cancel or a sleeping machine is noticed soon.
                std::thread::sleep(Duration::from_secs(left.min(5) as u64));
            }
            let mut finished = false;
            let saved = save(&app, |timers| {
                if take(timers, &timer) {
                    finished = true;
                }
            });
            // Replaced or cancelled while this thread woke up, or the save didn't land.
            if !saved || !finished {
                return;
            }
            let break_for = if timer.focus {
                crate::focus::break_seconds(&app)
            } else {
                0
            };
            log::info!(
                "{}: {} done",
                if timer.focus { "focus" } else { "timer" },
                span(timer.seconds)
            );
            crate::util::notify(&app, &timer.title(), &timer.body(break_for));
            if timer.focus {
                // A session started since this one finished keeps its own block.
                crate::focus::publish(&app);
                if break_for > 0
                    && let Err(err) = start(&app, break_for, "Break".into(), false)
                {
                    log::error!("couldn't start the break: {err}");
                }
            }
        });
    if let Err(err) = spawned {
        log::error!("couldn't start a timer: {err}");
    }
}

/// On launch: picks saved timers back up. One that ran out while Zephyr was closed is
/// announced if it was recent, dropped otherwise.
pub fn resume(app: &AppHandle) {
    let now = Local::now().timestamp();
    let timers = pending(app);
    let (stale, live): (Vec<_>, Vec<_>) = timers
        .into_iter()
        .partition(|timer| now - timer.at > STALE_SECS);
    if !stale.is_empty() {
        save(app, |timers| timers.retain(|timer| !stale.contains(timer)));
    }
    for timer in live {
        arm(app, timer);
    }
    crate::focus::publish(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_name_the_parts_set() {
        assert_eq!(span(45), "45s");
        assert_eq!(span(300), "5m");
        assert_eq!(span(135), "2m 15s");
        assert_eq!(span(5400), "1h 30m");
        assert_eq!(span(3600), "1h");
    }

    fn countdown(id: u64, focus: bool, label: &str) -> Countdown {
        Countdown {
            id,
            at: 1_000 + i64::try_from(id).unwrap_or(i64::MAX),
            seconds: if focus { 1_500 } else { 60 },
            label: label.into(),
            focus,
        }
    }

    #[test]
    fn a_label_leads_the_notification() {
        let timer = Countdown {
            id: 1,
            at: 0,
            seconds: 600,
            label: "Tea".into(),
            focus: false,
        };
        assert_eq!(timer.title(), "Tea — timer done");
        assert_eq!(timer.body(0), "10m timer");
    }

    #[test]
    fn a_focus_session_names_itself_and_the_break() {
        let session = countdown(1, true, "Deep work");
        assert_eq!(session.title(), "Deep work — focus done");
        assert_eq!(session.body(300), "5m break");
        assert_eq!(session.body(0), "25m focus");
        let unnamed = countdown(2, true, "");
        assert_eq!(unnamed.title(), "Focus done");
    }

    #[test]
    fn a_saved_timer_without_focus_still_loads() {
        let timer: Countdown =
            serde_json::from_str(r#"{"id":1,"at":2,"seconds":3,"label":"tea"}"#).unwrap();
        assert!(!timer.focus);
        assert_eq!(timer.label, "tea");
    }

    #[test]
    fn a_new_focus_session_replaces_the_one_running() {
        let tea = countdown(1, false, "tea");
        let first = countdown(2, true, "Deep work");
        let second = countdown(3, true, "Email");
        let mut timers = vec![tea.clone(), first.clone()];
        let replaced = place(&mut timers, second.clone());
        assert_eq!(replaced, Some(first));
        assert_eq!(timers, vec![tea, second]);
    }

    #[test]
    fn a_plain_timer_leaves_the_focus_session_running() {
        let session = countdown(2, true, "Deep work");
        let mut timers = vec![session.clone()];
        assert_eq!(place(&mut timers, countdown(4, false, "tea")), None);
        assert_eq!(timers, vec![session, countdown(4, false, "tea")]);
    }

    #[test]
    fn a_new_timer_with_a_cancelled_ones_id_is_not_that_timer() {
        let cancelled = Countdown {
            id: 3,
            at: 1_000,
            seconds: 60,
            label: String::new(),
            focus: false,
        };
        let restarted = Countdown {
            at: 1_600,
            seconds: 600,
            ..cancelled.clone()
        };
        assert!(!still_set(std::slice::from_ref(&restarted), &cancelled));
        assert!(still_set(std::slice::from_ref(&restarted), &restarted));
        let mut timers = vec![restarted.clone()];
        assert!(!take(&mut timers, &cancelled));
        assert_eq!(timers, vec![restarted]);
    }

    #[test]
    fn a_timer_is_armed_only_when_the_save_lands() {
        let timer = countdown(1, true, "Deep work");
        assert_eq!(
            ready_to_arm(false, Some(timer.clone())).unwrap_err(),
            "couldn't save the timer"
        );
        assert!(ready_to_arm(true, None).is_err());
        assert_eq!(ready_to_arm(true, Some(timer.clone())).unwrap(), timer);
    }
}
