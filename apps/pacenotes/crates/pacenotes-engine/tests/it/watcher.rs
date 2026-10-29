//! The folder watcher: changes start runs, runs never overlap. No sleeps: the clock and the source are the test's; the quiet
//! time itself is tested once, in paddock-watch.

use crate::support::{ended, rig};
use paddock_jobs::{JobSnapshot, JobState};
use paddock_watch::FakeSource;
use std::sync::Arc;

fn is_end(s: &JobSnapshot) -> bool {
    matches!(s.state, JobState::Done | JobState::Failed { .. } | JobState::Cancelled { .. })
}

/// No job's first event comes before the previous job's last.
fn runs_never_overlap(seen: &[JobSnapshot]) -> bool {
    let mut open = None;
    for s in seen {
        match open {
            None => open = Some(s.id),
            Some(id) if id != s.id => return false,
            Some(_) if is_end(s) => open = None,
            Some(_) => {}
        }
    }
    true
}

#[test]
fn nothing_runs_until_the_quiet_time_has_passed_then_the_change_is_indexed() {
    let r = rig(&[("a.rs", "fn a() {}\n")]);
    let source = FakeSource::new();
    let handle = r
        .service
        .watch(&source, Arc::new(r.clock.clone()), pacenotes_engine::WatchTiming::default())
        .unwrap();
    r.scratch.write("ws/b.rs", "fn b() {}\n").unwrap();
    for _ in 0..5 {
        assert!(source.change());
    }
    assert!(r.sink.all().is_empty(), "the clock has not moved, so the quiet time has not passed");
    let seen = r.sink.wait_driving(&r.clock, |s| ended(s, 1));
    assert!(matches!(seen.last().unwrap().state, JobState::Done));
    handle.stop();
    let opened = pacenotes_index::IndexStore::open(&r.scratch.path("index/index.db")).unwrap();
    assert_eq!(opened.store.known_files().unwrap().len(), 2);
}

#[test]
fn a_change_during_a_run_is_picked_up_by_a_later_run_and_runs_never_overlap() {
    let r = rig(&[("a.rs", "fn a() {}\n")]);
    let source = FakeSource::new();
    let handle = r
        .service
        .watch(&source, Arc::new(r.clock.clone()), pacenotes_engine::WatchTiming::default())
        .unwrap();
    assert!(source.change());
    r.sink.wait_driving(&r.clock, |s| ended(s, 1));
    r.scratch.write("ws/c.rs", "fn c() {}\n").unwrap();
    assert!(source.change());
    let seen = r.sink.wait_driving(&r.clock, |s| ended(s, 2));
    assert!(runs_never_overlap(&seen), "{seen:?}");
    handle.stop();
    let opened = pacenotes_index::IndexStore::open(&r.scratch.path("index/index.db")).unwrap();
    assert_eq!(opened.store.known_files().unwrap().len(), 2);
}
