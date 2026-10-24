//! The one real-folder test: a pack file added, broken and removed on disk is applied by the real watcher. Waits on
//! events with a hang guard, never on a fixed time.
#![cfg(feature = "watch")]

use crate::common;
use common::*;
use paddock_jobs::time::SystemClock;
use paddock_packs::watch::{self, NotifySource};
use paddock_packs::{DirSource, Limits, PackLoader, PackStore, Reloader};
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
    let wait_for = |want: &dyn Fn(&paddock_packs::Snapshot<Vec<Category>>) -> bool| loop {
        let s = rx
            .recv_timeout(guard)
            .expect("the watcher did not report the change");
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
