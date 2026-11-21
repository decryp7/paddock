//! The one real-folder test: a file written in a real folder reaches the debounced action. Waits on events with a hang
//! guard, never on a fixed time.
//!
//! FSEvents (macOS) is not a lossless, prompt channel: with fseventsd saturated (100% CPU, 12.7 GB, 6.5 days up) a probe
//! on the d7 Mac saw the first event after a new watch arrive in a median 912 ms (a change on a live watch: 119 ms) and
//! lost 5 of 50 events for 30 s; after fseventsd restarted, 0 of 100 were lost and each took about 10 ms (runs 8389 and
//! 8466 timed out here). So the test proves the watch live first ([`until_live`] rewrites a probe file until the action
//! has run once), and the action reports which of the test's paths existed when it ran, so a late probe event cannot
//! stand in for the change the test checks. A change that is still never reported fails, naming the host's likely state.
#![cfg(feature = "notify")]

use paddock_jobs::time::SystemClock;
use paddock_testkit::wait::{recv_within, retry_within};
use paddock_watch::{Debouncer, NotifySource, spawn};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

/// How often the probe file is rewritten until the watch reports it: every write is a new change, so a write the stream
/// missed while fseventsd was registering it is followed by one made after (a healthy fseventsd reports one in about 10 ms).
const PROBE_EVERY: Duration = Duration::from_millis(250);

/// The hang guard of each wait, as before the probe: a wait that sees nothing for this long fails with its own message.
const GUARD: Duration = Duration::from_secs(30);

/// What a timeout most likely means on macOS, for the failure message.
const HOST_HINT: &str = "on macOS, a saturated fseventsd delays or drops events: check `ps -o pcpu,rss -p $(pgrep -x fseventsd)`";

/// Rewrites `dir/.probe` until the watch reports a change, then drops everything reported so far.
fn until_live<T>(dir: &Path, rx: &Receiver<T>) {
    let probe = dir.join(".probe");
    let mut writes = 0u32;
    let live = retry_within(GUARD, PROBE_EVERY, || {
        if rx.try_recv().is_ok() {
            return true;
        }
        writes += 1;
        std::fs::write(&probe, writes.to_string()).unwrap();
        false
    });
    assert!(
        live,
        "the watch reported none of {writes} probe writes within {GUARD:?} ({HOST_HINT})"
    );
    while rx.try_recv().is_ok() {}
}

/// Which of the test's paths existed when the action ran.
fn existing(paths: &[PathBuf]) -> Vec<bool> {
    paths.iter().map(|p| p.exists()).collect()
}

/// Waits until the action runs while `paths[i]` exists.
fn until_seen(rx: &Receiver<Vec<bool>>, i: usize, what: &str) {
    let what = format!("{what} ({HOST_HINT})");
    while !recv_within(&what, GUARD, rx)[i] {}
}

#[test]
fn a_real_folder_change_reaches_the_action_and_a_recursive_watch_sees_sub_folders() {
    for recursive in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("w");
        let (file, sub, sub_file) = (dir.join("a.txt"), dir.join("sub"), dir.join("sub/a.txt"));
        let paths = vec![file.clone(), sub.clone(), sub_file.clone()];
        let d = Arc::new(Debouncer::new(
            Arc::new(SystemClock),
            Duration::ZERO,
            Duration::ZERO,
        ));
        let (tx, rx) = mpsc::channel();
        let h = spawn(d, &NotifySource, &dir, recursive, move || {
            let _ = tx.send(existing(&paths));
        })
        .unwrap();
        until_live(&dir, &rx);
        if recursive {
            std::fs::create_dir_all(&sub).unwrap();
            until_seen(&rx, 1, "an event for the new sub-folder");
            while rx.try_recv().is_ok() {}
            std::fs::write(&sub_file, "x").unwrap();
            until_seen(&rx, 2, "the watcher to report the change in the sub-folder");
        } else {
            std::fs::write(&file, "x").unwrap();
            until_seen(&rx, 0, "the watcher to report the change");
        }
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
