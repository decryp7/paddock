//! A watch that ends is started again, with a bounded back-off, and a watched folder that is gone is looked for until it is back.
//! Every step is logged (ER-27) and every wait is on the debouncer's fake clock; the tests wait on conditions, never on time.

use crate::rescan::{lines_with, logged};
use paddock_jobs::time::FakeClock;
use paddock_testkit::wait::HANG_GUARD;
use paddock_watch::{
    Debouncer, FakeSource, RESTART_FIRST, RESTART_MAX, ROOT_POLL, restart_delay, spawn,
};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn until(what: &str, f: impl FnMut() -> bool) {
    assert!(
        paddock_testkit::poll_until_within(HANG_GUARD, f),
        "timed out waiting for {what}"
    );
}

fn watch(
    dir: &std::path::Path,
) -> (
    FakeClock,
    FakeSource,
    mpsc::Receiver<()>,
    paddock_watch::WatchHandle,
) {
    logged();
    let clock = FakeClock::new();
    let d = Arc::new(Debouncer::new(Arc::new(clock.clone()), ms(300), ms(1500)));
    let source = FakeSource::new();
    let (tx, rx) = mpsc::channel();
    let h = spawn(d, &source, dir, false, move || {
        let _ = tx.send(());
    })
    .unwrap();
    (clock, source, rx, h)
}

#[test]
fn the_wait_doubles_from_the_first_and_stops_at_the_longest() {
    assert_eq!(restart_delay(0), RESTART_FIRST);
    assert_eq!(restart_delay(1), RESTART_FIRST * 2);
    assert_eq!(restart_delay(3), RESTART_FIRST * 8);
    assert_eq!(restart_delay(6), RESTART_MAX);
    assert_eq!(restart_delay(u32::MAX), RESTART_MAX);
}

#[test]
fn a_watch_that_ends_is_started_again_and_the_folder_is_looked_at() {
    let t = tempfile::tempdir().unwrap();
    let (clock, source, rx, h) = watch(t.path());
    let folder = t.path().display().to_string();
    assert_eq!(source.watches(), 1);
    source.end();
    until("the end to be logged", || {
        lines_with(&folder)
            .iter()
            .any(|(_, m)| m.contains("ended (the source stopped reporting)"))
    });
    clock.advance(RESTART_FIRST);
    until("the second start", || source.watches() == 2);
    until("the restart to be logged", || {
        lines_with(&folder)
            .iter()
            .any(|(_, m)| m.contains("started again (after 0 failed start(s))"))
    });
    clock.advance(ms(1500));
    rx.recv_timeout(HANG_GUARD)
        .expect("the folder was not looked at after the restart");
    assert!(source.change(), "the new watch reports");
    assert!(!h.is_finished());
    h.stop();
}

#[test]
fn a_failed_watch_is_started_again_and_a_failing_start_backs_off() {
    let t = tempfile::tempdir().unwrap();
    let (clock, source, _rx, h) = watch(t.path());
    let folder = t.path().display().to_string();
    source.refuse(true);
    assert!(source.fail("the watch broke (test 51c2)"));
    until("the failure to be logged", || {
        lines_with(&folder)
            .iter()
            .any(|(_, m)| m.contains("ended (the watch broke (test 51c2))"))
    });
    clock.advance(RESTART_FIRST);
    until("a refused start", || source.watches() == 2);
    until("the refusal to be logged with the next wait", || {
        lines_with(&folder).iter().any(|(_, m)| {
            m.contains("could not start the watch")
                && m.contains(&format!("{:?}", restart_delay(1)))
        })
    });
    source.refuse(false);
    clock.advance(restart_delay(1));
    until("the third start", || source.watches() == 3);
    until("the restart to be logged", || {
        lines_with(&folder)
            .iter()
            .any(|(_, m)| m.contains("started again (after 1 failed start(s))"))
    });
    h.stop();
}

#[test]
fn a_folder_that_is_gone_is_reported_and_watched_again_when_it_is_back() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("w");
    let (clock, source, rx, h) = watch(&dir);
    let folder = dir.display().to_string();
    std::fs::remove_dir(&dir).unwrap();
    assert!(source.change());
    until("the gone folder to be logged", || {
        lines_with(&folder)
            .iter()
            .any(|(_, m)| m.contains("is gone"))
    });
    clock.advance(ms(1500));
    rx.recv_timeout(HANG_GUARD)
        .expect("the action did not run for the gone folder");
    assert_eq!(
        source.watches(),
        1,
        "nothing is started while the folder is gone"
    );
    std::fs::create_dir(&dir).unwrap();
    clock.advance(ROOT_POLL);
    until("the folder to be watched again", || source.watches() == 2);
    until("the return to be logged", || {
        lines_with(&folder)
            .iter()
            .any(|(_, m)| m.contains("is back"))
    });
    h.stop();
}
