//! The orphan watch (ADR 0022 §3): if the app it serves is gone (dragged to the Bin, uninstalled) the helper removes itself,
//! but only after it saw the app missing twice an hour apart (an update's file swap never lasts that long). No polling: it
//! checks at start and on a file event for the app's parent folder, and the one-hour wait is a single timed wait on the
//! shared clock. [`Watch`] is the pure decision plus the one-shot timer; tests drive a fake clock and fake events.

use paddock_jobs::time::Clock;
use paddock_jobs::CancelToken;
use paddock_watch::{ChangeSource, Debouncer, WatchError, WatchHandle};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long the app must stay missing before the helper removes itself.
pub const GRACE: Duration = Duration::from_secs(3600);

/// What a check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The app is there (or was seen again): nothing to do.
    Keep,
    /// The app is missing; check again at this time on the clock.
    Armed(Duration),
    /// Missing since the deadline passed: remove the helper.
    Remove,
}

/// The pure decision: when was the app first seen missing?
#[derive(Debug, Default)]
pub struct Decision {
    missing_since: Option<Duration>,
}

impl Decision {
    /// Feeds one observation at `now`.
    pub fn observe(&mut self, app_present: bool, now: Duration, grace: Duration) -> Verdict {
        if app_present {
            self.missing_since = None;
            return Verdict::Keep;
        }
        let since = *self.missing_since.get_or_insert(now);
        if now >= since + grace {
            Verdict::Remove
        } else {
            Verdict::Armed(since + grace)
        }
    }
}

/// The app location the helper records at install: the `.app` bundle on macOS (the helper sits inside it), the install
/// folder on Windows. Linux has none (dpkg owns the package).
pub fn app_location(source: &Path) -> Option<PathBuf> {
    let bundle = source.ancestors().find(|p| p.extension().is_some_and(|e| e == "app"));
    if let Some(b) = bundle {
        return Some(b.to_path_buf());
    }
    if cfg!(windows) {
        return source.parent().map(Path::to_path_buf);
    }
    None
}

struct Inner {
    location: PathBuf,
    exists: Box<dyn Fn(&Path) -> bool + Send + Sync>,
    clock: Arc<dyn Clock>,
    grace: Duration,
    decision: Mutex<Decision>,
    timer_running: AtomicBool,
    cancel: CancelToken,
    on_remove: Box<dyn Fn() + Send + Sync>,
}

/// The watch over one app location.
#[derive(Clone)]
pub struct Watch(Arc<Inner>);

impl Watch {
    /// A watch that calls `on_remove` once the app has been missing for `grace`.
    pub fn new(
        location: PathBuf,
        exists: impl Fn(&Path) -> bool + Send + Sync + 'static,
        clock: Arc<dyn Clock>,
        grace: Duration,
        on_remove: impl Fn() + Send + Sync + 'static,
    ) -> Watch {
        Watch(Arc::new(Inner {
            location,
            exists: Box::new(exists),
            clock,
            grace,
            decision: Mutex::new(Decision::default()),
            timer_running: AtomicBool::new(false),
            cancel: CancelToken::new(),
            on_remove: Box::new(on_remove),
        }))
    }

    /// Checks the app now (at start, and on every file event). A missing app arms the one-shot wait.
    pub fn check(&self) -> Verdict {
        let inner = &self.0;
        let present = (inner.exists)(&inner.location);
        let now = inner.clock.monotonic();
        let verdict = inner
            .decision
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .observe(present, now, inner.grace);
        match verdict {
            Verdict::Remove => {
                log::warn!("Helper: the app is gone and stayed gone, so the helper removes itself");
                (inner.on_remove)();
            }
            Verdict::Armed(at) => {
                log::info!("Helper: the app is missing; it will be checked again once the grace period is over");
                self.arm(at);
            }
            Verdict::Keep => {}
        }
        verdict
    }

    fn arm(&self, at: Duration) {
        if self.0.timer_running.swap(true, Ordering::SeqCst) {
            return;
        }
        let watch = self.clone();
        let spawned = std::thread::Builder::new().name("helper-orphan".into()).spawn(move || {
            let wait = at.saturating_sub(watch.0.clock.monotonic());
            if watch.0.clock.wait(wait, &watch.0.cancel).is_ok() {
                watch.0.timer_running.store(false, Ordering::SeqCst);
                watch.check();
            }
        });
        if spawned.is_err() {
            self.0.timer_running.store(false, Ordering::SeqCst);
        }
    }

    /// Ends a pending wait (the daemon is stopping).
    pub fn stop(&self) {
        self.0.cancel.cancel();
    }

    /// Checks on every change in the app's parent folder.
    ///
    /// # Errors
    /// The watcher could not start.
    pub fn watch_parent(&self, source: &dyn ChangeSource) -> Result<Option<WatchHandle>, WatchError> {
        let Some(parent) = self.0.location.parent().map(Path::to_path_buf) else {
            return Ok(None);
        };
        let debouncer = Arc::new(Debouncer::new(
            self.0.clock.clone(),
            Duration::from_millis(500),
            Duration::from_secs(5),
        ));
        let watch = self.clone();
        paddock_watch::spawn(debouncer, source, &parent, false, move || {
            watch.check();
        })
        .map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_jobs::time::FakeClock;
    use paddock_watch::FakeSource;
    use std::sync::atomic::AtomicUsize;

    const HOUR: Duration = Duration::from_secs(3600);

    #[test]
    fn missing_twice_an_hour_apart_removes_and_seeing_it_again_resets() {
        let mut d = Decision::default();
        let t = |s| Duration::from_secs(s);
        assert_eq!(d.observe(true, t(0), HOUR), Verdict::Keep);
        assert_eq!(d.observe(false, t(10), HOUR), Verdict::Armed(t(10) + HOUR));
        assert_eq!(d.observe(false, t(20), HOUR), Verdict::Armed(t(10) + HOUR));
        assert_eq!(d.observe(true, t(30), HOUR), Verdict::Keep);
        assert_eq!(d.observe(false, t(40), HOUR), Verdict::Armed(t(40) + HOUR));
        assert_eq!(d.observe(false, t(40) + HOUR, HOUR), Verdict::Remove);
    }

    #[test]
    fn the_location_is_the_bundle_on_macos_and_the_folder_on_windows() {
        assert_eq!(
            app_location(Path::new("/Applications/Dyno.app/Contents/MacOS/dyno-helper")),
            Some(PathBuf::from("/Applications/Dyno.app"))
        );
        assert_eq!(app_location(Path::new("/usr/bin/dyno-helper")).is_some(), cfg!(windows));
    }

    fn watch(present: Arc<AtomicBool>, removed: Arc<AtomicUsize>, clock: Arc<FakeClock>) -> Watch {
        Watch::new(
            PathBuf::from("/Applications/Dyno.app"),
            move |_| present.load(Ordering::SeqCst),
            clock,
            HOUR,
            move || {
                removed.fetch_add(1, Ordering::SeqCst);
            },
        )
    }

    #[test]
    fn a_missing_app_is_removed_after_one_hour_on_the_clock_and_not_before() {
        let clock = Arc::new(FakeClock::default());
        let (present, removed) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicUsize::new(0)));
        let w = watch(present.clone(), removed.clone(), clock.clone());
        assert!(matches!(w.check(), Verdict::Armed(_)));
        while clock.pending() == 0 {
            std::thread::yield_now();
        }
        clock.advance(HOUR - Duration::from_secs(1));
        assert_eq!(
            (removed.load(Ordering::SeqCst), clock.pending()),
            (0, 1),
            "not yet: the wait is still running"
        );
        clock.advance(Duration::from_secs(1));
        while removed.load(Ordering::SeqCst) == 0 {
            std::thread::yield_now();
        }
        assert_eq!(removed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn an_app_that_comes_back_is_kept_and_a_stop_ends_the_wait() {
        let clock = Arc::new(FakeClock::default());
        let (present, removed) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicUsize::new(0)));
        let w = watch(present.clone(), removed.clone(), clock.clone());
        w.check();
        present.store(true, Ordering::SeqCst);
        assert_eq!(w.check(), Verdict::Keep);
        while clock.pending() == 0 {
            std::thread::yield_now();
        }
        w.stop();
        while clock.pending() != 0 {
            std::thread::yield_now();
        }
        assert_eq!(removed.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_file_event_in_the_parent_folder_checks_again() {
        let clock = Arc::new(FakeClock::default());
        let dir = paddock_testkit::private_tempdir().unwrap();
        let checks = Arc::new(AtomicUsize::new(0));
        let counter = checks.clone();
        let w = Watch::new(
            dir.path().join("Dyno.app"),
            move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
                true
            },
            clock.clone(),
            HOUR,
            || {},
        );
        let source = FakeSource::new();
        let handle = w.watch_parent(&source).unwrap().unwrap();
        assert!(source.change());
        while checks.load(Ordering::SeqCst) == 0 {
            clock.advance(Duration::from_secs(1));
            std::thread::yield_now();
        }
        drop(handle);
        assert!(Watch::new(PathBuf::from("/"), |_| true, clock, HOUR, || {})
            .watch_parent(&source)
            .unwrap()
            .is_none());
    }
}
