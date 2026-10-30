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

/// The reviewer's probe: an index file inside the repository is itself a file of the repository, so every run changed it and the next
/// run saw a change, forever. It is refused before anything runs.
#[test]
fn a_store_inside_the_workspace_is_refused_by_start_and_watch_with_a_message_that_says_what_to_change() {
    for inside in ["ws/.pacenotes/index.db", "ws/index.db", "ws/a/b/c/index.db"] {
        let mut r = rig(&[("a.rs", "fn a() {}\n")]);
        let setup = pacenotes_engine::IndexSetup {
            store_path: r.scratch.path(inside),
            root: r.scratch.path("ws"),
            source: std::sync::Arc::new(crate::support::NoLanguages),
            denylist: std::sync::Arc::new(pacenotes_index::Denylist::new()),
            limits: pacenotes_index::IndexLimits::default(),
            read_workers: paddock_jobs::Workers::one(),
            parse_workers: paddock_jobs::Workers::one(),
        };
        r.service = pacenotes_engine::IndexService::new(r.registry.clone(), r.pool.clone(), std::sync::Arc::new(setup));
        let err = r.service.start().err().unwrap_or_else(|| panic!("{inside} was accepted"));
        assert!(matches!(err, pacenotes_engine::EngineError::StoreInsideWorkspace { .. }), "{err:?}");
        assert!(err.to_string().contains("outside the repository"), "{err}");
        let source = paddock_watch::FakeSource::new();
        let timing = pacenotes_engine::WatchTiming::default();
        assert!(r.service.watch(&source, std::sync::Arc::new(r.clock.clone()), timing).is_err());
        assert!(r.sink.all().is_empty(), "nothing was registered");
        assert!(
            !r.scratch.path("ws/.pacenotes").exists() && !r.scratch.path("ws/index.db").exists(),
            "nothing was written"
        );
    }
}

#[test]
fn a_store_beside_the_workspace_is_fine_and_a_run_with_no_changes_writes_nothing() {
    let r = rig(&[("a.rs", "fn a() {}\n"), ("b.rs", "fn b() {}\n")]);
    // files edited within the last two seconds are read again on purpose (the racy rule) and that is a write; these are old
    for name in ["a.rs", "b.rs"] {
        let file = std::fs::File::options()
            .write(true)
            .open(r.scratch.path(&format!("ws/{name}")))
            .unwrap();
        file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
            .unwrap();
    }
    r.service.start().unwrap().join().unwrap();
    let store = r.scratch.path("index/index.db");
    let before = std::fs::read(&store).unwrap();
    let folder_before: Vec<_> = std::fs::read_dir(store.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    for _ in 0..3 {
        let again = r.service.start().unwrap().join().unwrap();
        assert!(again.report.generation.is_none(), "{:?}", again.report);
    }
    assert_eq!(std::fs::read(&store).unwrap(), before, "the index file is byte for byte the same");
    let folder_after: Vec<_> = std::fs::read_dir(store.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(folder_before.len(), folder_after.len());
}
