//! Phase logs for waits (ER-27, ER-32): a [`Watch`] marks a point where a thread may wait (a lock, a queue, a
//! DuckDB statement, a join), logs how long it took when it ends, and a watchdog thread says "still open" for every
//! watch that stays open longer than a threshold, together with the other open watches. A hang that leaves no other
//! trace then shows which wait was open, since when, where it was started, and what else was running.
//!
//! Two kinds, because Debrief's log has a daily byte budget (`logging.rs`) that a chatty line would use up:
//! - [`Kind::Wait`]: a lock or a queue. It is short when healthy, so a long one is a finding: "still open" at `warn`
//!   every `still` period for the first [`WARN_REPORTS`] reports, then at `info` with the same backoff as an activity (a
//!   lock held behind a long compaction is not a new finding every 5 s); the end at `warn` from one second.
//! - [`Kind::Activity`]: a job, a borrowed connection, a request that is running. These are long when healthy (an import
//!   of ten minutes), so they log "still open" at `info` with backoff (`still`, twice, four times ... up to
//!   [`MAX_BACKOFF`] times `still`) and end at `debug`, never `warn`.
//!
//! Bounded by design: the watchdog only reads and logs and never waits on the thing it watches, so it must never call
//! into DuckDB or take a store lock (a hang it is meant to explain must not be able to stop it). It names at most
//! [`SNAPSHOT_MAX`] other watches per line. No user content: labels are fixed phase names and a source location.
//!
//! Cost: a watch is a registry insert in one of [`SHARDS`] shards (no allocation for a `&'static str` label; the thread
//! is a handle, its name is read only when a line is written), measured in `docs/performance.md`.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::panic::Location;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::Thread;
use std::time::{Duration, Instant};

/// How long a watch stays open before the watchdog says it is still open (the first interval of the backoff).
pub const STILL: Duration = Duration::from_secs(5);
/// A [`Kind::Wait`] that ended after at least this long is logged at `warn` (shorter ones at `debug`).
pub const SLOW: Duration = Duration::from_secs(1);
/// Most other open watches named in one "still open" line.
pub const SNAPSHOT_MAX: usize = 8;
/// The backoff of an [`Kind::Activity`] stops growing at this many times `still` (5 s * 60 = 5 min).
pub const MAX_BACKOFF: u32 = 60;
/// How many "still open" reports of a [`Kind::Wait`] are at `warn` (the next ones back off at `info`).
pub const WARN_REPORTS: u32 = 3;
/// Registry shards (a watch goes to the shard of its id): threads opening watches rarely meet on one lock.
pub const SHARDS: usize = 16;

/// The watchdog is working or between timed wake-ups.
const PARK_RUNNING: u8 = 0;
/// It sleeps with no timeout: no watch is open.
const PARK_IDLE: u8 = 1;
/// It sleeps until its next tick: a watch is open.
const PARK_TIMED: u8 = 2;

const TARGET: &str = "debrief_core::wait_log";

/// What a watch is for (see the module documentation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A lock or queue wait: long means something is wrong.
    Wait,
    /// Work that is long when healthy: reported at `info` with backoff, never at `warn`.
    Activity,
}

/// Where the lines go (`log` in production, a channel in tests).
type Sink = Arc<dyn Fn(log::Level, String) + Send + Sync>;

struct Entry {
    label: Cow<'static, str>,
    at: &'static Location<'static>,
    thread: Thread,
    kind: Kind,
    started: Instant,
    /// When the watchdog reports it next, and the gap after that (doubles for an activity).
    next: Instant,
    gap: Duration,
    /// How many "still open" reports were written.
    reports: u32,
    detail: Option<fn() -> String>,
}

struct Shared {
    shards: [Mutex<BTreeMap<u64, Entry>>; SHARDS],
    idle: Mutex<()>,
    wake: Condvar,
    closed: AtomicBool,
    /// Watches open now: the watchdog sleeps without a timeout while it is 0 (no idle wake-ups).
    open: AtomicUsize,
    /// What the watchdog is doing (`PARK_*`), for the idle test.
    parked: AtomicU8,
    /// Send the begin and end lines to the sink whatever the log level (tests; production checks the level first).
    all_lines: bool,
    still: Duration,
    sink: Sink,
}

impl Shared {
    fn shard(&self, id: u64) -> MutexGuard<'_, BTreeMap<u64, Entry>> {
        self.shards[id as usize % SHARDS].lock().unwrap_or_else(|p| p.into_inner())
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
    GLOBAL.get_or_init(|| Waits::new(STILL, Arc::new(to_log)))
}

/// Opens a [`Kind::Wait`] on the global set; it ends when the guard drops. The caller's location is in its lines.
#[track_caller]
pub fn watch(label: impl Into<Cow<'static, str>>) -> Watch {
    global().open(Kind::Wait, label.into(), None, Location::caller())
}

/// Opens a [`Kind::Activity`] on the global set (a job, a borrowed connection, a running request).
#[track_caller]
pub fn activity(label: impl Into<Cow<'static, str>>) -> Watch {
    global().open(Kind::Activity, label.into(), None, Location::caller())
}

/// [`activity`] with a function whose text is added to the "still open" line (for example how many statements run).
#[track_caller]
pub fn activity_with(label: impl Into<Cow<'static, str>>, detail: fn() -> String) -> Watch {
    global().open(Kind::Activity, label.into(), Some(detail), Location::caller())
}

/// [`watch`] with such a detail function.
#[track_caller]
pub fn watch_with(label: impl Into<Cow<'static, str>>, detail: fn() -> String) -> Watch {
    global().open(Kind::Wait, label.into(), Some(detail), Location::caller())
}

/// Locks `m` (recovering from poisoning). Only when another thread holds it, the wait is watched as `label`.
#[track_caller]
pub fn lock_watched<'a, T>(m: &'a Mutex<T>, label: &'static str) -> MutexGuard<'a, T> {
    global().lock_watched(m, label)
}

impl Waits {
    /// A set whose watchdog first reports watches open longer than `still` to `sink`.
    pub fn new(still: Duration, sink: Sink) -> Waits {
        Waits::build(still, sink, false)
    }

    /// Like [`Waits::new`], and every begin and end line goes to `sink` whatever the log level (tests).
    pub fn new_tracing(still: Duration, sink: Sink) -> Waits {
        Waits::build(still, sink, true)
    }

    fn build(still: Duration, sink: Sink, all_lines: bool) -> Waits {
        let shared = Arc::new(Shared {
            shards: std::array::from_fn(|_| Mutex::new(BTreeMap::new())),
            idle: Mutex::new(()),
            wake: Condvar::new(),
            closed: AtomicBool::new(false),
            open: AtomicUsize::new(0),
            parked: AtomicU8::new(PARK_RUNNING),
            all_lines,
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

    /// See [`lock_watched`].
    #[track_caller]
    pub fn lock_watched<'a, T>(&self, m: &'a Mutex<T>, label: &'static str) -> MutexGuard<'a, T> {
        match m.try_lock() {
            Ok(g) => g,
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                let _w = self.open(Kind::Wait, Cow::Borrowed(label), None, Location::caller());
                m.lock().unwrap_or_else(|p| p.into_inner())
            }
        }
    }

    /// Opens a watch of `kind` (see [`watch`], [`activity`]).
    pub fn open(&self, kind: Kind, label: Cow<'static, str>, detail: Option<fn() -> String>, at: &'static Location<'static>) -> Watch {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let now = Instant::now();
        if self.shared.all_lines || log::log_enabled!(target: TARGET, log::Level::Trace) {
            (self.shared.sink)(log::Level::Trace, format!("wait #{id} begins: {label} ({at})"));
        }
        if self.shared.open.fetch_add(1, Ordering::SeqCst) == 0 {
            // The first watch wakes the watchdog; taking the lock first means it is either not yet asleep (and then
            // sees the count) or asleep (and is notified): no wake-up is lost.
            drop(self.shared.idle.lock().unwrap_or_else(|p| p.into_inner()));
            self.shared.wake.notify_one();
        }
        self.shared.shard(id).insert(
            id,
            Entry {
                label,
                at,
                thread: std::thread::current(),
                kind,
                started: now,
                next: now + self.shared.still,
                gap: self.shared.still,
                reports: 0,
                detail,
            },
        );
        Watch {
            id,
            shared: self.shared.clone(),
        }
    }

    /// A [`Kind::Wait`] on this set.
    #[track_caller]
    pub fn watch(&self, label: impl Into<Cow<'static, str>>) -> Watch {
        self.open(Kind::Wait, label.into(), None, Location::caller())
    }

    /// A [`Kind::Activity`] on this set.
    #[track_caller]
    pub fn activity(&self, label: impl Into<Cow<'static, str>>) -> Watch {
        self.open(Kind::Activity, label.into(), None, Location::caller())
    }

    /// What the watchdog does now: running, asleep with no timeout (nothing open) or asleep until its next tick (tests).
    #[cfg(test)]
    fn parked(&self) -> u8 {
        self.shared.parked.load(Ordering::SeqCst)
    }

    /// How many watches are open.
    pub fn open_count(&self) -> usize {
        self.shared
            .shards
            .iter()
            .map(|s| s.lock().unwrap_or_else(|p| p.into_inner()).len())
            .sum()
    }
}

impl Drop for Waits {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::SeqCst);
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
        let Some(e) = self.shared.shard(self.id).remove(&self.id) else {
            return;
        };
        self.shared.open.fetch_sub(1, Ordering::SeqCst);
        let took = e.started.elapsed();
        let level = match e.kind {
            Kind::Wait if took >= SLOW.min(self.shared.still) => log::Level::Warn,
            _ => log::Level::Debug,
        };
        if level == log::Level::Warn || self.shared.all_lines || log::log_enabled!(target: TARGET, level) {
            let note = if e.reports > 0 { " (it was reported as still open)" } else { "" };
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

/// A line due from the watchdog: its level, the text and the watch's detail function.
type Report = (log::Level, String, Option<fn() -> String>);

/// The gap after report number `reports` (1-based): a wait repeats every `still` for its [`WARN_REPORTS`] warn reports,
/// then backs off like an activity, which doubles from the first report up to `MAX_BACKOFF` times `still`.
fn next_gap(kind: Kind, gap: Duration, still: Duration, reports: u32) -> Duration {
    match kind {
        Kind::Wait if reports < WARN_REPORTS => still,
        _ => (gap * 2).min(still * MAX_BACKOFF),
    }
}

/// The level of report number `reports` (1-based).
fn report_level(kind: Kind, reports: u32) -> log::Level {
    match kind {
        Kind::Wait if reports <= WARN_REPORTS => log::Level::Warn,
        _ => log::Level::Info,
    }
}

/// The open watches whose report is due, with the line to write (the shards are locked one at a time and briefly;
/// nothing here calls out of this module while a lock is held).
fn due_lines(shared: &Shared, now: Instant) -> Vec<Report> {
    struct Row {
        id: u64,
        label: Cow<'static, str>,
        at: &'static Location<'static>,
        thread: Thread,
        started: Instant,
        kind: Kind,
        due: bool,
        reports: u32,
        detail: Option<fn() -> String>,
    }
    let mut rows = Vec::new();
    for i in 0..SHARDS {
        let mut shard = shared.shards[i].lock().unwrap_or_else(|p| p.into_inner());
        for (id, e) in shard.iter_mut() {
            let due = now >= e.next;
            if due {
                e.reports += 1;
                e.gap = next_gap(e.kind, e.gap, shared.still, e.reports);
                e.next = now + e.gap;
            }
            rows.push(Row {
                id: *id,
                label: e.label.clone(),
                at: e.at,
                thread: e.thread.clone(),
                started: e.started,
                kind: e.kind,
                due,
                reports: e.reports,
                detail: e.detail,
            });
        }
    }
    rows.sort_by_key(|r| r.started);
    let mut out = Vec::new();
    for r in rows.iter().filter(|r| r.due) {
        let others: Vec<&Row> = rows.iter().filter(|o| o.id != r.id).collect();
        let list: Vec<String> = others
            .iter()
            .take(SNAPSHOT_MAX)
            .map(|o| {
                format!(
                    "#{} {} on {} for {} ms",
                    o.id,
                    o.label,
                    o.thread.name().unwrap_or("unnamed"),
                    now.duration_since(o.started).as_millis()
                )
            })
            .collect();
        let more = others.len().saturating_sub(SNAPSHOT_MAX);
        let line = format!(
            "wait #{} is still open after {} ms: {} ({}) on thread {}; {} other open: [{}]{}",
            r.id,
            now.duration_since(r.started).as_millis(),
            r.label,
            r.at,
            r.thread.name().unwrap_or("unnamed"),
            others.len(),
            list.join("; "),
            if more > 0 { format!(", and {more} more") } else { String::new() }
        );
        out.push((report_level(r.kind, r.reports), line, r.detail));
    }
    out
}

fn watchdog(shared: &Shared) {
    let tick = (shared.still / 4).max(Duration::from_millis(5));
    while !shared.closed.load(Ordering::SeqCst) {
        shared.parked.store(PARK_RUNNING, Ordering::SeqCst);
        for (level, line, detail) in due_lines(shared, Instant::now()) {
            let extra = detail.map(|f| format!("; {}", f())).unwrap_or_default();
            (shared.sink)(level, format!("{line}{extra}"));
        }
        let mut idle = shared.idle.lock().unwrap_or_else(|p| p.into_inner());
        // With nothing open it sleeps with no timeout (an idle Debrief must not wake every few ms: no idle CPU); the first
        // watch opened notifies it.
        while shared.open.load(Ordering::SeqCst) == 0 {
            if shared.closed.load(Ordering::SeqCst) {
                return;
            }
            shared.parked.store(PARK_IDLE, Ordering::SeqCst);
            idle = shared.wake.wait(idle).unwrap_or_else(|p| p.into_inner());
        }
        if shared.closed.load(Ordering::SeqCst) {
            return;
        }
        shared.parked.store(PARK_TIMED, Ordering::SeqCst);
        let _ = shared.wake.wait_timeout(idle, tick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    type Lines = mpsc::Receiver<(log::Level, String)>;

    fn sink() -> (Sink, Lines) {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let sink: Sink = Arc::new(move |l, s| {
            let _ = tx.lock().unwrap_or_else(|p| p.into_inner()).send((l, s));
        });
        (sink, rx)
    }

    fn waits(still: Duration) -> (Waits, Lines) {
        let (sink, rx) = sink();
        (Waits::new_tracing(still, sink), rx)
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
                let _job = w.activity("an import job");
                let _w = w.open(
                    Kind::Wait,
                    "the pool lock".into(),
                    Some(|| "2 statements active".to_string()),
                    Location::caller(),
                );
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

    /// The log has a daily byte budget: a job that is long when healthy must never write at warn, neither while it
    /// runs nor when it ends (review of 255b1db99, P1).
    #[test]
    fn a_long_activity_is_reported_at_info_with_backoff_and_never_at_warn() {
        let (w, rx) = waits(Duration::from_millis(10));
        let job = w.activity("import job runs");
        let mut after = Vec::new();
        while after.len() < 4 {
            let (level, text) = line(&rx, "still open");
            assert_eq!(level, log::Level::Info, "{text}");
            let ms: u64 = text
                .split("still open after ")
                .nth(1)
                .unwrap()
                .split(" ms")
                .next()
                .unwrap()
                .parse()
                .unwrap();
            after.push(ms);
        }
        assert!(after.windows(2).all(|p| p[0] < p[1]), "reports come later each time: {after:?}");
        drop(job);
        let (level, text) = line(&rx, "ended after");
        assert_eq!(level, log::Level::Debug, "an activity ends at debug however long it ran: {text}");
        drop(w);
        assert!(rx.iter().all(|(l, _)| l != log::Level::Warn), "nothing at warn");
    }

    #[test]
    fn the_backoff_doubles_for_an_activity_up_to_the_cap_and_a_wait_backs_off_after_its_warn_reports() {
        let still = Duration::from_secs(5);
        let gaps = |kind| {
            let mut gap = still;
            let mut seen = Vec::new();
            for n in 1..=9 {
                gap = next_gap(kind, gap, still, n);
                seen.push(gap.as_secs());
            }
            seen
        };
        assert_eq!(gaps(Kind::Activity), [10, 20, 40, 80, 160, 300, 300, 300, 300]);
        assert_eq!(
            gaps(Kind::Wait),
            [5, 5, 10, 20, 40, 80, 160, 300, 300],
            "every period for the warn reports, then backing off"
        );
        assert_eq!(report_level(Kind::Wait, WARN_REPORTS), log::Level::Warn);
        assert_eq!(report_level(Kind::Wait, WARN_REPORTS + 1), log::Level::Info);
        assert_eq!(report_level(Kind::Activity, 1), log::Level::Info);
    }

    /// A lock held behind a long job (a compaction) must not warn every period for as long as it is held.
    #[test]
    fn a_wait_held_for_long_warns_three_times_then_reports_at_info_with_backoff() {
        let (w, rx) = waits(Duration::from_millis(10));
        let held = w.watch("the write lock");
        let mut levels = Vec::new();
        while levels.len() < 5 {
            levels.push(line(&rx, "still open").0);
        }
        use log::Level::{Info, Warn};
        assert_eq!(levels, [Warn, Warn, Warn, Info, Info]);
        drop(held);
    }

    /// An idle Debrief must not wake a thread every few milliseconds: with no watch open the watchdog sleeps with no
    /// timeout; the first watch wakes it, and when the last one ends it goes back (ER-36, idle CPU).
    #[test]
    fn the_watchdog_sleeps_without_a_timeout_when_no_watch_is_open() {
        let (w, _rx) = waits(Duration::from_millis(10));
        let until = |w: &Waits, state: u8| {
            let deadline = Instant::now() + Duration::from_secs(30);
            while w.parked() != state {
                assert!(Instant::now() < deadline, "the watchdog never reached state {state}");
                std::thread::yield_now();
            }
        };
        until(&w, PARK_IDLE);
        let held = w.watch("something");
        until(&w, PARK_TIMED);
        drop(held);
        until(&w, PARK_IDLE);
    }

    #[test]
    fn a_quick_wait_logs_no_still_line_and_nothing_at_warn() {
        let (w, rx) = waits(Duration::from_secs(3600));
        drop(w.watch("quick"));
        let (level, text) = line(&rx, "ended after");
        assert_eq!(level, log::Level::Debug, "{text}");
        drop(w);
        assert!(rx.iter().all(|(l, t)| l != log::Level::Warn && !t.contains("still open")));
    }

    #[test]
    fn lock_watched_begins_a_wait_only_when_contended_and_recovers_from_poisoning() {
        let (w, rx) = waits(Duration::from_secs(3600));
        let m = Arc::new(Mutex::new(1));
        let m2 = m.clone();
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("poison");
        })
        .join();
        // Uncontended (and poisoned): no wait is opened, so no begin line.
        assert_eq!(*w.lock_watched(&m, "the test lock"), 1);
        assert_eq!(w.open_count(), 0);
        assert!(rx.try_iter().next().is_none(), "an uncontended lock writes nothing");
        // Contended: the wait begins (a line with its label and location) and ends when the lock is given.
        let held = Mutex::new(());
        let guard = held.lock().unwrap();
        std::thread::scope(|s| {
            let h = s.spawn(|| drop(w.lock_watched(&held, "the held lock")));
            let (_, begin) = line(&rx, "begins: the held lock");
            assert!(begin.contains("wait_log.rs:"), "{begin}");
            drop(guard);
            h.join().unwrap();
        });
        assert_eq!(w.open_count(), 0);
    }
}
