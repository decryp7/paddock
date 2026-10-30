//! `.gitignore` through the shared walker.

use crate::support::rig;
use pacenotes_core::RepoPath;
use pacenotes_engine::RepoFilesIgnore;
use pacenotes_index::Ignore;
use paddock_repo_files::WalkLimits;

fn p(s: &str) -> RepoPath {
    RepoPath::parse(s).unwrap()
}

#[test]
fn the_visible_sets_come_from_the_paths_and_their_folders() {
    let ignore = RepoFilesIgnore::from_paths(["a.rs".to_owned(), "src/deep/b.rs".to_owned()]);
    assert!(ignore.visible_file(&p("a.rs")) && ignore.visible_file(&p("src/deep/b.rs")));
    assert!(!ignore.visible_file(&p("c.rs")) && !ignore.visible_file(&p("src")));
    assert!(ignore.visible_dir(&p("src")) && ignore.visible_dir(&p("src/deep")));
    assert!(!ignore.visible_dir(&p("target")) && !ignore.visible_dir(&p("src/other")));
}

#[test]
fn gitignore_files_apply_with_negations_and_nesting_and_without_a_repository() {
    let r = rig(&[
        (".gitignore", "target/\n*.log\n!keep.log\n"),
        ("src/lib.rs", "x"),
        ("src/.gitignore", "gen/\n"),
        ("src/gen/out.rs", "x"),
        ("target/debug/a.rs", "x"),
        ("run.log", "x"),
        ("keep.log", "x"),
    ]);
    let ignore = RepoFilesIgnore::scan(&r.scratch.path("ws"), WalkLimits::default()).unwrap();
    for visible in ["src/lib.rs", "keep.log", ".gitignore", "src/.gitignore"] {
        assert!(ignore.visible_file(&p(visible)), "{visible}");
    }
    for hidden in ["src/gen/out.rs", "target/debug/a.rs", "run.log"] {
        assert!(!ignore.visible_file(&p(hidden)), "{hidden}");
    }
    assert!(!ignore.visible_dir(&p("target")) && !ignore.visible_dir(&p("src/gen")) && ignore.visible_dir(&p("src")));
}

#[test]
fn a_workspace_that_is_not_a_folder_is_an_error_that_says_so() {
    let r = rig(&[("a.rs", "x")]);
    let err = RepoFilesIgnore::scan(&r.scratch.path("ws/a.rs"), WalkLimits::default())
        .err()
        .unwrap();
    assert!(err.to_string().starts_with("could not read the ignore rules"), "{err}");
}

/// A measurement helper, not a test (rule 48): `PN_MEASURE_REPO=<a folder> cargo test --release -p pacenotes-engine --test it --
/// --ignored --nocapture measure_ignore_walk`. Times the shared walker (the extra cost of honouring `.gitignore`) against the
/// index's own walk.
#[test]
#[ignore = "a measurement, run by hand with PN_MEASURE_REPO"]
fn measure_ignore_walk() {
    let repo = std::path::PathBuf::from(std::env::var("PN_MEASURE_REPO").expect("PN_MEASURE_REPO names a folder"));
    let fs = pacenotes_index::DiskFs::new(&repo).unwrap();
    let deny = pacenotes_index::Denylist::new();
    for round in 1..=3 {
        let t = std::time::Instant::now();
        let snap = pacenotes_index::snapshot(
            &fs,
            &deny,
            &pacenotes_index::SnapshotLimits::default(),
            &paddock_jobs::CancelToken::new(),
        )
        .unwrap();
        let own = t.elapsed();
        let t = std::time::Instant::now();
        let ignore = RepoFilesIgnore::scan(&repo, WalkLimits::default()).unwrap();
        let shared = t.elapsed();
        let t = std::time::Instant::now();
        let with = pacenotes_index::snapshot_with(
            &fs,
            &deny,
            &ignore,
            &pacenotes_index::SnapshotLimits::default(),
            &paddock_jobs::CancelToken::new(),
            &pacenotes_index::NoProgress,
        )
        .unwrap();
        println!(
            "round {round}: index walk without ignore rules {own:?} ({} files); shared walk {shared:?}; index walk with them {:?} ({} files, {} ignored)",
            snap.files.len(),
            t.elapsed(),
            with.files.len(),
            with.ignored
        );
    }
}

/// Counts what the walk reports.
#[derive(Default)]
struct Count(std::sync::Mutex<(u64, Option<u64>)>);

impl pacenotes_index::Progress for Count {
    fn phase(&self, _: &'static str, _: &'static str, _: Option<u64>) {}
    fn add(&self, n: u64) {
        self.0.lock().unwrap().0 += n;
    }
    fn set_total(&self, total: u64) {
        self.0.lock().unwrap().1 = Some(total);
    }
}

#[test]
fn finding_files_shows_a_growing_count_and_ends_with_the_total() {
    let r = rig(&[
        (".gitignore", "target/\n"),
        ("a.rs", "x"),
        ("src/b.rs", "x"),
        ("src/c.rs", "x"),
        ("target/d.rs", "x"),
    ]);
    let progress = Count::default();
    let ignore = RepoFilesIgnore::scan_cancellable(
        &r.scratch.path("ws"),
        WalkLimits::default(),
        paddock_jobs::Workers::new(2),
        &paddock_jobs::CancelToken::new(),
        &progress,
    )
    .unwrap();
    assert_eq!(
        *progress.0.lock().unwrap(),
        (5, Some(5)),
        "every file of the tree was counted as it was found"
    );
    assert!(ignore.visible_file(&p("src/b.rs")) && !ignore.visible_file(&p("target/d.rs")));
}

/// ER-19: Cancel works in "Finding files". The shared walk is blocked here and never returns until the test lets it; the call
/// still returns, as cancelled, the moment the token fires.
#[test]
fn cancel_ends_finding_files_while_the_shared_walk_is_still_running() {
    use std::sync::mpsc;
    let cancel = paddock_jobs::CancelToken::new();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let canceller = {
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            entered_rx.recv().unwrap();
            cancel.cancel();
        })
    };
    let result = RepoFilesIgnore::scan_with(
        move || {
            entered_tx.send(()).unwrap();
            let _ = release_rx.recv();
            Ok(vec!["late.rs".to_owned()])
        },
        || Ok(()),
        &cancel,
    );
    assert!(matches!(result, Err(paddock_jobs::JobError::Cancelled(_))), "{:?}", result.err());
    canceller.join().unwrap();
    drop(release_tx);
}

#[test]
fn cancel_ends_the_count_too_and_a_failed_walk_is_a_failure_with_its_message() {
    let cancel = paddock_jobs::CancelToken::new();
    cancel.cancel();
    let r = rig(&[("a.rs", "x")]);
    let err = RepoFilesIgnore::scan_cancellable(
        &r.scratch.path("ws"),
        WalkLimits::default(),
        paddock_jobs::Workers::one(),
        &cancel,
        &pacenotes_index::NoProgress,
    );
    assert!(matches!(err, Err(paddock_jobs::JobError::Cancelled(_))), "{:?}", err.err());
    let err = RepoFilesIgnore::scan_cancellable(
        &r.scratch.path("ws/a.rs"),
        WalkLimits::default(),
        paddock_jobs::Workers::one(),
        &paddock_jobs::CancelToken::new(),
        &pacenotes_index::NoProgress,
    );
    assert!(matches!(err, Err(paddock_jobs::JobError::Failed(_))), "{:?}", err.err());
}
