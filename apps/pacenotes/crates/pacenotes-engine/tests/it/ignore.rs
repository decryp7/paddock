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
