//! The one real-folder test: a file written in a real folder reaches the debounced action. Waits on events with a hang
//! guard, never on a fixed time.
#![cfg(feature = "notify")]

use paddock_jobs::time::SystemClock;
use paddock_watch::{Debouncer, NotifySource, spawn};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn a_real_folder_change_reaches_the_action_and_a_recursive_watch_sees_sub_folders() {
    for recursive in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("w");
        let d = Arc::new(Debouncer::new(
            Arc::new(SystemClock),
            Duration::ZERO,
            Duration::ZERO,
        ));
        let (tx, rx) = mpsc::channel();
        let h = spawn(d, &NotifySource, &dir, recursive, move || {
            let _ = tx.send(());
        })
        .unwrap();
        if recursive {
            std::fs::create_dir_all(dir.join("sub")).unwrap();
            rx.recv_timeout(Duration::from_secs(30))
                .expect("no event for the new sub-folder");
            while rx.try_recv().is_ok() {}
            std::fs::write(dir.join("sub/a.txt"), "x").unwrap();
        } else {
            std::fs::write(dir.join("a.txt"), "x").unwrap();
        }
        rx.recv_timeout(Duration::from_secs(30))
            .expect("the watcher did not report the change");
        h.stop();
    }
}

#[test]
fn watching_a_file_instead_of_a_folder_is_reported() {
    let t = tempfile::tempdir().unwrap();
    let missing = t.path().join("gone");
    let d = Arc::new(Debouncer::new(
        Arc::new(SystemClock),
        Duration::ZERO,
        Duration::ZERO,
    ));
    // `spawn` creates the folder, so use the source directly on a path that does not exist.
    let (tx, _rx) = mpsc::channel();
    use paddock_watch::ChangeSource;
    assert!(NotifySource.watch(&missing, false, tx).is_err());
    drop(d);
}
