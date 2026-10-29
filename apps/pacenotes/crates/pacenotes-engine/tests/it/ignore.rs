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
