#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]
//! C2b: who wrote each line, renames followed, and the safe-open rules of ADR 0004 unchanged. Repositories are built with the git
//! binary under the same isolation as in `history.rs`; the product code never runs it.

use pacenotes_core::ports::HistoryError;
use pacenotes_core::{CommitHash, Rev};
use pacenotes_git::open;
use paddock_jobs::CancelToken;

use crate::history::{Repo, live, p};

fn hash(r: &Rev) -> CommitHash {
    match r {
        Rev::Commit(h) => h.clone(),
        Rev::WorkingTree(_) => unreachable!(),
    }
}

#[test]
fn blame_gives_each_line_to_the_commit_and_author_that_wrote_it_and_follows_a_rename() {
    let repo = Repo::new();
    let a = repo.commit(1_000, "ann", &[("src/old.rs", "one\ntwo\nthree\n")]);
    repo.git(&["mv", "src/old.rs", "src/new.rs"], 2_000);
    let b = repo.commit(2_000, "bob", &[("src/new.rs", "one\ntwo\nthree\nfour\n")]);
    let c = repo.commit(3_000, "cat", &[("src/new.rs", "one\nTWO\nthree\nfour\n")]);
    let history = open(&repo.dir).unwrap();
    let blame = history.blame(&c, &p("src/new.rs"), None, None, &live()).unwrap();
    let rows: Vec<_> = blame
        .hunks
        .iter()
        .map(|h| (h.first_line, h.last_line, h.commit.clone(), h.author.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            (1, 1, hash(&a), "ann@example.invalid"),
            (2, 2, hash(&c), "cat@example.invalid"),
            (3, 3, hash(&a), "ann@example.invalid"),
            (4, 4, hash(&b), "bob@example.invalid"),
        ]
    );
    assert_eq!(
        blame.hunks[0].source_path.as_ref().map(|p| p.as_str()),
        Some("src/old.rs"),
        "the first line came from the old name"
    );
    assert!(blame.hunks[1].source_path.is_none() && !blame.truncated);
    assert_eq!(blame.hunks[3].time, 2_000);
    assert_eq!(
        blame.lines_by_author(),
        [
            ("ann@example.invalid".to_owned(), 2),
            ("bob@example.invalid".to_owned(), 1),
            ("cat@example.invalid".to_owned(), 1)
        ]
    );
}

#[test]
fn a_range_since_a_date_and_the_old_revision_all_narrow_the_answer() {
    let repo = Repo::new();
    let a = repo.commit(1_000, "ann", &[("f.rs", "1\n2\n3\n")]);
    let b = repo.commit(2_000, "bob", &[("f.rs", "1\n2\n3\n4\n5\n")]);
    let history = open(&repo.dir).unwrap();
    let ranged = history.blame(&b, &p("f.rs"), Some((4, 5)), None, &live()).unwrap();
    assert_eq!(
        (
            ranged.hunks.len(),
            ranged.hunks[0].first_line,
            ranged.hunks[0].last_line,
            ranged.hunks[0].commit.clone()
        ),
        (1, 4, 5, hash(&b))
    );
    let at_a = history.blame(&a, &p("f.rs"), None, None, &live()).unwrap();
    assert_eq!(
        (at_a.hunks.len(), at_a.hunks[0].commit.clone(), at_a.hunks[0].lines()),
        (1, hash(&a), 3),
        "blame at an older revision"
    );
    let windowed = history.blame(&b, &p("f.rs"), None, Some(1_500), &live()).unwrap();
    assert_eq!(
        windowed.hunks.last().map(|h| (h.first_line, h.commit.clone())),
        Some((4, hash(&b))),
        "the newer lines keep their commit: {windowed:?}"
    );
}

#[test]
fn blame_refuses_what_it_cannot_serve_with_a_typed_error() {
    let repo = Repo::new();
    let a = repo.commit(1_000, "ann", &[("f.rs", "1\n")]);
    let history = open(&repo.dir).unwrap();
    assert!(matches!(
        history.blame(&a, &p("missing.rs"), None, None, &live()),
        Err(HistoryError::NotFound(_) | HistoryError::Backend(_))
    ));
    assert!(matches!(history.blame(&a, &p("f.rs"), Some((0, 3)), None, &live()), Err(HistoryError::Backend(m)) if m.contains("1-based")));
    assert!(matches!(
        history.blame(&a, &p("f.rs"), Some((3, 1)), None, &live()),
        Err(HistoryError::Backend(_))
    ));
    let unknown = Rev::Commit(CommitHash::parse(&"0".repeat(40)).unwrap());
    assert!(matches!(
        history.blame(&unknown, &p("f.rs"), None, None, &live()),
        Err(HistoryError::UnknownRev(_))
    ));
    assert!(matches!(
        history.blame(&Rev::WorkingTree(pacenotes_core::Generation(1)), &p("f.rs"), None, None, &live()),
        Err(HistoryError::UnknownRev(_))
    ));
    let stopped = CancelToken::new();
    stopped.cancel();
    assert!(matches!(
        history.blame(&a, &p("f.rs"), None, None, &stopped),
        Err(HistoryError::Cancelled)
    ));
}

/// The trap row for blame: a repository whose own config and attributes name a text-conversion command and a filter must not make
/// blame run anything.
#[test]
fn blame_runs_nothing_a_repository_configures() {
    let repo = Repo::new();
    let marker = repo.dir.parent().unwrap().join("marker-ran");
    let evil = format!("touch {}", marker.display());
    repo.commit(
        1_000,
        "ann",
        &[(".gitattributes", "*.rs diff=evil filter=evil\n"), ("f.rs", "1\n2\n")],
    );
    let b = repo.commit(2_000, "bob", &[("f.rs", "1\n2\n3\n")]);
    // configured after the commits: the git binary that builds this repository would itself run a clean filter
    repo.git(&["config", "diff.evil.textconv", &evil], 0);
    repo.git(&["config", "diff.evil.command", &evil], 0);
    repo.git(&["config", "filter.evil.clean", &evil], 0);
    repo.git(&["config", "filter.evil.smudge", &evil], 0);
    repo.git(&["config", "core.fsmonitor", &evil], 0);
    let history = open(&repo.dir).unwrap();
    let blame = history.blame(&b, &p("f.rs"), None, None, &live()).unwrap();
    assert_eq!(blame.hunks.iter().map(|h| h.lines()).sum::<u32>(), 3);
    assert!(!marker.exists(), "blame ran a command the repository configured");
}

#[test]
fn a_blame_with_more_hunks_than_the_limit_is_cut_and_says_so() {
    let repo = Repo::new();
    repo.commit(1_000, "ann", &[("f.rs", "1\n2\n3\n")]);
    repo.commit(2_000, "bob", &[("f.rs", "1\nX\n3\n")]);
    let c = repo.commit(3_000, "cat", &[("f.rs", "Y\nX\n3\n")]);
    let history = open(&repo.dir).unwrap();
    let cut = history.blame_limited(&c, &p("f.rs"), None, None, 2, &live()).unwrap();
    assert_eq!((cut.hunks.len(), cut.truncated), (2, true));
    assert!(!history.blame_limited(&c, &p("f.rs"), None, None, 3, &live()).unwrap().truncated);
}
