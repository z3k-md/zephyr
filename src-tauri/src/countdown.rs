//! Countdown timers started from the bar ("timer 5m", "timer 90 tea"). Kept in state.json so
//! the bar can list and cancel them and a restart picks them back up; each waits on its own
//! thread by the wall clock and ends in a notification.

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
}

impl Countdown {
    fn title(&self) -> String {
        if self.label.is_empty() {
            "Timer done".into()
        } else {
            format!("{} — timer done", self.label)
        }
    }

    fn body(&self) -> String {
        format!("{} timer", span(self.seconds))
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

/// Changes the saved timers, telling every window.
fn save(app: &AppHandle, change: impl FnOnce(&mut Vec<Countdown>)) {
    let state = app.state::<crate::state::AppState>();
    let saved = state.update(|persisted| {
        change(&mut persisted.timers);
        Ok(())
    });
    match saved {
        Ok(snapshot) => {
            let _ = app.emit("state-changed", &snapshot);
        }
        Err(err) => log::error!("couldn't save the timers: {err}"),
    }
}

fn pending(app: &AppHandle) -> Vec<Countdown> {
    app.state::<crate::state::AppState>()
        .snapshot()
        .map(|snapshot| snapshot.timers.clone())
        .unwrap_or_default()
}

/// Starts a timer for `seconds`.
pub fn start(app: &AppHandle, seconds: i64, label: String) -> Result<Countdown, String> {
    if !(1..=MAX_SECONDS).contains(&seconds) {
        return Err("A timer runs from a second to a week".into());
    }
    let label: String = label.trim().chars().take(80).collect();
    let mut timer = None;
    save(app, |timers| {
        let id = timers.iter().map(|timer| timer.id).max().unwrap_or(0) + 1;
        let countdown = Countdown {
            id,
            at: Local::now().timestamp() + seconds,
            seconds,
            label,
        };
        timers.push(countdown.clone());
        timer = Some(countdown);
    });
    let timer = timer.ok_or("couldn't save the timer")?;
    log::info!("timer: {} started", span(seconds));
    arm(app, timer.clone());
    Ok(timer)
}

/// Stops a timer before it runs out.
pub fn cancel(app: &AppHandle, id: u64) {
    save(app, |timers| timers.retain(|timer| timer.id != id));
}

/// Whether this very timer is still saved. Ids come back after the newest timer is cancelled,
/// so a new timer can carry a cancelled one's id; the whole timer has to match.
fn still_set(pending: &[Countdown], timer: &Countdown) -> bool {
    pending.contains(timer)
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
            save(&app, |timers| {
                timers.retain(|pending| pending.id != timer.id)
            });
            log::info!("timer: {} done", span(timer.seconds));
            crate::util::notify(&app, &timer.title(), &timer.body());
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

    #[test]
    fn a_label_leads_the_notification() {
        let timer = Countdown {
            id: 1,
            at: 0,
            seconds: 600,
            label: "Tea".into(),
        };
        assert_eq!(timer.title(), "Tea — timer done");
        assert_eq!(timer.body(), "10m timer");
    }

    #[test]
    fn a_new_timer_with_a_cancelled_ones_id_is_not_that_timer() {
        let cancelled = Countdown {
            id: 3,
            at: 1_000,
            seconds: 60,
            label: String::new(),
        };
        let restarted = Countdown {
            at: 1_600,
            seconds: 600,
            ..cancelled.clone()
        };
        assert!(!still_set(std::slice::from_ref(&restarted), &cancelled));
        assert!(still_set(std::slice::from_ref(&restarted), &restarted));
    }
}
