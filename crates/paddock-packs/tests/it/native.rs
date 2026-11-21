//! The one real-folder test: a pack file added, broken and removed on disk is applied by the real watcher. Waits on
//! events with a hang guard, never on a fixed time.
//!
//! FSEvents (macOS) reports the first change after a new watch late, and a saturated fseventsd drops some (measured in
//! `paddock-watch`'s real-folder test, runs 8389 and 8466). So the watch is proven live first: a probe file the loader
//! ignores (not `.toml`) is rewritten until the store has reloaded once. Every wait checks the store's content, so a late
//! event cannot stand in for the change it waits for.
#![cfg(feature = "watch")]

use crate::common;
use common::*;
use paddock_jobs::time::SystemClock;
use paddock_packs::watch::{self, NotifySource};
use paddock_packs::{DirSource, Limits, PackLoader, PackStore, Reloader};
use paddock_testkit::wait::retry_within;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn the_real_watcher_applies_an_added_a_broken_and_a_deleted_pack() {
    let t = tempfile::tempdir().unwrap();
    // The OS may report the folder under a canonical name; the watch is on the path we made.
    let dir = t.path().join("packs");
    let store = Arc::new(PackStore::new(PackLoader::<Cleanup>::new(
        vec![],
        Box::new(DirSource::new(&dir)),
        Limits::default(),
    )));
    let reloader = Arc::new(Reloader::with_timing(
        store.clone(),
        Arc::new(SystemClock),
        Duration::ZERO,
        Duration::ZERO,
    ));
    let (tx, rx) = mpsc::channel();
    store.observe(move |s| {
        let _ = tx.send(s.clone());
    });
    let handle = watch::spawn(reloader, &NotifySource, &dir).unwrap();
    let guard = Duration::from_secs(30);
    let hint = "on macOS, a saturated fseventsd delays or drops events: check `ps -o pcpu,rss -p $(pgrep -x fseventsd)`";
    // Every write is a new change, so one the stream missed while fseventsd registered it is followed by one made after.
    let probe = dir.join(".probe");
    let mut writes = 0u32;
    let live = retry_within(guard, Duration::from_millis(250), || {
        if rx.try_recv().is_ok() {
            return true;
        }
        writes += 1;
        std::fs::write(&probe, writes.to_string()).unwrap();
        false
    });
    assert!(
        live,
        "the watch reported none of {writes} probe writes within {guard:?} ({hint})"
    );
    while rx.try_recv().is_ok() {}
    let wait_for = |want: &dyn Fn(&paddock_packs::Snapshot<Vec<Category>>) -> bool| loop {
        let s = rx
            .recv_timeout(guard)
            .unwrap_or_else(|e| panic!("the watcher did not report the change: {e:?} ({hint})"));
        if want(&s) {
            return s;
        }
    };

    std::fs::write(dir.join("a.toml"), GOOD_CLEANUP).unwrap();
    wait_for(&|s| s.packs.len() == 1);
    std::fs::write(dir.join("a.toml"), "format = 1\n[[").unwrap();
    wait_for(&|s| s.packs.len() == 1 && s.problems.len() == 1);
    std::fs::remove_file(dir.join("a.toml")).unwrap();
    wait_for(&|s| s.packs.is_empty() && s.problems.is_empty());
    handle.stop();
}
