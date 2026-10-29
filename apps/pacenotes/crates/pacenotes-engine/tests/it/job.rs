//! The index run as a job.

use crate::support::{ended, phase_ids, rig};
use paddock_jobs::JobState;

fn known(r: &crate::support::Rig) -> Vec<String> {
    let opened = pacenotes_index::IndexStore::open(&r.scratch.path("index/index.db")).unwrap();
    let mut v: Vec<String> = opened
        .store
        .known_files()
        .unwrap()
        .into_iter()
        .map(|k| k.path.as_str().to_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn a_run_reports_its_phases_indexes_the_files_and_honours_gitignore_with_the_denylist_winning() {
    let r = rig(&[
        (".gitignore", "target/\n.env\n"),
        ("src/lib.rs", "pub fn a() {}\n"),
        ("src/main.rs", "fn main() {}\n"),
        ("target/x.rs", "fn t() {}\n"),
        ("id_rsa", "secret"),
        (".env", "KEY=1"),
    ]);
    let outcome = r.service.start().unwrap().join().unwrap();
    assert_eq!(outcome.report.added, 3, "{:?}", outcome.report);
    // id_rsa and .env are both withheld: the denylist is checked before the ignore rules, so a secret that .gitignore also lists is
    // still counted as withheld and never merely "ignored"; target/ is the one ignored entry.
    assert_eq!((outcome.report.denied, outcome.report.ignored), (2, 1), "{:?}", outcome.report);
    assert!(outcome.rebuilt.is_none());
    assert_eq!(known(&r), [".gitignore", "src/lib.rs", "src/main.rs"]);

    let seen = r.sink.all();
    assert!(matches!(seen.first().unwrap().state, JobState::Queued));
    assert!(matches!(seen.last().unwrap().state, JobState::Done));
    assert_eq!(seen.first().unwrap().kind.0, "pacenotes.index");
    assert_eq!(phase_ids(&seen), ["walk", "read", "extract", "save"]);
}

#[test]
fn a_second_run_over_an_unchanged_tree_changes_nothing_and_a_change_is_picked_up() {
    let r = rig(&[("a.rs", "fn a() {}\n"), ("b.rs", "fn b() {}\n")]);
    r.service.start().unwrap().join().unwrap();
    let again = r.service.start().unwrap().join().unwrap();
    assert_eq!((again.report.added, again.report.modified, again.report.unchanged), (0, 0, 2));
    assert!(again.report.generation.is_none());
    r.scratch.write("ws/c.rs", "fn c() {}\n").unwrap();
    let third = r.service.start().unwrap().join().unwrap();
    assert_eq!(third.report.added, 1);
    assert_eq!(known(&r), ["a.rs", "b.rs", "c.rs"]);
}

#[test]
fn a_workspace_that_is_missing_fails_with_a_message_instead_of_panicking() {
    let r = rig(&[("a.rs", "x")]);
    std::fs::remove_dir_all(r.scratch.path("ws")).unwrap();
    let err = r.service.start().unwrap().join().unwrap_err();
    assert!(matches!(err, paddock_jobs::JobError::Failed(_)), "{err:?}");
    let seen = r.sink.wait_for(|s| ended(s, 1));
    assert!(matches!(seen.last().unwrap().state, JobState::Failed { .. }));
}

#[test]
fn a_job_cancelled_while_queued_never_starts() {
    let r = rig(&[("a.rs", "x")]);
    let held = r.pool.hold();
    let handle = r.service.start().unwrap();
    let id = r.sink.all().first().unwrap().id;
    assert!(r.registry.cancel(id));
    drop(held);
    assert!(matches!(handle.join(), Err(paddock_jobs::JobError::Cancelled(_))));
    assert!(!r.scratch.path("index/index.db").exists(), "nothing was written");
}
