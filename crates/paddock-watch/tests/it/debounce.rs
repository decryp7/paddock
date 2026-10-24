use paddock_jobs::time::FakeClock;
use paddock_watch::{ChangeSource, Debouncer, Due, FakeSource, spawn};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn debouncer() -> (FakeClock, Arc<Debouncer>) {
    let clock = FakeClock::new();
    (
        clock.clone(),
        Arc::new(Debouncer::new(Arc::new(clock), ms(300), ms(1500))),
    )
}

#[test]
fn due_only_after_the_quiet_time() {
    let (clock, d) = debouncer();
    assert_eq!(d.poll(), Due::Idle);
    d.note_change();
    assert!(d.has_pending());
    assert_eq!(d.poll(), Due::Waiting(ms(300)));
    clock.advance_ms(299);
    assert_eq!(d.poll(), Due::Waiting(ms(1)));
    clock.advance_ms(1);
    assert_eq!(d.poll(), Due::Now);
    assert!(!d.has_pending());
    assert_eq!(d.poll(), Due::Idle);
}

#[test]
fn a_storm_is_one_due_and_a_burst_that_never_settles_is_cut_at_the_max_wait() {
    let (clock, d) = debouncer();
    for _ in 0..1000 {
        d.note_change();
        clock.advance_ms(1);
        assert!(matches!(d.poll(), Due::Waiting(_)));
    }
    clock.advance_ms(300);
    assert_eq!(d.poll(), Due::Now);

    let (clock, d) = debouncer();
    d.note_change();
    for _ in 0..14 {
        clock.advance_ms(100);
        d.note_change();
        assert!(matches!(d.poll(), Due::Waiting(_)));
    }
    clock.advance_ms(100);
    d.note_change();
    assert_eq!(d.poll(), Due::Now);
}

#[test]
fn clear_drops_the_waiting_change_and_max_wait_is_at_least_the_quiet_time() {
    let (_c, d) = debouncer();
    d.note_change();
    d.clear();
    assert_eq!(d.poll(), Due::Idle);
    let clock = FakeClock::new();
    let d = Debouncer::new(Arc::new(clock.clone()), ms(500), ms(100));
    d.note_change();
    clock.advance_ms(400);
    assert!(
        matches!(d.poll(), Due::Waiting(_)),
        "max_wait was raised to the quiet time"
    );
}

#[test]
fn the_thread_runs_the_action_once_per_burst_and_survives_a_panic() {
    let (clock, d) = debouncer();
    let source = FakeSource::new();
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let n = calls.clone();
    let handle = spawn(d.clone(), &source, dir.path(), false, move || {
        let k = n.fetch_add(1, Ordering::SeqCst);
        let _ = tx.send(k);
        if k == 0 {
            panic!("action bug");
        }
    })
    .unwrap();
    for round in 0..2 {
        // One event per round: with several, the thread could note a late one after the clock moved and wait again.
        assert!(source.change());
        while !d.has_pending() {
            std::thread::yield_now();
        }
        clock.advance_ms(300);
        assert_eq!(rx.recv_timeout(Duration::from_secs(20)).unwrap(), round);
    }
    handle.stop();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(!source.change(), "the watch is gone with the thread");
}

#[test]
fn a_source_that_cannot_start_or_a_folder_that_cannot_be_made_is_reported() {
    struct Refuses;
    impl ChangeSource for Refuses {
        fn watch(
            &self,
            _: &std::path::Path,
            _: bool,
            _: mpsc::Sender<()>,
        ) -> Result<Box<dyn Send>, paddock_watch::WatchError> {
            Err(paddock_watch::WatchError("no watches left".into()))
        }
    }
    let (_c, d) = debouncer();
    let dir = tempfile::tempdir().unwrap();
    let e = spawn(d.clone(), &Refuses, dir.path(), true, || {})
        .err()
        .unwrap();
    assert_eq!(e.to_string(), "no watches left");
    let file = dir.path().join("f");
    std::fs::write(&file, "x").unwrap();
    assert!(
        spawn(
            d.clone(),
            &FakeSource::new(),
            &file.join("sub"),
            false,
            || {}
        )
        .is_err()
    );
    // Dropping the handle stops the thread too.
    let h = spawn(
        d,
        &FakeSource::new(),
        &dir.path().join("made/here"),
        false,
        || {},
    )
    .unwrap();
    assert!(dir.path().join("made/here").is_dir());
    drop(h);
}

#[test]
fn a_source_that_goes_away_ends_the_thread() {
    struct Drops;
    impl ChangeSource for Drops {
        fn watch(
            &self,
            _: &std::path::Path,
            _: bool,
            tx: mpsc::Sender<()>,
        ) -> Result<Box<dyn Send>, paddock_watch::WatchError> {
            drop(tx);
            Ok(Box::new(()))
        }
    }
    let (_c, d) = debouncer();
    let dir = tempfile::tempdir().unwrap();
    let h = spawn(d, &Drops, dir.path(), false, || {}).unwrap();
    while !h.is_finished() {
        std::thread::yield_now(); // the thread leaves by itself: every sender is gone
    }
    h.stop();
}
