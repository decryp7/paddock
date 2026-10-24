use crate::common;
use common::*;
use paddock_jobs::time::{Clock, FakeClock};
use paddock_packs::watch::{self, FakeSource};
use paddock_packs::{Limits, PackLoader, PackStore, Polled, Reloader};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

fn pack(id: &str) -> String {
    format!("format = 1\n[[category]]\nid='{id}'\nname='{id}'\n")
}

fn setup() -> (
    MemSource,
    Arc<PackStore<Cleanup>>,
    FakeClock,
    Reloader<Cleanup>,
) {
    let src = MemSource::default();
    let store = Arc::new(PackStore::new(PackLoader::new(
        vec![],
        Box::new(src.clone()),
        Limits::default(),
    )));
    let clock = FakeClock::new();
    let r = Reloader::with_timing(
        store.clone(),
        Arc::new(clock.clone()),
        Duration::from_millis(300),
        Duration::from_millis(1500),
    );
    (src, store, clock, r)
}

#[test]
fn nothing_is_read_until_the_quiet_time_has_passed() {
    let (src, store, clock, r) = setup();
    assert_eq!(r.poll(), Polled::Idle);
    src.put("a.toml", &pack("a"));
    r.note_change();
    assert_eq!(r.poll(), Polled::Waiting(Duration::from_millis(300)));
    assert_eq!(src.reads(), 0);
    clock.advance_ms(299);
    assert_eq!(r.poll(), Polled::Waiting(Duration::from_millis(1)));
    clock.advance_ms(1);
    assert_eq!(r.poll(), Polled::Reloaded(true));
    assert_eq!(store.current().packs.len(), 1);
    assert_eq!(r.poll(), Polled::Idle);
}

#[test]
fn a_reload_storm_is_one_reload() {
    let (src, store, clock, r) = setup();
    let reloads = Arc::new(AtomicUsize::new(0));
    let n = reloads.clone();
    store.observe(move |_| {
        n.fetch_add(1, Ordering::SeqCst);
    });
    src.put("a.toml", &pack("a"));
    for _ in 0..1000 {
        r.note_change();
        clock.advance_ms(1);
        assert!(matches!(r.poll(), Polled::Waiting(_)));
    }
    clock.advance_ms(300);
    assert_eq!(r.poll(), Polled::Reloaded(true));
    assert_eq!(reloads.load(Ordering::SeqCst), 1);
    assert_eq!(src.reads(), 1);
}

#[test]
fn a_folder_that_never_settles_is_still_read_after_the_maximum_wait() {
    let (src, store, clock, r) = setup();
    src.put("a.toml", &pack("a"));
    r.note_change();
    for _ in 0..14 {
        clock.advance_ms(100);
        r.note_change();
        assert!(matches!(r.poll(), Polled::Waiting(_)));
    }
    clock.advance_ms(100); // 1500 ms after the first event
    r.note_change();
    assert_eq!(r.poll(), Polled::Reloaded(true));
    assert_eq!(store.current().revision, 1);
}

#[test]
fn a_storm_that_changes_nothing_swaps_nothing_and_tells_nobody() {
    let (src, store, clock, r) = setup();
    src.put("a.toml", &pack("a"));
    assert!(r.reload_now());
    let told = Arc::new(AtomicUsize::new(0));
    let n = told.clone();
    store.observe(move |_| {
        n.fetch_add(1, Ordering::SeqCst);
    });
    let before = store.current();
    for _ in 0..50 {
        r.note_change();
    }
    clock.advance_ms(300);
    assert_eq!(r.poll(), Polled::Reloaded(false));
    assert_eq!(told.load(Ordering::SeqCst), 0);
    assert!(Arc::ptr_eq(&before, &store.current()));
}

#[test]
fn readers_keep_their_snapshot_while_the_next_is_swapped_in() {
    let (src, store, _clock, r) = setup();
    src.put("a.toml", &pack("a"));
    r.reload_now();
    let held = store.current();
    src.put("b.toml", &pack("b"));
    r.reload_now();
    assert_eq!(held.packs.len(), 1);
    assert_eq!(store.current().packs.len(), 2);
    assert_eq!(store.current().revision, 2);
}

#[test]
fn a_pack_deleted_while_a_reload_is_waiting_is_gone_after_it() {
    let (src, store, clock, r) = setup();
    src.put("a.toml", &pack("a"));
    src.put("b.toml", &pack("b"));
    r.reload_now();
    src.remove("b.toml");
    r.note_change();
    src.vanish_on_read("a.toml"); // and a is deleted during the reload itself
    clock.advance_ms(300);
    assert_eq!(r.poll(), Polled::Reloaded(true));
    assert!(store.current().packs.is_empty());
    assert!(store.current().problems.is_empty());
}

#[test]
fn reload_now_drops_the_waiting_change() {
    let (src, _store, _clock, r) = setup();
    src.put("a.toml", &pack("a"));
    r.note_change();
    r.reload_now();
    assert_eq!(r.poll(), Polled::Idle);
}

#[test]
fn an_observer_that_panics_does_not_stop_the_reload_or_the_observers_after_it() {
    let (src, store, _clock, r) = setup();
    store.observe(|_| panic!("observer bug"));
    let told = Arc::new(AtomicUsize::new(0));
    let n = told.clone();
    store.observe(move |_| {
        n.fetch_add(1, Ordering::SeqCst);
    });
    src.put("a.toml", &pack("a"));
    assert!(r.reload_now());
    src.put("b.toml", &pack("b"));
    assert!(r.reload_now());
    assert_eq!(told.load(Ordering::SeqCst), 2);
    assert_eq!(store.current().packs.len(), 2);
}

#[test]
fn the_watcher_thread_applies_a_change_a_break_and_a_delete_without_a_restart() {
    let (src, store, clock, r) = setup();
    let r = Arc::new(r);
    let dir = tempfile::tempdir().unwrap();
    let source = FakeSource::new();
    let (tx, rx) = mpsc::channel();
    store.observe(move |s| {
        let _ = tx.send(s.clone());
    });
    let handle = watch::spawn(r.clone(), &source, dir.path()).unwrap();
    let guard = Duration::from_secs(20);
    let settle = |clock: &FakeClock| {
        // Let the thread note the events (it reads them in its own time), then move the clock past the quiet time.
        while !r.has_pending() {
            std::thread::yield_now();
        }
        clock.advance_ms(300);
    };

    // Added.
    src.put("a.toml", &pack("a"));
    assert!(source.change());
    settle(&clock);
    assert_eq!(rx.recv_timeout(guard).unwrap().packs.len(), 1);

    // Broken: the last good version stays, with a problem.
    src.put("a.toml", "format = 1\n[[");
    assert!(source.change());
    settle(&clock);
    let s = rx.recv_timeout(guard).unwrap();
    assert_eq!((s.packs.len(), s.problems.len()), (1, 1));

    // Deleted.
    src.remove("a.toml");
    assert!(source.change());
    settle(&clock);
    let s = rx.recv_timeout(guard).unwrap();
    assert!(s.packs.is_empty() && s.problems.is_empty());
    handle.stop();
    assert!(clock.monotonic() >= Duration::from_millis(900));
}

#[test]
fn dropping_the_handle_stops_the_thread_and_a_start_failure_is_reported() {
    let (_src, _store, _clock, r) = setup();
    let r = Arc::new(r);
    let dir = tempfile::tempdir().unwrap();
    let source = FakeSource::new();
    let h = watch::spawn(r.clone(), &source, &dir.path().join("made/by/spawn")).unwrap();
    assert!(dir.path().join("made/by/spawn").is_dir());
    drop(h);
    assert!(!source.change(), "the watch guard is gone with the thread");

    // A file where the folder should be: reported, nothing started.
    let file = dir.path().join("file");
    std::fs::write(&file, "x").unwrap();
    assert!(watch::spawn(r, &FakeSource::new(), &file.join("sub")).is_err());
}

#[test]
fn the_default_timing_is_the_documented_one() {
    let (_src, store, clock, _r) = setup();
    let r = Reloader::new(store, Arc::new(clock.clone()));
    r.note_change();
    assert_eq!(r.poll(), Polled::Waiting(Reloader::<Cleanup>::DEBOUNCE));
    assert_eq!(Reloader::<Cleanup>::MAX_WAIT, Duration::from_millis(1500));
}
