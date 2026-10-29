//! One thread for the app's short timers: letting focus loss hide the bar again after a
//! move, saving where a drag left the bar, an update's countdown. A sleeping thread per timer
//! piled up during a drag, which reports hundreds of moves.
//!
//! Timers run one after another on that thread, so each should be quick.

use std::cmp::Ordering as Order;
use std::collections::BinaryHeap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

struct Timer {
    at: Instant,
    /// Breaks ties, so timers due at the same moment run in the order they were set.
    order: u64,
    run: Box<dyn FnOnce() + Send>,
}

// The heap keeps its largest on top, so the soonest compares largest.
impl Ord for Timer {
    fn cmp(&self, other: &Self) -> Order {
        (other.at, other.order).cmp(&(self.at, self.order))
    }
}

impl PartialOrd for Timer {
    fn partial_cmp(&self, other: &Self) -> Option<Order> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Timer {
    fn eq(&self, other: &Self) -> bool {
        (self.at, self.order) == (other.at, other.order)
    }
}

impl Eq for Timer {}

struct Timers {
    queue: Mutex<BinaryHeap<Timer>>,
    wake: Condvar,
    next: AtomicU64,
}

fn timers() -> &'static Timers {
    static TIMERS: OnceLock<Timers> = OnceLock::new();
    TIMERS.get_or_init(|| {
        if let Err(err) = std::thread::Builder::new().name("timers".into()).spawn(run) {
            log::error!("couldn't start the timer thread: {err}");
        }
        Timers {
            queue: Mutex::new(BinaryHeap::new()),
            wake: Condvar::new(),
            next: AtomicU64::new(0),
        }
    })
}

fn lock(timers: &Timers) -> MutexGuard<'_, BinaryHeap<Timer>> {
    timers.queue.lock().unwrap_or_else(PoisonError::into_inner)
}

fn run() {
    let timers = timers();
    let mut queue = lock(timers);
    loop {
        let now = Instant::now();
        match queue.peek().map(|timer| timer.at) {
            Some(at) if at <= now => {
                let Some(timer) = queue.pop() else {
                    continue;
                };
                drop(queue);
                // A timer that panics mustn't take the others down with it.
                if catch_unwind(AssertUnwindSafe(timer.run)).is_err() {
                    log::error!("a timer panicked");
                }
                queue = lock(timers);
            }
            Some(at) => {
                queue = timers
                    .wake
                    .wait_timeout(queue, at - now)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
            None => {
                queue = timers
                    .wake
                    .wait(queue)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        }
    }
}

/// Runs `run` on the timer thread once `delay` has passed.
pub fn after(delay: Duration, run: impl FnOnce() + Send + 'static) {
    let timers = timers();
    let timer = Timer {
        at: Instant::now() + delay,
        order: timers.next.fetch_add(1, Ordering::SeqCst),
        run: Box::new(run),
    };
    lock(timers).push(timer);
    timers.wake.notify_one();
}

/// Runs only the last of a burst of calls, once the calls have stopped for its delay; or
/// none, when cancelled first.
pub struct Debounce(AtomicU64);

impl Debounce {
    pub const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    /// Runs `run` after `delay`, unless this is scheduled again or cancelled before then.
    pub fn schedule(&'static self, delay: Duration, run: impl FnOnce() + Send + 'static) {
        let generation = self.0.fetch_add(1, Ordering::SeqCst) + 1;
        after(delay, move || {
            if self.0.load(Ordering::SeqCst) == generation {
                run();
            }
        });
    }

    /// Drops what's scheduled.
    pub fn cancel(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    const WAIT: Duration = Duration::from_secs(2);

    #[test]
    fn timers_run_soonest_first() {
        let (tx, rx) = mpsc::channel();
        for (delay, name) in [(80, "late"), (10, "early"), (40, "middle")] {
            let tx = tx.clone();
            after(Duration::from_millis(delay), move || {
                let _ = tx.send(name);
            });
        }
        let order: Vec<_> = (0..3).map(|_| rx.recv_timeout(WAIT).unwrap()).collect();
        assert_eq!(order, ["early", "middle", "late"]);
    }

    #[test]
    fn a_burst_runs_once_with_the_last_value() {
        static BURST: Debounce = Debounce::new();
        let (tx, rx) = mpsc::channel();
        for value in 0..100 {
            let tx = tx.clone();
            BURST.schedule(Duration::from_millis(50), move || {
                let _ = tx.send(value);
            });
        }
        assert_eq!(rx.recv_timeout(WAIT), Ok(99));
        assert!(rx.recv_timeout(Duration::from_millis(150)).is_err());
    }

    #[test]
    fn a_cancelled_debounce_never_runs() {
        static CANCELLED: Debounce = Debounce::new();
        let (tx, rx) = mpsc::channel();
        CANCELLED.schedule(Duration::from_millis(30), move || {
            let _ = tx.send(());
        });
        CANCELLED.cancel();
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
    }

    #[test]
    fn a_panicking_timer_leaves_the_rest_running() {
        after(Duration::ZERO, || panic!("on purpose"));
        let (tx, rx) = mpsc::channel();
        after(Duration::from_millis(10), move || {
            let _ = tx.send(());
        });
        assert!(rx.recv_timeout(WAIT).is_ok());
    }
}
