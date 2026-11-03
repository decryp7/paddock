//! The performance counters browser's engine (WP-12): finds, reads and remembers counters through the
//! [`CounterSource`] port. Every call into the port runs on its own thread under a deadline, so a hung provider
//! costs the user an error with a Retry, never a frozen agent; only two calls may be in flight at once. Observe-only.

use crate::cache::Cache;
use dyno_core::counters::{
    search, CounterCatalog, CounterHistory, CounterHit, CounterObject, CounterObjectList, CounterPath, CounterReading, CounterRing,
    CounterSeries, CounterUnit, RateTracker, RawCounter, MAX_HITS, MAX_READ,
};
use dyno_core::metrics::Reading;
use dyno_core::ports::{CounterSource, PlatformError};
use dyno_core::protocol::{AgentError, ErrorCode};
use dyno_core::time::{Timestamp, MINUTE};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// The key of the saved catalog in the cache (`view_saved`) and its document format.
const SAVED_KEY: &str = "counters.catalog";
const SAVED_FORMAT: u32 = 1;
/// How long the catalog is reused (it changes when a device appears or a program starts).
const CATALOG_TTL: Duration = Duration::from_secs(5 * 60);
/// How long listing everything may take on the user's click (a slow provider finishes into the cache for the retry).
const CATALOG_DEADLINE: Duration = Duration::from_secs(10);
/// How long reading a few counters may take.
const READ_DEADLINE: Duration = Duration::from_secs(5);
/// Calls into the OS that may be unfinished at once.
const MAX_IN_FLIGHT: usize = 2;
/// Counters whose history is kept (the least recently used go first).
const MAX_RINGS: usize = 48;
/// How old the newest reading of a pinned counter may be to still count as one (ms).
const LATEST_WITHIN: i64 = 10 * MINUTE;
/// The longest history window, in minutes.
pub const MAX_MINUTES: u32 = 60;
/// How often pinned counters are read in the background (ADR 0002, own overhead): slow, because a minute of history
/// at this pace is still a readable line and the OS call is not free.
pub const PINNED_PERIOD: Duration = Duration::from_secs(15);
/// How many pinned counters the background reading covers; the rest are read when the Counters page is open.
pub const MAX_PINNED_POLLED: usize = 8;

/// The pause between background readings: the base period, stretched in proportion to the overshoot when Dyno's own
/// CPU (percent of one core) is above its budget (like the sampling intervals, ADR 0002), at most four times.
pub fn pinned_period(own_cpu: f32) -> Duration {
    let budget = dyno_core::sampling::CPU_BUDGET;
    if own_cpu.is_finite() && own_cpu > budget {
        PINNED_PERIOD.mul_f32((own_cpu / budget).min(4.0))
    } else {
        PINNED_PERIOD
    }
}

struct Series {
    unit: CounterUnit,
    ring: CounterRing,
    used: Instant,
}

struct Inner {
    source: Arc<dyn CounterSource>,
    in_flight: AtomicUsize,
    catalog: Mutex<Option<(Instant, Arc<CounterCatalog>)>>,
    rates: Mutex<RateTracker>,
    rings: Mutex<HashMap<String, Series>>,
    /// The pinned counters the background reading covers, so a page's reads do not make the rate tracker forget them.
    pinned: Mutex<Vec<String>>,
    /// The listing of every counter while one runs (callers join it).
    listing: Mutex<Option<Arc<Listing>>>,
    /// The saved-views cache (ADR 0029), when there is one.
    cache: Mutex<Option<Cache>>,
    /// The catalog an earlier run saved and when: shown at once until the first listing of this run ends.
    saved: Mutex<Option<(Timestamp, Arc<CounterCatalog>)>>,
    /// A background listing for the saved catalog is under way.
    refreshing: AtomicBool,
    /// Its thread, so a test can wait for it to end.
    refresh_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

/// One listing of every counter: its result once it ended.
#[derive(Default)]
struct Listing {
    result: Mutex<Option<Result<Arc<CounterCatalog>, PlatformError>>>,
    done: Condvar,
}

/// The error of a call that did not end within `deadline`.
fn timed_out(what: &str, deadline: Duration) -> AgentError {
    AgentError::new(
        ErrorCode::Timeout,
        format!(
            "Windows, macOS or Linux took too long {what} (more than {} s); try again.",
            deadline.as_secs()
        ),
    )
}

/// Counts a call in flight until the thread running it ends, wherever that happens.
struct Slot(Arc<Inner>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The counters browser.
pub struct CounterCentre {
    inner: Arc<Inner>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn bad(msg: impl Into<String>) -> AgentError {
    AgentError::new(ErrorCode::BadRequest, msg)
}

/// Turns raw readings into values (running totals become rates) and adds them to their histories.
fn record(inner: &Inner, raws: Vec<RawCounter>, now: Timestamp) -> Vec<CounterReading> {
    let mut keep: Vec<String> = raws.iter().map(|r| r.path.clone()).collect();
    keep.extend(lock(&inner.pinned).iter().cloned());
    let readings: Vec<CounterReading> = {
        let mut rates = lock(&inner.rates);
        let out = raws.into_iter().map(|r| rates.apply(r, now)).collect();
        // Counters nobody asks for (the page and the pinned list) are forgotten, so the tracker cannot grow.
        rates.retain(&keep);
        out
    };
    let mut rings = lock(&inner.rings);
    for r in &readings {
        if let Some(v) = r.value.value() {
            if !rings.contains_key(&r.path) && rings.len() >= MAX_RINGS {
                if let Some(oldest) = rings.iter().min_by_key(|(_, s)| s.used).map(|(k, _)| k.clone()) {
                    rings.remove(&oldest);
                }
            }
            let s = rings.entry(r.path.clone()).or_insert_with(|| Series {
                unit: r.unit,
                ring: CounterRing::new(),
                used: Instant::now(),
            });
            s.unit = r.unit;
            s.used = Instant::now();
            s.ring.push(now, *v);
        }
    }
    readings
}

impl CounterCentre {
    /// A centre reading through `source`.
    pub fn new(source: Arc<dyn CounterSource>) -> CounterCentre {
        CounterCentre {
            inner: Arc::new(Inner {
                source,
                in_flight: AtomicUsize::new(0),
                catalog: Mutex::new(None),
                rates: Mutex::new(RateTracker::new()),
                rings: Mutex::new(HashMap::new()),
                pinned: Mutex::new(Vec::new()),
                listing: Mutex::new(None),
                cache: Mutex::new(None),
                saved: Mutex::new(None),
                refreshing: AtomicBool::new(false),
                refresh_thread: Mutex::new(None),
            }),
        }
    }

    /// Gives the centre the saved-views cache and reads the catalog an earlier run saved: the objects list then answers at once
    /// (with `saved_at`) while the first listing of this run happens in the background. A document that does not parse, or is
    /// of another format, is ignored.
    pub fn attach_cache(&self, cache: Option<Cache>) {
        let saved = cache.as_ref().and_then(|c| c.view(SAVED_KEY)).and_then(|(format, at, data)| {
            if format != SAVED_FORMAT {
                return None;
            }
            // Parse, then bound again: the file is not trusted more than the OS is.
            let parsed: CounterCatalog = serde_json::from_str(&data).ok()?;
            Some((Timestamp(at), Arc::new(CounterCatalog::bounded(parsed.objects))))
        });
        *lock(&self.inner.cache) = cache;
        *lock(&self.inner.saved) = saved;
    }

    /// Runs `f` on a thread and waits at most `deadline`. The thread keeps going after a timeout (its result is
    /// dropped or, for the catalog, cached).
    fn within<T: Send + 'static>(
        &self,
        deadline: Duration,
        what: &str,
        f: impl FnOnce(&Arc<Inner>) -> T + Send + 'static,
    ) -> Result<T, AgentError> {
        let (tx, rx) = mpsc::channel();
        self.start(what, move |inner| {
            let _ = tx.send(f(inner));
        })?;
        rx.recv_timeout(deadline).map_err(|_| timed_out(what, deadline))
    }

    /// Runs `f` on its own thread, counted as a call in flight until it ends; refused when [`MAX_IN_FLIGHT`] are.
    fn start(&self, what: &str, f: impl FnOnce(&Arc<Inner>) + Send + 'static) -> Result<(), AgentError> {
        if self.inner.in_flight.fetch_add(1, Ordering::SeqCst) >= MAX_IN_FLIGHT {
            self.inner.in_flight.fetch_sub(1, Ordering::SeqCst);
            return Err(AgentError::new(
                ErrorCode::Busy,
                format!("The system is still busy {what}; try again in a moment."),
            ));
        }
        let slot = Slot(self.inner.clone());
        let inner = self.inner.clone();
        let spawned = std::thread::Builder::new().name("dyno-counters".into()).spawn(move || {
            // The user waits for it on the Counters page: live work (rule 52).
            dyno_platform::priority::set_current_thread(dyno_platform::priority::ThreadRole::Live);
            let _slot = slot;
            f(&inner);
        });
        if let Err(e) = spawned {
            self.inner.in_flight.fetch_sub(1, Ordering::SeqCst);
            return Err(AgentError::new(ErrorCode::Internal, format!("Dyno could not start a reader: {e}")));
        }
        Ok(())
    }

    fn catalog(&self) -> Result<Arc<CounterCatalog>, AgentError> {
        self.catalog_with_origin().map(|(c, _)| c)
    }

    /// The catalog, and when it was saved if this is the one an earlier run saved. Before the first listing of this run a saved
    /// catalog is answered at once and the listing runs in the background; otherwise as always: the catalog of the last five
    /// minutes, or a listing the caller waits for.
    fn catalog_with_origin(&self) -> Result<(Arc<CounterCatalog>, Option<Timestamp>), AgentError> {
        let fresh = lock(&self.inner.catalog).as_ref().is_some_and(|(at, _)| at.elapsed() < CATALOG_TTL);
        if !fresh {
            let first_of_this_run = lock(&self.inner.catalog).is_none();
            let saved = lock(&self.inner.saved).clone();
            if let (true, Some((at, c))) = (first_of_this_run, saved) {
                self.list_in_background();
                return Ok((c, Some(at)));
            }
        }
        self.catalog_within(CATALOG_DEADLINE).map(|c| (c, None))
    }

    /// Lists everything on a thread nobody waits for; the saved catalog is replaced when it ends. One at a time.
    fn list_in_background(&self) {
        if self.inner.refreshing.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = CounterCentre { inner: self.inner.clone() };
        let spawned = std::thread::Builder::new().name("dyno-counters-refresh".into()).spawn(move || {
            let outcome = me.catalog_within(CATALOG_DEADLINE);
            match outcome {
                Ok(_) => log::info!("Counter catalog listed again in the background"),
                Err(e) => log::warn!(Cause = e.message.as_str(); "Counter catalog could not be listed again; the saved one stays; next step: open the page again"),
            }
            me.inner.refreshing.store(false, Ordering::SeqCst);
        });
        match spawned {
            Ok(h) => *lock(&self.inner.refresh_thread) = Some(h),
            Err(_) => self.inner.refreshing.store(false, Ordering::SeqCst),
        }
    }

    /// Waits for the background listing of the saved catalog to end, if one is running.
    pub fn wait_background_listing(&self) {
        let handle = lock(&self.inner.refresh_thread).take();
        if let Some(h) = handle {
            let _ = h.join();
        }
    }

    fn catalog_within(&self, deadline: Duration) -> Result<Arc<CounterCatalog>, AgentError> {
        if let Some((at, c)) = lock(&self.inner.catalog).as_ref() {
            if at.elapsed() < CATALOG_TTL {
                return Ok(c.clone());
            }
        }
        const WHAT: &str = "listing the performance counters";
        // One listing at a time: a retry after a timeout waits for the listing still running instead of starting another
        // (on a busy machine a fresh one would time out again, and every retry would add one more).
        let listing = {
            let mut running = lock(&self.inner.listing);
            match running.as_ref() {
                Some(l) => l.clone(),
                None => {
                    let l = Arc::new(Listing::default());
                    let mine = l.clone();
                    self.start(WHAT, move |inner| {
                        let found = inner.source.catalog().map(|c| Arc::new(CounterCatalog::bounded(c.objects)));
                        if let Ok(c) = &found {
                            *lock(&inner.catalog) = Some((Instant::now(), c.clone()));
                            // Listed in this run: the saved one is no longer the latest, and this one is kept for the next run.
                            *lock(&inner.saved) = None;
                            if let (Some(cache), Ok(json)) = (lock(&inner.cache).clone(), serde_json::to_string(&**c)) {
                                cache.put_view(SAVED_KEY, SAVED_FORMAT, json);
                            }
                        }
                        // Ended: a later call starts afresh (after a failure, too); callers already waiting get this result.
                        *lock(&inner.listing) = None;
                        *lock(&mine.result) = Some(found);
                        mine.done.notify_all();
                    })?;
                    *running = Some(l.clone());
                    l
                }
            }
        };
        let result = lock(&listing.result);
        let (result, _) = listing
            .done
            .wait_timeout_while(result, deadline, |r| r.is_none())
            .unwrap_or_else(|e| e.into_inner());
        match result.as_ref() {
            Some(found) => found.clone().map_err(AgentError::from),
            None => Err(timed_out(WHAT, deadline)),
        }
    }

    /// The first level of the browser.
    pub fn objects(&self) -> Result<CounterObjectList, AgentError> {
        let (catalog, saved_at) = self.catalog_with_origin()?;
        let mut list = catalog.summaries();
        list.saved_at = saved_at;
        Ok(list)
    }

    /// One object with its counters and instances.
    pub fn object(&self, name: &str) -> Result<CounterObject, AgentError> {
        self.catalog()?.object(name).cloned().ok_or_else(|| {
            AgentError::new(
                ErrorCode::NotFound,
                format!("This computer has no counter object named \"{name}\" (any more)."),
            )
        })
    }

    /// Counters matching the words.
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<CounterHit>, AgentError> {
        if query.chars().count() > 200 {
            return Err(bad("Type fewer than 200 characters to search."));
        }
        let limit = usize::try_from(limit).unwrap_or(MAX_HITS).clamp(1, MAX_HITS);
        let catalog = self.catalog()?;
        Ok(search(&catalog, query, limit))
    }

    /// Parses the paths of a request: at most [`MAX_READ`], each valid.
    pub fn parse(paths: &[String]) -> Result<Vec<CounterPath>, AgentError> {
        if paths.len() > MAX_READ {
            return Err(bad(format!("Ask for at most {MAX_READ} counters at a time.")));
        }
        paths
            .iter()
            .map(|p| p.parse::<CounterPath>().map_err(|e| bad(format!("{p}: {e}"))))
            .collect()
    }

    /// Reads the counters now, turns running totals into rates and adds the readings to their history.
    pub fn values(&self, paths: &[String], now: Timestamp) -> Result<Vec<CounterReading>, AgentError> {
        let parsed = Self::parse(paths)?;
        if parsed.is_empty() {
            return Ok(Vec::new());
        }
        let raws: Vec<RawCounter> = self.within(READ_DEADLINE, "reading the performance counters", move |inner| {
            inner.source.read(&parsed)
        })?;
        Ok(record(&self.inner, raws, now))
    }

    /// Reads the pinned counters (at most [`MAX_PINNED_POLLED`]) in the background so they have history before anyone
    /// opens the page. Never waits: when two OS calls are already in flight, or nothing valid is pinned, it does nothing
    /// and says so. The readings go to the same history as the page's reads; nothing else is stored.
    pub fn poll_pinned(&self, pinned: &[String], now: Timestamp) -> bool {
        let parsed: Vec<CounterPath> = pinned.iter().filter_map(|p| p.parse().ok()).take(MAX_PINNED_POLLED).collect();
        *lock(&self.inner.pinned) = parsed.iter().map(ToString::to_string).collect();
        if parsed.is_empty() || self.inner.in_flight.fetch_add(1, Ordering::SeqCst) >= MAX_IN_FLIGHT {
            if !parsed.is_empty() {
                self.inner.in_flight.fetch_sub(1, Ordering::SeqCst);
            }
            return false;
        }
        let slot = Slot(self.inner.clone());
        let inner = self.inner.clone();
        let spawned = std::thread::Builder::new().name("dyno-counters-pinned".into()).spawn(move || {
            // It holds one of the two slots the page's requests use, and the live sampler starts it: live work (rule 52).
            dyno_platform::priority::set_current_thread(dyno_platform::priority::ThreadRole::Live);
            let _slot = slot;
            let raws = inner.source.read(&parsed);
            record(&inner, raws, now);
        });
        spawned.is_ok() || {
            self.inner.in_flight.fetch_sub(1, Ordering::SeqCst);
            false
        }
    }

    /// The newest kept reading of each of `paths` (those that parse), without asking the OS, and when the newest was read:
    /// the popup's pinned counters read this on open and on every tick. A counter with no reading yet says so.
    pub fn latest(&self, paths: &[String], now: Timestamp) -> (Vec<CounterReading>, Option<Timestamp>) {
        let rings = lock(&self.inner.rings);
        let mut newest: Option<Timestamp> = None;
        let readings = paths
            .iter()
            .filter_map(|p| p.parse::<CounterPath>().ok())
            .map(|p| {
                let path = p.to_string();
                match rings.get(&path) {
                    Some(s) => CounterReading::new(
                        path,
                        s.unit,
                        match s.ring.since(now.minus(LATEST_WITHIN)).last() {
                            Some(point) => {
                                newest = newest.max(Some(point.at));
                                Reading::Available(point.value)
                            }
                            None => Reading::Unavailable("No recent reading.".into()),
                        },
                    ),
                    None => CounterReading::new(
                        path,
                        CounterUnit::guess(p.counter()),
                        Reading::Unavailable("No reading yet.".into()),
                    ),
                }
            })
            .collect();
        (readings, newest)
    }

    /// The readings kept over the last `minutes` (1 to [`MAX_MINUTES`]) for each path; `events` are the app's own
    /// events in that window, which the caller reads from the store.
    pub fn history(
        &self,
        paths: &[String],
        minutes: u32,
        now: Timestamp,
        events: impl FnOnce(Timestamp, Timestamp) -> Result<Vec<dyno_core::protocol::TimelineEvent>, AgentError>,
    ) -> Result<CounterHistory, AgentError> {
        let parsed = Self::parse(paths)?;
        let minutes = minutes.clamp(1, MAX_MINUTES);
        let from = now.minus(i64::from(minutes) * MINUTE);
        let rings = lock(&self.inner.rings);
        let series = parsed
            .iter()
            .map(|p| {
                let path = p.to_string();
                match rings.get(&path) {
                    Some(s) => CounterSeries {
                        points: s.ring.since(from),
                        unit: s.unit,
                        path,
                    },
                    None => CounterSeries {
                        unit: CounterUnit::guess(p.counter()),
                        points: Vec::new(),
                        path,
                    },
                }
            })
            .collect();
        drop(rings);
        Ok(CounterHistory {
            from,
            to: now,
            series,
            events: events(from, now)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::counters::{CounterKind, CounterUnit as U};
    use dyno_core::metrics::Reading;
    use dyno_platform::counters::{Entry, FakeCounters};

    fn entry(inst: Option<&str>, name: &str, kind: CounterKind, v: f64) -> Entry {
        Entry::new("Network Interface", inst, name, U::Bytes, kind, v)
    }

    fn centre() -> (CounterCentre, Arc<FakeCounters>) {
        let fake = Arc::new(FakeCounters::new(vec![
            entry(Some("en0"), "Bytes Received", CounterKind::Cumulative, 1000.0),
            entry(Some("lo0"), "Bytes Received", CounterKind::Cumulative, 5.0),
            Entry::new("Memory", None, "Free", U::Bytes, CounterKind::Gauge, 42.0),
        ]));
        (CounterCentre::new(fake.clone()), fake)
    }

    fn no_events(_: Timestamp, _: Timestamp) -> Result<Vec<dyno_core::protocol::TimelineEvent>, AgentError> {
        Ok(vec![])
    }

    #[test]
    fn it_lists_objects_opens_one_and_searches() {
        let (c, _) = centre();
        assert_eq!(c.objects().unwrap().objects.len(), 2);
        assert_eq!(c.object("Network Interface").unwrap().instances, ["en0", "lo0"]);
        assert_eq!(c.object("Nope").unwrap_err().code, ErrorCode::NotFound);
        let hits = c.search("received", 10).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(c.search("free", 0).unwrap().len(), 1);
        assert_eq!(c.search(&"x".repeat(201), 5).unwrap_err().code, ErrorCode::BadRequest);
    }

    #[test]
    fn values_turn_running_totals_into_rates_and_fill_the_history() {
        let (c, fake) = centre();
        let p = vec!["\\Network Interface(en0)\\Bytes Received".to_string(), "\\Memory\\Free".to_string()];
        let first = c.values(&p, Timestamp(1_000)).unwrap();
        assert!(matches!(first[0].value, Reading::Unavailable(_)));
        assert_eq!(first[1].value, Reading::Available(42.0));
        fake.set(vec![
            entry(Some("en0"), "Bytes Received", CounterKind::Cumulative, 3000.0),
            Entry::new("Memory", None, "Free", U::Bytes, CounterKind::Gauge, 40.0),
        ]);
        let second = c.values(&p, Timestamp(3_000)).unwrap();
        assert_eq!(second[0].value, Reading::Available(1000.0));
        assert_eq!(second[0].unit, U::BytesPerSec);

        let ev = dyno_core::protocol::TimelineEvent {
            at: Timestamp(2_000),
            kind: dyno_core::protocol::TimelineEventKind::Alert,
            label: "Alert raised".into(),
        };
        let h = c
            .history(&p, 5, Timestamp(3_500), |from, to| {
                assert_eq!(to, Timestamp(3_500));
                assert_eq!(from, Timestamp(3_500 - 5 * 60_000));
                Ok(vec![ev.clone()])
            })
            .unwrap();
        assert_eq!(h.series[0].points.len(), 1);
        assert_eq!(h.series[1].points.len(), 2);
        assert_eq!(h.events, vec![ev]);
    }

    #[test]
    fn a_counter_that_disappears_is_unavailable_with_the_reason_and_the_others_keep_working() {
        let (c, fake) = centre();
        let p = vec!["\\Network Interface(en0)\\Bytes Received".to_string(), "\\Memory\\Free".to_string()];
        c.values(&p, Timestamp(1_000)).unwrap();
        fake.set(vec![Entry::new("Memory", None, "Free", U::Bytes, CounterKind::Gauge, 1.0)]);
        let r = c.values(&p, Timestamp(2_000)).unwrap();
        assert!(matches!(&r[0].value, Reading::Unavailable(w) if w.contains("en0")));
        assert_eq!(r[1].value, Reading::Available(1.0));
    }

    #[test]
    fn requests_are_validated() {
        let (c, _) = centre();
        assert_eq!(
            c.values(&["nonsense".into()], Timestamp(0)).unwrap_err().code,
            ErrorCode::BadRequest
        );
        let many: Vec<String> = (0..=MAX_READ).map(|i| format!("\\a\\b{i}")).collect();
        assert_eq!(c.values(&many, Timestamp(0)).unwrap_err().code, ErrorCode::BadRequest);
        assert_eq!(
            c.values(&["\\Processor(*)\\x".into()], Timestamp(0)).unwrap_err().code,
            ErrorCode::BadRequest
        );
        assert!(c.values(&[], Timestamp(0)).unwrap().is_empty());
        assert!(c.history(&["bad".into()], 5, Timestamp(0), no_events).is_err());
    }

    #[test]
    fn the_history_window_is_clamped_and_unread_counters_have_empty_series() {
        let (c, _) = centre();
        let h = c
            .history(&["\\Memory\\Free".into()], 10_000, Timestamp(100 * 60_000), no_events)
            .unwrap();
        assert_eq!(h.from, Timestamp(40 * 60_000));
        assert!(h.series[0].points.is_empty());
        assert_eq!(h.series[0].unit, U::Number, "the unit is unknown until the counter has been read");
        let h = c.history(&[], 0, Timestamp(120_000), no_events).unwrap();
        assert_eq!(h.from, Timestamp(60_000));
    }

    #[test]
    fn a_failing_catalog_is_an_error_and_recovers() {
        let (c, fake) = centre();
        fake.fail_catalog(Some(PlatformError::AccessDenied("Run as administrator.".into())));
        let e = c.objects().unwrap_err();
        assert_eq!((e.code, e.message.as_str()), (ErrorCode::AccessDenied, "Run as administrator."));
        fake.fail_catalog(None);
        assert!(c.objects().is_ok());
    }

    #[test]
    fn the_catalog_is_cached() {
        let (c, fake) = centre();
        c.objects().unwrap();
        fake.fail_catalog(Some(PlatformError::Failed("gone".into())));
        assert!(c.objects().is_ok(), "the second look reuses the cached catalog");
    }

    #[test]
    fn the_history_of_many_counters_is_bounded() {
        let fake = Arc::new(FakeCounters::new(
            (0..MAX_RINGS + 10)
                .map(|i| Entry::new("O", None, &format!("c{i}"), U::Count, CounterKind::Gauge, 1.0))
                .collect(),
        ));
        let c = CounterCentre::new(fake);
        for chunk in (0..MAX_RINGS + 10).collect::<Vec<_>>().chunks(MAX_READ) {
            let p: Vec<String> = chunk.iter().map(|i| format!("\\O\\c{i}")).collect();
            c.values(&p, Timestamp(1)).unwrap();
        }
        assert_eq!(lock(&c.inner.rings).len(), MAX_RINGS);
    }

    #[test]
    fn pinned_counters_are_read_in_the_background_at_a_bounded_and_stretching_pace() {
        let (c, _) = centre();
        assert!(!c.poll_pinned(&[], Timestamp(1)), "nothing pinned, nothing read");
        assert!(
            !c.poll_pinned(&["junk".into()], Timestamp(1)),
            "an invalid path is skipped, not an error"
        );
        assert!(c.poll_pinned(&["\\Memory\\Free".into()], Timestamp(5_000)));
        let mut points = 0;
        for _ in 0..200 {
            let h = c.history(&["\\Memory\\Free".into()], 5, Timestamp(6_000), no_events).unwrap();
            points = h.series[0].points.len();
            if points > 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(points, 1, "the background reading filled the history");
        assert_eq!(pinned_period(0.2), PINNED_PERIOD);
        assert_eq!(pinned_period(f32::NAN), PINNED_PERIOD);
        assert_eq!(pinned_period(2.0), PINNED_PERIOD * 2);
        assert_eq!(pinned_period(50.0), PINNED_PERIOD * 4, "stretched at most four times");
    }

    #[test]
    fn the_background_reading_covers_at_most_the_first_few_pinned_and_never_piles_up() {
        let many: Vec<String> = (0..20).map(|i| format!("\\O\\c{i}")).collect();
        let fake = Arc::new(FakeCounters::new(
            (0..20)
                .map(|i| Entry::new("O", None, &format!("c{i}"), U::Count, CounterKind::Gauge, 1.0))
                .collect(),
        ));
        let c = CounterCentre::new(fake);
        assert!(c.poll_pinned(&many, Timestamp(1)));
        assert_eq!(lock(&c.inner.pinned).len(), MAX_PINNED_POLLED);
        let hang = Arc::new(Hanging(Mutex::new(false)));
        let c = CounterCentre::new(hang.clone());
        let p = vec!["\\O\\c1".to_string()];
        assert!(c.poll_pinned(&p, Timestamp(1)));
        assert!(c.poll_pinned(&p, Timestamp(2)));
        assert!(
            !c.poll_pinned(&p, Timestamp(3)),
            "two reads are stuck: the third is skipped, nothing waits"
        );
        assert_eq!(c.inner.in_flight.load(Ordering::SeqCst), 2);
        *lock(&hang.0) = true;
    }

    struct Hanging(Mutex<bool>);

    impl CounterSource for Hanging {
        fn catalog(&self) -> Result<CounterCatalog, PlatformError> {
            while !*lock(&self.0) {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(CounterCatalog::default())
        }

        fn read(&self, _: &[CounterPath]) -> Vec<RawCounter> {
            self.catalog().ok();
            vec![]
        }
    }

    #[test]
    fn a_hung_provider_times_out_and_at_most_two_calls_pile_up() {
        let hang = Arc::new(Hanging(Mutex::new(false)));
        let c = CounterCentre::new(hang.clone());
        let started = Instant::now();
        let one = c.within(Duration::from_millis(50), "listing", |i| i.source.catalog().is_ok());
        assert_eq!(one.unwrap_err().code, ErrorCode::Timeout);
        let two = c.within(Duration::from_millis(50), "listing", |i| i.source.catalog().is_ok());
        assert_eq!(two.unwrap_err().code, ErrorCode::Timeout);
        let three = c.within(Duration::from_millis(50), "listing", |i| i.source.catalog().is_ok());
        assert_eq!(three.unwrap_err().code, ErrorCode::Busy);
        assert!(started.elapsed() < Duration::from_secs(2));
        *lock(&hang.0) = true;
        // The stuck calls finish, free their slots, and the centre works again.
        let mut ok = false;
        for _ in 0..100 {
            if c.within(Duration::from_millis(500), "listing", |i| i.source.catalog().is_ok())
                .is_ok()
            {
                ok = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(ok);
    }

    /// Counts its listings; each waits until released.
    struct SlowCatalog {
        calls: AtomicUsize,
        released: Mutex<bool>,
        changed: std::sync::Condvar,
    }

    impl CounterSource for SlowCatalog {
        fn catalog(&self) -> Result<CounterCatalog, PlatformError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let g = lock(&self.released);
            drop(self.changed.wait_while(g, |r| !*r).unwrap_or_else(|e| e.into_inner()));
            FakeCounters::new(vec![Entry::new("Memory", None, "Free", U::Bytes, CounterKind::Gauge, 1.0)]).catalog()
        }

        fn read(&self, _: &[CounterPath]) -> Vec<RawCounter> {
            vec![]
        }
    }

    /// Regression (Windows CI run 7166): listing every counter took longer than the deadline on a busy machine, and the
    /// retry the error asks for started a second full listing instead of waiting for the one still running, so on a slow
    /// machine no retry could ever finish in time. A retry joins the running listing, and gets its result.
    #[test]
    fn a_retry_waits_for_the_listing_still_running_instead_of_listing_again() {
        let src = Arc::new(SlowCatalog {
            calls: AtomicUsize::new(0),
            released: Mutex::new(false),
            changed: std::sync::Condvar::new(),
        });
        let c = CounterCentre::new(src.clone());
        let first = c.catalog_within(Duration::from_millis(50));
        assert_eq!(first.unwrap_err().code, ErrorCode::Timeout);
        let retry = c.catalog_within(Duration::from_millis(50));
        assert_eq!(retry.unwrap_err().code, ErrorCode::Timeout);
        assert_eq!(src.calls.load(Ordering::SeqCst), 1, "the retry joined the running listing");
        *lock(&src.released) = true;
        src.changed.notify_all();
        // Bounded far above any scheduling delay (a hang guard): the running listing ends and the retry gets it.
        let listed = c.catalog_within(Duration::from_secs(20)).unwrap();
        assert!(listed.object("Memory").is_some());
        assert_eq!(src.calls.load(Ordering::SeqCst), 1, "listed once");
    }

    fn cache_in(dir: &std::path::Path, clock: Arc<crate::cache::testing::FakeClock>) -> Cache {
        Cache::start(
            dir.join("cache.db"),
            crate::cache::CacheSettings::default(),
            crate::cache::CacheDeps {
                clock,
                free: Arc::new(crate::cache::db::tests::Roomy),
            },
        )
        .cache
        .unwrap()
    }

    /// ADR 0029 (D7): the catalog an earlier run saved is shown at once, with its time, while the first listing of this run
    /// happens in the background; when that ends the answer is the fresh one and is saved for the next run.
    #[test]
    fn the_saved_catalog_is_shown_at_once_and_replaced_when_the_listing_ends() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(crate::cache::testing::FakeClock::new(5_000));
        // The first run lists and saves.
        let first = CounterCentre::new(Arc::new(FakeCounters::new(vec![Entry::new(
            "Memory",
            None,
            "Free",
            U::Bytes,
            CounterKind::Gauge,
            1.0,
        )])));
        let cache = cache_in(dir.path(), clock.clone());
        first.attach_cache(Some(cache.clone()));
        let listed = first.objects().unwrap();
        assert_eq!((listed.objects.len(), listed.saved_at), (1, None), "read just now");
        cache.flush();
        drop(first);
        drop(cache);
        // The second run: the source is slow; the saved list answers at once.
        clock.advance(86_400_000);
        let src = Arc::new(SlowCatalog {
            calls: AtomicUsize::new(0),
            released: Mutex::new(false),
            changed: std::sync::Condvar::new(),
        });
        let second = CounterCentre::new(src.clone());
        let cache = cache_in(dir.path(), clock);
        second.attach_cache(Some(cache.clone()));
        let at_once = second.objects().unwrap();
        assert_eq!(at_once.saved_at, Some(Timestamp(5_000)), "the time it was saved");
        assert_eq!(at_once.objects[0].name, "Memory");
        assert!(
            second.object("Memory").is_ok(),
            "the objects below it come from the saved catalog too"
        );
        assert!(!second.search("free", 5).unwrap().is_empty());
        *lock(&src.released) = true;
        src.changed.notify_all();
        // The background listing ends (joined, not polled): the next answer is fresh.
        second.wait_background_listing();
        assert_eq!(second.objects().unwrap().saved_at, None);
        assert_eq!(src.calls.load(Ordering::SeqCst), 1, "listed once, in the background");
        cache.flush();
        assert_eq!(
            cache.view(SAVED_KEY).unwrap().1,
            86_405_000,
            "saved again with the time of the second run"
        );
    }

    #[test]
    fn a_saved_catalog_that_does_not_parse_or_has_another_format_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(crate::cache::testing::FakeClock::new(1));
        let cache = cache_in(dir.path(), clock);
        for (format, data) in [
            (1u32, "{not json".to_string()),
            (2, "{\"objects\":[],\"truncated\":false}".to_string()),
        ] {
            cache.put_view(SAVED_KEY, format, data);
            cache.flush();
            let c = CounterCentre::new(Arc::new(FakeCounters::new(vec![Entry::new(
                "Memory",
                None,
                "Free",
                U::Bytes,
                CounterKind::Gauge,
                1.0,
            )])));
            c.attach_cache(Some(cache.clone()));
            let l = c.objects().unwrap();
            assert_eq!(l.saved_at, None, "listed, not taken from the file");
            assert_eq!(l.objects.len(), 1);
        }
    }

    /// Records the thread role every call into the OS ran at.
    struct Roles(Mutex<Vec<Option<dyno_platform::priority::ThreadRole>>>);

    impl CounterSource for Roles {
        fn catalog(&self) -> Result<CounterCatalog, PlatformError> {
            lock(&self.0).push(dyno_platform::priority::current_role());
            FakeCounters::new(vec![Entry::new("Memory", None, "Free", U::Bytes, CounterKind::Gauge, 1.0)]).catalog()
        }

        fn read(&self, _: &[CounterPath]) -> Vec<RawCounter> {
            lock(&self.0).push(dyno_platform::priority::current_role());
            vec![]
        }
    }

    /// Rule 52 (coordinator review, 2026-10-30): the Counters page's requests and the pinned reading hold the same two
    /// slots, and both are live work. A bulk pinned read stalled behind busy normal threads on Windows would make the
    /// user's request come back Busy.
    #[test]
    fn requests_and_the_pinned_reading_run_as_live_work() {
        use dyno_platform::priority::ThreadRole;
        let src = Arc::new(Roles(Mutex::new(Vec::new())));
        let c = CounterCentre::new(src.clone());
        c.objects().unwrap();
        c.values(&["\\Memory\\Free".into()], Timestamp(1)).unwrap();
        assert!(c.poll_pinned(&["\\Memory\\Free".into()], Timestamp(2)));
        // The background reading ends on its own thread: wait for it (a hang guard bounds the loop).
        for _ in 0..500 {
            if lock(&src.0).len() == 3 && c.inner.in_flight.load(Ordering::SeqCst) == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(*lock(&src.0), vec![Some(ThreadRole::Live); 3]);
    }
}
