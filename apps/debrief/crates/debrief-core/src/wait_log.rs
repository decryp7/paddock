//! Phase logs for waits (ER-27, ER-32): a [`Watch`] marks a point where a thread may wait (a lock, a queue, a
//! DuckDB statement, a join), logs how long it took when it ends, and a watchdog thread says "still open" for every
//! watch that stays open longer than a threshold, together with the other open watches. A hang that leaves no other
//! trace then shows which wait was open, since when, where it was started, and what else was running.
//!
//! Bounded by design: the watchdog only reads and logs and never waits on the thing it watches; it reports each open
//! watch every `still` period (the first after one period), naming at most [`SNAPSHOT_MAX`] other watches per line.
//! No user content: labels are fixed phase names and a source location.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::panic::Location;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// How long a watch stays open before the watchdog says it is still open, and how often it repeats.
pub const STILL: Duration = Duration::from_secs(5);
/// A watch that ended after at least this long is logged at `warn` (shorter ones at `debug`).
pub const SLOW: Duration = Duration::from_secs(1);
/// Most other open watches named in one "still open" line.
pub const SNAPSHOT_MAX: usize = 8;

const TARGET: &str = "debrief_core::wait_log";

/// Where the lines go (`log` in production, a channel in tests).
type Sink = Arc<dyn Fn(log::Level, String) + Send + Sync>;

struct Entry {
    label: Cow<'static, str>,
    at: &'static Location<'static>,
    thread: String,
    started: Instant,
    reported: bool,
    last: Instant,
    detail: Option<fn() -> String>,
}

#[derive(Default)]
struct State {
    open: BTreeMap<u64, Entry>,
    closed: bool,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    still: Duration,
    sink: Sink,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// A set of open watches and the watchdog thread that reports the long ones.
pub struct Waits {
    shared: Arc<Shared>,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn to_log(level: log::Level, line: String) {
    log::log!(target: TARGET, level, "{line}");
}

/// The process-wide set (reports through `log`).
pub fn global() -> &'static Waits {
    static GLOBAL: std::sync::OnceLock<Waits> = std::sync::OnceLock::new();
    GLOBAL.get_or_init(|| {
        // `DEBRIEF_WAIT_STILL_MS` shortens the period (tests, or a diagnosis that wants an earlier first line).
        let still = std::env::var("DEBRIEF_WAIT_STILL_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|ms| *ms > 0)
            .map_or(STILL, Duration::from_millis);
        Waits::new(still, Arc::new(to_log))
    })
}

/// Opens a watch on the global set; it ends when the guard drops. The caller's source location is in its lines.
#[track_caller]
pub fn watch(label: impl Into<Cow<'static, str>>) -> Watch {
    global().watch(label)
}

/// [`watch`] with a function whose text is added to the "still open" line (for example how many statements run).
#[track_caller]
pub fn watch_with(label: impl Into<Cow<'static, str>>, detail: fn() -> String) -> Watch {
    global().watch_with(label, detail)
}

/// Locks `m` (recovering from poisoning). Only when another thread holds it, the wait is watched as `label`.
#[track_caller]
pub fn lock_watched<'a, T>(m: &'a Mutex<T>, label: &'static str) -> MutexGuard<'a, T> {
    match m.try_lock() {
        Ok(g) => g,
        Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => {
            let _w = watch(label);
            m.lock().unwrap_or_else(|p| p.into_inner())
        }
    }
}

impl Waits {
    /// A set whose watchdog reports watches open longer than `still` to `sink`.
    pub fn new(still: Duration, sink: Sink) -> Waits {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            still,
            sink,
        });
        let dog = shared.clone();
        // No watchdog thread (a limit of the machine) means no "still open" lines, never a failure.
        let _ = std::thread::Builder::new()
            .name("debrief-wait-watch".into())
            .spawn(move || watchdog(&dog));
        Waits { shared }
    }

    #[track_caller]
    pub fn watch(&self, label: impl Into<Cow<'static, str>>) -> Watch {
        self.open(label.into(), None, Location::caller())
    }

    #[track_caller]
    pub fn watch_with(&self, label: impl Into<Cow<'static, str>>, detail: fn() -> String) -> Watch {
        self.open(label.into(), Some(detail), Location::caller())
    }

    fn open(&self, label: Cow<'static, str>, detail: Option<fn() -> String>, at: &'static Location<'static>) -> Watch {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        let now = Instant::now();
        let first = {
            let mut s = self.shared.state();
            let first = s.open.is_empty();
            s.open.insert(
                id,
                Entry {
                    label: label.clone(),
                    at,
                    thread,
                    started: now,
                    reported: false,
                    last: now,
                    detail,
                },
            );
            first
        };
        if first {
            self.shared.wake.notify_all();
        }
        if log::log_enabled!(target: TARGET, log::Level::Trace) {
            (self.shared.sink)(log::Level::Trace, format!("wait #{id} begins: {label} ({at})"));
        }
        Watch {
            id,
            shared: self.shared.clone(),
        }
    }

    /// How many watches are open.
    pub fn open_count(&self) -> usize {
        self.shared.state().open.len()
    }
}

impl Drop for Waits {
    fn drop(&mut self) {
        self.shared.state().closed = true;
        self.shared.wake.notify_all();
    }
}

/// An open wait; dropping it ends the wait and logs how long it took.
#[must_use = "a watch ends when it is dropped"]
pub struct Watch {
    id: u64,
    shared: Arc<Shared>,
}

impl Watch {
    /// The correlation id of this wait: on its begin, end and still-open lines.
    pub fn id(&self) -> u64 {
        self.id
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let Some(e) = self.shared.state().open.remove(&self.id) else {
            return;
        };
        let took = e.started.elapsed();
        let slow = took >= SLOW.min(self.shared.still);
        let level = if slow { log::Level::Warn } else { log::Level::Debug };
        if slow || log::log_enabled!(target: TARGET, level) {
            let note = if e.reported { " (it was reported as still open)" } else { "" };
            (self.shared.sink)(
                level,
                format!(
                    "wait #{} ended after {} ms: {} ({}){note}",
                    self.id,
                    took.as_millis(),
                    e.label,
                    e.at
                ),
            );
        }
    }
}

fn watchdog(shared: &Shared) {
    let tick = (shared.still / 4).max(Duration::from_millis(5));
    let mut state = shared.state();
    loop {
        if state.closed {
            return;
        }
        if state.open.is_empty() {
            state = shared.wake.wait(state).unwrap_or_else(|p| p.into_inner());
            continue;
        }
        let now = Instant::now();
        let due: Vec<u64> = state
            .open
            .iter()
            .filter(|(_, e)| now.duration_since(e.last) >= shared.still)
            .map(|(id, _)| *id)
            .collect();
        let mut lines = Vec::new();
        for id in &due {
            lines.push((still_line(&state, *id, now), state.open[id].detail));
            if let Some(e) = state.open.get_mut(id) {
                e.reported = true;
                e.last = now;
            }
        }
        drop(state);
        for (line, detail) in lines {
            let extra = detail.map(|f| format!("; {}", f())).unwrap_or_default();
            (shared.sink)(log::Level::Warn, format!("{line}{extra}"));
        }
        state = shared.state();
        if state.closed {
            return;
        }
        state = shared.wake.wait_timeout(state, tick).unwrap_or_else(|p| p.into_inner()).0;
    }
}

fn still_line(state: &State, id: u64, now: Instant) -> String {
    let e = &state.open[&id];
    let mut others: Vec<(&u64, &Entry)> = state.open.iter().filter(|(i, _)| **i != id).collect();
    others.sort_by_key(|(_, o)| o.started);
    let more = others.len().saturating_sub(SNAPSHOT_MAX);
    let list: Vec<String> = others
        .iter()
        .take(SNAPSHOT_MAX)
        .map(|(i, o)| {
            format!(
                "#{i} {} on {} for {} ms",
                o.label,
                o.thread,
                now.duration_since(o.started).as_millis()
            )
        })
        .collect();
    format!(
        "wait #{id} is still open after {} ms: {} ({}) on thread {}; {} other open: [{}]{}",
        now.duration_since(e.started).as_millis(),
        e.label,
        e.at,
        e.thread,
        others.len(),
        list.join("; "),
        if more > 0 { format!(", and {more} more") } else { String::new() }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    type Lines = mpsc::Receiver<(log::Level, String)>;

    fn waits(still: Duration) -> (Waits, Lines) {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let sink: Sink = Arc::new(move |l, s| {
            let _ = tx.lock().unwrap_or_else(|p| p.into_inner()).send((l, s));
        });
        (Waits::new(still, sink), rx)
    }

    /// The next line containing `needle`: waits on the channel (the condition), bounded.
    fn line(rx: &Lines, needle: &str) -> (log::Level, String) {
        loop {
            let l = rx.recv_timeout(Duration::from_secs(30)).expect("the line never came");
            if l.1.contains(needle) {
                return l;
            }
        }
    }

    #[test]
    fn a_blocked_wait_is_reported_as_still_open_with_the_others_and_ends_with_its_duration() {
        let (w, rx) = waits(Duration::from_millis(20));
        let held = Mutex::new(());
        let guard = held.lock().unwrap();
        let (in_wait_tx, in_wait_rx) = mpsc::channel();
        std::thread::scope(|s| {
            let h = s.spawn(|| {
                let _job = w.watch("an import job");
                let _w = w.watch_with("the pool lock", || "2 statements active".to_string());
                in_wait_tx.send(()).unwrap();
                let _g = held.lock().unwrap();
            });
            in_wait_rx.recv().unwrap();
            let (level, text) = loop {
                let l = line(&rx, "still open");
                if l.1.contains("ms: the pool lock") {
                    break l;
                }
            };
            assert_eq!(level, log::Level::Warn);
            assert!(text.contains("an import job on"), "names what else is open: {text}");
            assert!(text.contains("2 statements active"), "{text}");
            assert!(text.contains("wait_log.rs:"), "names where the wait started: {text}");
            drop(guard);
            h.join().unwrap();
        });
        let (level, text) = line(&rx, "ended after");
        assert_eq!(level, log::Level::Warn, "a long wait ends at warn: {text}");
        assert!(text.contains("reported as still open"), "{text}");
        assert_eq!(w.open_count(), 0);
    }

    #[test]
    fn a_quick_wait_logs_no_still_line_and_nothing_at_warn() {
        let (w, rx) = waits(Duration::from_secs(3600));
        drop(w.watch("quick"));
        // Debug lines go through the log facade's level check; with no logger set none is sent at all.
        drop(w);
        assert!(rx.iter().all(|(l, t)| l != log::Level::Warn && !t.contains("still open")));
    }

    #[test]
    fn lock_watched_recovers_from_poisoning_and_watches_only_a_contended_lock() {
        let m = Arc::new(Mutex::new(1));
        let m2 = m.clone();
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("poison");
        })
        .join();
        assert_eq!(*lock_watched(&m, "x"), 1);
        assert_eq!(global().open_count(), 0);
    }
}
