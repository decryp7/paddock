#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]
//! C2b: who wrote each line, renames followed, bounded and cancellable, and the safe-open rules of ADR 0004 unchanged. Repositories are
//! built with the git binary under the same isolation as in `history.rs`; the product code never runs it.

use std::sync::atomic::{AtomicU64, Ordering};

use pacenotes_core::contract::{BlameFixture, blame_contract};
use pacenotes_core::ports::{Blame, BlameError, BlameLimits, BlameRequest, Blamer};
use pacenotes_core::{CommitHash, Rev};
use pacenotes_git::{GixHistory, open};
use paddock_jobs::CancelToken;

use crate::history::{Repo, live, p};

fn hash(r: &Rev) -> CommitHash {
    match r {
        Rev::Commit(h) => h.clone(),
        Rev::WorkingTree(_) => unreachable!(),
    }
}

fn blame(h: &GixHistory, rev: &Rev, path: &str, req: &BlameRequest) -> Result<Blame, BlameError> {
    h.blame(rev, &p(path), req, &|_| {}, &live())
}

fn who(b: &Blame) -> Vec<(u32, u32, CommitHash)> {
    b.hunks.iter().map(|h| (h.first_line, h.last_line, h.commit.clone())).collect()
}

/// Three commits that each change one line of `f.rs`: line 1 is the third's, line 2 the second's, line 3 the first's.
fn three(repo: &Repo) -> (Rev, Rev, Rev) {
    let a = repo.commit(1_000, "ann", &[("f.rs", "1\n2\n3\n")]);
    let b = repo.commit(2_000, "bob", &[("f.rs", "1\nX\n3\n")]);
    let c = repo.commit(3_000, "cat", &[("f.rs", "Y\nX\n3\n")]);
    (a, b, c)
}

fn expect_three(history: &GixHistory, a: &Rev, b: &Rev, c: &Rev) {
    let got = blame(history, c, "f.rs", &BlameRequest::default()).unwrap();
    assert_eq!(who(&got), [(1, 1, hash(c)), (2, 2, hash(b)), (3, 3, hash(a))], "{got:?}");
}

#[test]
fn blame_gives_each_line_to_the_commit_and_author_that_wrote_it_and_follows_a_rename() {
    let repo = Repo::new();
    let a = repo.commit(1_000, "ann", &[("src/old.rs", "one\ntwo\nthree\n")]);
    repo.git(&["mv", "src/old.rs", "src/new.rs"], 2_000);
    let b = repo.commit(2_000, "bob", &[("src/new.rs", "one\ntwo\nthree\nfour\n")]);
    let c = repo.commit(3_000, "cat", &[("src/new.rs", "one\nTWO\nthree\nfour\n")]);
    let history = open(&repo.dir).unwrap();
    let got = blame(&history, &c, "src/new.rs", &BlameRequest::default()).unwrap();
    let rows: Vec<_> = got
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
        got.hunks[0].source_path.as_ref().map(|p| p.as_str()),
        Some("src/old.rs"),
        "the first line came from the old name"
    );
    assert!(got.hunks[1].source_path.is_none() && !got.truncated && got.unnamed_sources == 0);
    assert_eq!(got.hunks[3].time, 2_000);
    assert_eq!(
        got.lines_by_author(),
        [
            ("ann@example.invalid".to_owned(), 2),
            ("bob@example.invalid".to_owned(), 1),
            ("cat@example.invalid".to_owned(), 1)
        ]
    );
}

#[test]
fn the_real_adapter_passes_the_blame_contract() {
    let repo = Repo::new();
    let (_, _, c) = three(&repo);
    let history = open(&repo.dir).unwrap();
    blame_contract(&BlameFixture {
        blamer: &history,
        rev: c,
        path: p("f.rs"),
        lines: 3,
        missing: p("missing.rs"),
        unknown: Rev::Commit(CommitHash::parse(&"0".repeat(40)).unwrap()),
    });
}

#[test]
fn a_range_since_a_date_and_the_old_revision_all_narrow_the_answer() {
    let repo = Repo::new();
    let a = repo.commit(1_000, "ann", &[("f.rs", "1\n2\n3\n")]);
    let b = repo.commit(2_000, "bob", &[("f.rs", "1\n2\n3\n4\n5\n")]);
    let history = open(&repo.dir).unwrap();
    let ranged = blame(
        &history,
        &b,
        "f.rs",
        &BlameRequest {
            range: Some((4, 5)),
            ..BlameRequest::default()
        },
    )
    .unwrap();
    assert_eq!(who(&ranged), [(4, 5, hash(&b))]);
    assert_eq!(
        who(&blame(&history, &a, "f.rs", &BlameRequest::default()).unwrap()),
        [(1, 3, hash(&a))],
        "blame at an older revision"
    );
    let windowed = blame(
        &history,
        &b,
        "f.rs",
        &BlameRequest {
            since: Some(1_500),
            ..BlameRequest::default()
        },
    )
    .unwrap();
    assert_eq!(
        windowed.hunks.last().map(|h| (h.first_line, h.commit.clone())),
        Some((4, hash(&b))),
        "{windowed:?}"
    );
}

// ---- a repository's attributes and config must not change the attribution (P1) -----------------------------------------------

#[test]
fn a_committed_dash_diff_attribute_does_not_corrupt_the_attribution() {
    let repo = Repo::new();
    let a = repo.commit(1_000, "ann", &[(".gitattributes", "*.rs -diff\n"), ("f.rs", "1\n2\n3\n")]);
    let b = repo.commit(2_000, "bob", &[("f.rs", "1\nX\n3\n")]);
    let c = repo.commit(3_000, "cat", &[("f.rs", "Y\nX\n3\n")]);
    expect_three(&open(&repo.dir).unwrap(), &a, &b, &c);
}

#[test]
fn an_attribute_that_is_only_staged_in_the_index_does_not_corrupt_it() {
    let repo = Repo::new();
    let (a, b, c) = three(&repo);
    std::fs::write(repo.dir.join(".gitattributes"), "*.rs -diff\n*.rs binary\n").unwrap();
    repo.git(&["add", ".gitattributes"], 4_000);
    expect_three(&open(&repo.dir).unwrap(), &a, &b, &c);
}

#[test]
fn the_info_attributes_file_does_not_corrupt_it() {
    let repo = Repo::new();
    let (a, b, c) = three(&repo);
    std::fs::create_dir_all(repo.dir.join(".git/info")).unwrap();
    std::fs::write(repo.dir.join(".git/info/attributes"), "*.rs -diff\n* binary\n").unwrap();
    expect_three(&open(&repo.dir).unwrap(), &a, &b, &c);
}

#[test]
fn a_tiny_big_file_threshold_does_not_corrupt_it() {
    let repo = Repo::new();
    let (a, b, c) = three(&repo);
    repo.git(&["config", "core.bigFileThreshold", "1"], 4_000);
    expect_three(&open(&repo.dir).unwrap(), &a, &b, &c);
}

#[test]
fn a_binary_file_is_a_typed_refusal_not_a_whole_file_given_to_the_oldest_commit() {
    let repo = Repo::new();
    repo.commit(1_000, "ann", &[("data.bin", "\0\0\0abc\n")]);
    let b = repo.commit(2_000, "bob", &[("data.bin", "\0\0\0abc\nmore\n")]);
    let history = open(&repo.dir).unwrap();
    assert_eq!(
        blame(&history, &b, "data.bin", &BlameRequest::default()),
        Err(BlameError::Binary(p("data.bin")))
    );
}

#[test]
fn a_source_name_that_is_not_a_repository_path_is_counted_not_dropped() {
    let repo = Repo::new();
    // a file name with a backslash is legal in git and not a RepoPath
    let a = repo.commit(1_000, "ann", &[("a\\b.rs", "one\ntwo\n")]);
    repo.git(&["mv", "a\\b.rs", "f.rs"], 2_000);
    let b = repo.commit(2_000, "bob", &[("f.rs", "one\ntwo\nthree\n")]);
    let history = open(&repo.dir).unwrap();
    let got = blame(&history, &b, "f.rs", &BlameRequest::default()).unwrap();
    assert_eq!(who(&got), [(1, 2, hash(&a)), (3, 3, hash(&b))]);
    assert_eq!((got.unnamed_sources, got.hunks[0].source_path.is_none()), (1, true), "{got:?}");
}

// ---- bounded, cancellable, with progress (P1) --------------------------------------------------------------------------------

#[test]
fn a_blob_over_the_cap_is_refused_before_it_is_read() {
    let repo = Repo::new();
    // 24 MiB of zeros: a few kilobytes in the object store, 24 MiB if it were inflated
    let big = repo.commit(1_000, "ann", &[("big.txt", &"0".repeat(24 * 1024 * 1024))]);
    let history = open(&repo.dir).unwrap();
    let err = blame(&history, &big, "big.txt", &BlameRequest::default()).unwrap_err();
    assert!(
        matches!(&err, BlameError::Limit { what: "file size in bytes", got, max } if *got == 24 * 1024 * 1024 && *max == 2 * 1024 * 1024),
        "{err:?}"
    );
    // the cap is the caller's: a file of 8 MiB is read when the cap allows it
    let repo = Repo::new();
    let mid = repo.commit(1_000, "ann", &[("mid.txt", &"0".repeat(8 * 1024 * 1024))]);
    let history = open(&repo.dir).unwrap();
    assert!(matches!(
        blame(&history, &mid, "mid.txt", &BlameRequest::default()),
        Err(BlameError::Limit { .. })
    ));
    let allowed = BlameRequest {
        limits: BlameLimits {
            max_blob_bytes: 32 * 1024 * 1024,
            ..BlameLimits::default()
        },
        ..BlameRequest::default()
    };
    let ok = blame(&history, &mid, "mid.txt", &allowed);
    assert!(ok.is_ok(), "{ok:?}");
}

#[test]
fn the_object_budget_stops_a_long_walk_and_the_progress_grows_to_the_figure_in_the_answer() {
    let repo = Repo::new();
    let origin = repo.commit(1_000, "ann", &[("f.rs", "0\n")]);
    let mut last = origin.clone();
    for i in 1..=200 {
        last = repo.commit(1_000 + i, "ann", &[("other.txt", &format!("{i}\n"))]);
    }
    let _ = &last;
    let history = open(&repo.dir).unwrap();
    let tight = BlameRequest {
        limits: BlameLimits {
            max_objects: 10,
            ..BlameLimits::default()
        },
        ..BlameRequest::default()
    };
    let err = blame(&history, &last, "f.rs", &tight).unwrap_err();
    assert!(
        matches!(
            err,
            BlameError::Limit {
                what: "objects read",
                max: 10,
                ..
            }
        ),
        "{err:?}"
    );
    assert!(err.to_string().contains("narrow the range, set `since`"));
    let seen = AtomicU64::new(0);
    let monotonic = std::sync::atomic::AtomicBool::new(true);
    let got = history
        .blame(
            &last,
            &p("f.rs"),
            &BlameRequest::default(),
            &|n| {
                if n < seen.swap(n, Ordering::SeqCst) {
                    monotonic.store(false, Ordering::SeqCst);
                }
            },
            &live(),
        )
        .unwrap();
    assert!(monotonic.load(Ordering::SeqCst), "progress never goes backwards");
    assert_eq!(
        seen.load(Ordering::SeqCst),
        got.objects_read,
        "and ends at the figure the answer reports"
    );
    assert!(got.objects_read > 200, "{}", got.objects_read);
    assert_eq!(
        who(&got),
        [(1, 1, hash(&origin))],
        "the walk went back through 200 commits to the one that wrote the line"
    );
}

#[test]
fn a_cancel_during_the_walk_stops_it() {
    let repo = Repo::new();
    repo.commit(1_000, "ann", &[("f.rs", "0\n")]);
    let mut last = repo.commit(1_001, "ann", &[("other.txt", "0\n")]);
    for i in 2..=200 {
        last = repo.commit(1_000 + i, "ann", &[("other.txt", &format!("{i}\n"))]);
    }
    let history = open(&repo.dir).unwrap();
    let cancel = CancelToken::new();
    let token = cancel.clone();
    let err = history
        .blame(
            &last,
            &p("f.rs"),
            &BlameRequest::default(),
            &|n| {
                if n >= 64 {
                    token.cancel();
                }
            },
            &cancel,
        )
        .unwrap_err();
    assert_eq!(err, BlameError::Cancelled);
}

// ---- typed refusals ------------------------------------------------------------------------------------------------------------

#[test]
fn blame_refuses_what_it_cannot_serve_with_a_typed_error() {
    let repo = Repo::new();
    let a = repo.commit(1_000, "ann", &[("f.rs", "1\n2\n")]);
    let history = open(&repo.dir).unwrap();
    assert_eq!(
        blame(&history, &a, "missing.rs", &BlameRequest::default()),
        Err(BlameError::NotFound(p("missing.rs")))
    );
    assert_eq!(
        blame(
            &history,
            &a,
            "f.rs",
            &BlameRequest {
                range: Some((0, 3)),
                ..BlameRequest::default()
            }
        ),
        Err(BlameError::InvalidRange { first: 0, last: 3 })
    );
    assert_eq!(
        blame(
            &history,
            &a,
            "f.rs",
            &BlameRequest {
                range: Some((3, 1)),
                ..BlameRequest::default()
            }
        ),
        Err(BlameError::InvalidRange { first: 3, last: 1 })
    );
    assert_eq!(
        blame(
            &history,
            &a,
            "f.rs",
            &BlameRequest {
                range: Some((2, 9)),
                ..BlameRequest::default()
            }
        ),
        Err(BlameError::RangeOutsideFile {
            first: 2,
            last: 9,
            lines: 2
        }),
        "a range past the end is an error, never an empty answer"
    );
    assert_eq!(
        blame(
            &history,
            &a,
            "f.rs",
            &BlameRequest {
                range: Some((5, 6)),
                ..BlameRequest::default()
            }
        ),
        Err(BlameError::RangeOutsideFile {
            first: 5,
            last: 6,
            lines: 2
        })
    );
    let unknown = Rev::Commit(CommitHash::parse(&"0".repeat(40)).unwrap());
    assert!(matches!(
        blame(&history, &unknown, "f.rs", &BlameRequest::default()),
        Err(BlameError::UnknownRev(_))
    ));
    assert!(matches!(
        blame(
            &history,
            &Rev::WorkingTree(pacenotes_core::Generation(1)),
            "f.rs",
            &BlameRequest::default()
        ),
        Err(BlameError::UnknownRev(_))
    ));
    let stopped = CancelToken::new();
    stopped.cancel();
    assert_eq!(
        history.blame(&a, &p("f.rs"), &BlameRequest::default(), &|_| {}, &stopped),
        Err(BlameError::Cancelled)
    );
}

#[test]
fn a_blame_with_more_hunks_than_the_limit_is_cut_and_says_so() {
    let repo = Repo::new();
    let (_, _, c) = three(&repo);
    let history = open(&repo.dir).unwrap();
    let cut = BlameRequest {
        limits: BlameLimits {
            max_hunks: 2,
            ..BlameLimits::default()
        },
        ..BlameRequest::default()
    };
    let got = blame(&history, &c, "f.rs", &cut).unwrap();
    assert_eq!((got.hunks.len(), got.truncated), (2, true));
    let ok = BlameRequest {
        limits: BlameLimits {
            max_hunks: 3,
            ..BlameLimits::default()
        },
        ..BlameRequest::default()
    };
    assert!(!blame(&history, &c, "f.rs", &ok).unwrap().truncated);
}

/// A measurement is not a test; but a configured command must never run, with the attributes and drivers a repository names.
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
    for key in [
        "diff.evil.textconv",
        "diff.evil.command",
        "filter.evil.clean",
        "filter.evil.smudge",
        "core.fsmonitor",
    ] {
        repo.git(&["config", key, &evil], 0);
    }
    std::fs::create_dir_all(repo.dir.join(".git/info")).unwrap();
    std::fs::write(repo.dir.join(".git/info/attributes"), "*.rs diff=evil filter=evil\n").unwrap();
    let history = open(&repo.dir).unwrap();
    let got = blame(&history, &b, "f.rs", &BlameRequest::default()).unwrap();
    assert_eq!(got.hunks.iter().map(|h| h.lines()).sum::<u32>(), 3);
    assert!(!marker.exists(), "blame ran a command the repository configured");
}

/// A measurement helper, not a test (rule 48): `cargo test --release -p pacenotes-git --test it -- --ignored --nocapture measure_blame`.
/// Builds invented repositories in the scratch area (never a real one) and times a whole-file blame with gix's similarity rename
/// tracking (`Rewrites::default()`), with a history of edits and with a history that also renames the file every tenth commit.
#[test]
#[ignore = "a measurement, run by hand"]
fn measure_blame() {
    for (name, rename_every) in [("edits only", 0usize), ("a rename every 10th commit", 10)] {
        let repo = Repo::new();
        let lines = |round: usize| {
            (0..2000)
                .map(|i| {
                    if i % 300 == round % 300 {
                        format!("line {i} edited in round {round}\n")
                    } else {
                        format!("line {i}\n")
                    }
                })
                .collect::<String>()
        };
        let mut current = "src/hot0.rs".to_owned();
        let mut last = repo.commit(1_000, "ann", &[(&current, &lines(0))]);
        for round in 1..=300usize {
            if rename_every > 0 && round % rename_every == 0 {
                let next = format!("src/hot{round}.rs");
                repo.git(&["mv", &current, &next], 1_000 + round as i64);
                current = next;
            }
            last = repo.commit(1_000 + round as i64, "ann", &[(&current, &lines(round))]);
        }
        let history = open(&repo.dir).unwrap();
        let started = std::time::Instant::now();
        let got = blame(&history, &last, &current, &BlameRequest::default()).unwrap();
        println!(
            "{name}: 300 commits, 2000 lines: {:?}, {} hunks, {} objects read",
            started.elapsed(),
            got.hunks.len(),
            got.objects_read
        );
    }
}

/// The range is judged from one guarded read of the blamed blob, before the walk: with a budget of one object, which the walk would
/// exceed at once, a range past the end is still `RangeOutsideFile`, and one inside the file is the walk's `Limit`.
#[test]
fn the_range_is_checked_before_the_walk_starts() {
    let repo = Repo::new();
    let (_, _, c) = three(&repo);
    let history = open(&repo.dir).unwrap();
    let one = BlameLimits {
        max_objects: 1,
        ..BlameLimits::default()
    };
    let past = BlameRequest {
        range: Some((1, 9)),
        limits: one,
        ..BlameRequest::default()
    };
    assert_eq!(
        blame(&history, &c, "f.rs", &past),
        Err(BlameError::RangeOutsideFile {
            first: 1,
            last: 9,
            lines: 3
        })
    );
    let inside = BlameRequest {
        range: Some((1, 3)),
        limits: one,
        ..BlameRequest::default()
    };
    assert!(matches!(
        blame(&history, &c, "f.rs", &inside),
        Err(BlameError::Limit { what: "objects read", .. })
    ));
}
