//! Changes the OS may have lost: a rescan notice or an error from the source, and a watch that hears nothing for the
//! resync time, each run the action (which looks at the whole folder) and leave a log line with the cause (ER-27).

use paddock_jobs::time::FakeClock;
use paddock_testkit::wait::HANG_GUARD;
use paddock_watch::{DEFAULT_RESYNC, Debouncer, FakeSource, spawn};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// Every record the crate logged in this test binary, as `(level, message)`.
fn logged() -> &'static Mutex<Vec<(log::Level, String)>> {
    struct Capture;
    impl log::Log for Capture {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            if record.target().starts_with("paddock_watch") {
                RECORDS
                    .get_or_init(Mutex::default)
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push((record.level(), record.args().to_string()));
            }
        }
        fn flush(&self) {}
    }
    static RECORDS: OnceLock<Mutex<Vec<(log::Level, String)>>> = OnceLock::new();
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        log::set_boxed_logger(Box::new(Capture)).expect("one logger per test binary");
        log::set_max_level(log::LevelFilter::Info);
    });
    RECORDS.get_or_init(Mutex::default)
}

fn lines_with(text: &str) -> Vec<(log::Level, String)> {
    logged()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .filter(|(_, m)| m.contains(text))
        .cloned()
        .collect()
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn a_rescan_notice_runs_the_action_and_logs_its_cause() {
    logged();
    let clock = FakeClock::new();
    let d = Arc::new(Debouncer::new(Arc::new(clock.clone()), ms(300), ms(1500)));
    let source = FakeSource::new();
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let handle = spawn(d.clone(), &source, dir.path(), false, move || {
        let _ = tx.send(());
    })
    .unwrap();
    let cause = "rescan: kernel dropped (test 7f3a)";
    assert!(source.rescan(cause));
    while !d.has_pending() {
        std::thread::yield_now();
    }
    clock.advance_ms(300);
    rx.recv_timeout(HANG_GUARD)
        .expect("a rescan notice did not run the action");
    handle.stop();
    let lines = lines_with("test 7f3a");
    assert_eq!(lines.len(), 1, "one warning per notice: {lines:?}");
    assert_eq!(lines[0].0, log::Level::Warn);
    assert!(
        lines[0].1.contains("may have been missed")
            && lines[0].1.contains(&dir.path().display().to_string()),
        "the warning names the folder and the cause: {}",
        lines[0].1
    );
}

#[test]
fn a_plain_change_logs_no_rescan() {
    // The control for the test above: the capture sees the crate's records, and a plain change is not one.
    logged();
    let clock = FakeClock::new();
    let d = Arc::new(Debouncer::new(Arc::new(clock.clone()), ms(300), ms(1500)));
    let source = FakeSource::new();
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let handle = spawn(d.clone(), &source, dir.path(), false, move || {
        let _ = tx.send(());
    })
    .unwrap();
    assert!(source.change());
    while !d.has_pending() {
        std::thread::yield_now();
    }
    clock.advance_ms(300);
    rx.recv_timeout(HANG_GUARD).unwrap();
    handle.stop();
    let folder = dir.path().display().to_string();
    assert!(
        lines_with(&folder).is_empty(),
        "a plain change logged: {:?}",
        lines_with(&folder)
    );
}

#[test]
fn a_quiet_watch_still_looks_again_after_the_resync_time() {
    logged();
    let clock = FakeClock::new();
    let d = Arc::new(Debouncer::new(Arc::new(clock.clone()), ms(300), ms(1500)));
    let source = FakeSource::new();
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let handle = spawn(d, &source, dir.path(), false, move || {
        let _ = tx.send(());
    })
    .unwrap();
    // Not before the resync time: the pure test below pins that edge on the debouncer, without a timed window here.
    clock.advance(DEFAULT_RESYNC);
    rx.recv_timeout(HANG_GUARD)
        .expect("no resync after the resync time of silence");
    handle.stop();
    let folder = dir.path().display().to_string();
    let lines = lines_with(&folder);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].0, log::Level::Info);
    assert!(lines[0].1.contains("resync time"), "{}", lines[0].1);
}

#[test]
fn resync_waits_for_a_full_quiet_period_after_each_action_and_can_be_turned_off() {
    let clock = FakeClock::new();
    let d = Debouncer::new(Arc::new(clock.clone()), ms(300), ms(1500)).with_resync(Some(ms(1000)));
    assert!(!d.resync_due());
    clock.advance_ms(1000);
    assert!(d.resync_due());
    assert!(!d.resync_due(), "a resync counts as a run");
    // A change that ran the action moves the next resync a full period on.
    clock.advance_ms(500);
    d.note_change();
    assert!(!d.resync_due(), "no resync while a change is waiting");
    clock.advance_ms(300);
    assert_eq!(d.poll(), paddock_watch::Due::Now);
    clock.advance_ms(999);
    assert!(!d.resync_due());
    clock.advance_ms(1);
    assert!(d.resync_due());

    let off = Debouncer::new(Arc::new(clock.clone()), ms(300), ms(1500)).with_resync(None);
    clock.advance(Duration::from_secs(10 * 24 * 60 * 60));
    assert!(!off.resync_due());
}

#[cfg(feature = "notify")]
mod notify_results {
    use notify::event::{EventKind, Flag};
    use paddock_watch::{Change, NotifySource};

    #[test]
    fn an_overflow_or_a_dropped_events_notice_asks_for_a_rescan_with_its_cause() {
        // What notify 8.2's inotify backend sends for IN_Q_OVERFLOW: a bare Rescan flag.
        let overflow = notify::Event::new(EventKind::Other).set_flag(Flag::Rescan);
        assert_eq!(
            NotifySource::change_from(&Ok(overflow)),
            Change::Rescan("the OS dropped change events: its event queue overflowed".into())
        );
        // What its FSEvents backend sends for MustScanSubDirs with KernelDropped.
        let fsevents = notify::Event::new(EventKind::Other)
            .set_flag(Flag::Rescan)
            .set_info("rescan: kernel dropped");
        assert_eq!(
            NotifySource::change_from(&Ok(fsevents)),
            Change::Rescan("the OS dropped change events: rescan: kernel dropped".into())
        );
    }

    #[test]
    fn an_error_asks_for_a_rescan_and_a_plain_event_is_a_change() {
        let err = notify::Error::generic("the watch broke");
        match NotifySource::change_from(&Err(err)) {
            Change::Rescan(cause) => assert!(cause.contains("the watch broke"), "{cause}"),
            other => panic!("an error was {other:?}"),
        }
        let plain = notify::Event::new(EventKind::Create(notify::event::CreateKind::File));
        assert_eq!(NotifySource::change_from(&Ok(plain)), Change::Changed);
    }
}
