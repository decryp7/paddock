//! The reader against a real repository made by git.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_git::{GitError, Kind, Reader};
use paddock_testkit::TestCommand;
use std::path::{Path, PathBuf};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = TestCommand::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// A repository with two commits: `a.txt`, `d/b.txt`, `d/e/c.bin` (not UTF-8), then `a.txt` changed and `d/b.txt` removed.
fn repo() -> (tempfile::TempDir, String, String) {
    let d = paddock_testkit::tempdir();
    let r = d.path();
    git(r, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(r.join("d/e")).unwrap();
    std::fs::write(r.join("a.txt"), "one\n").unwrap();
    std::fs::write(r.join("d/b.txt"), "bee\n").unwrap();
    std::fs::write(r.join("d/e/c.bin"), [0u8, 159, 146, 150, 10]).unwrap();
    git(r, &["add", "--all"]);
    git(r, &["commit", "-q", "-m", "one"]);
    let first = git(r, &["rev-parse", "HEAD"]).trim().to_string();
    std::fs::write(r.join("a.txt"), "two\n").unwrap();
    std::fs::remove_file(r.join("d/b.txt")).unwrap();
    git(r, &["add", "--all"]);
    git(r, &["commit", "-q", "-m", "two"]);
    let second = git(r, &["rev-parse", "HEAD"]).trim().to_string();
    (d, first, second)
}

fn reader(root: &Path) -> Reader {
    let root: PathBuf = root.to_path_buf();
    Reader::new(move || {
        let (mut cmd, guard) = TestCommand::new("git").into_parts();
        // The private folder may go: cat-file reads only the repository.
        drop(guard);
        cmd.arg("-C").arg(&root);
        cmd
    })
}

#[test]
fn files_at_reads_many_files_through_one_git_and_a_missing_path_or_revision_is_none() {
    let (d, first, second) = repo();
    let mut r = reader(d.path());
    let got = r.files_at(&[
        (&first, "a.txt"),
        (&second, "a.txt"),
        (&first, "d/b.txt"),
        (&second, "d/b.txt"),
        (&first, "d/e/c.bin"),
        ("0000000000000000000000000000000000000000", "a.txt"),
    ]);
    let got: Vec<Option<Vec<u8>>> = got.into_iter().map(Result::unwrap).collect();
    assert_eq!(got[0].as_deref(), Some(&b"one\n"[..]));
    assert_eq!(got[1].as_deref(), Some(&b"two\n"[..]));
    assert_eq!(got[2].as_deref(), Some(&b"bee\n"[..]));
    assert_eq!(got[3], None, "removed in the second commit");
    assert_eq!(
        got[4].as_deref(),
        Some(&[0u8, 159, 146, 150, 10][..]),
        "bytes, not text"
    );
    assert_eq!(got[5], None, "a revision that does not exist");
    assert_eq!(r.starts(), 1, "one git process for every read");
}

/// The spawn budget of the reader itself: 200 reads, one start (the class: one git start per item, ADR 0108).
#[test]
fn two_hundred_reads_start_git_once() {
    let (d, first, _) = repo();
    let mut r = reader(d.path());
    for _ in 0..100 {
        assert!(r.file_at(&first, "a.txt").unwrap().is_some());
        assert!(
            r.info(&format!("{first}:d"))
                .unwrap()
                .is_some_and(|i| i.kind == Kind::Tree)
        );
    }
    assert_eq!(r.starts(), 1);
}

#[test]
fn walk_lists_what_ls_tree_r_lists() {
    let (d, first, second) = repo();
    let mut r = reader(d.path());
    for rev in [&first, &second] {
        let walked: Vec<String> = r
            .walk(&format!("{rev}^{{tree}}"))
            .unwrap()
            .unwrap()
            .into_iter()
            .map(|e| format!("{} blob {}\t{}", e.mode, e.oid, e.path))
            .collect();
        let listed: Vec<String> = git(d.path(), &["ls-tree", "-r", rev])
            .lines()
            .map(str::to_string)
            .collect();
        assert_eq!(walked, listed, "{rev}");
    }
    assert_eq!(
        r.walk("0000000000000000000000000000000000000000").unwrap(),
        None
    );
}

#[test]
fn a_folder_asked_as_a_file_is_an_error_naming_it() {
    let (d, first, _) = repo();
    let mut r = reader(d.path());
    let e = r.file_at(&first, "d").unwrap_err();
    assert!(
        matches!(
            e,
            GitError::WrongKind {
                kind: Kind::Tree,
                ..
            }
        ),
        "{e}"
    );
    assert!(e.to_string().contains(":d"), "{e}");
    assert!(
        r.file_at(&first, "a.txt").unwrap().is_some(),
        "the process still answers"
    );
    assert_eq!(r.starts(), 1);
}

#[test]
fn a_request_with_a_newline_is_refused_before_git_sees_it() {
    let (d, first, _) = repo();
    let mut r = reader(d.path());
    let e = r.file_at(&first, "a.txt\nHEAD").unwrap_err();
    assert!(matches!(e, GitError::BadRequest(_)), "{e}");
    assert_eq!(r.starts(), 0);
}

/// A folder that is no repository: git ends at once, the read is an error with git's words, and the next read starts again.
#[test]
fn a_git_that_ends_is_an_error_with_its_words_and_the_next_read_starts_a_new_one() {
    let d = paddock_testkit::tempdir();
    let mut r = reader(d.path());
    let e = r.file_at("HEAD", "a.txt").unwrap_err();
    assert!(matches!(e, GitError::Process(_)), "{e}");
    assert!(
        e.to_string()
            .to_lowercase()
            .contains("not a git repository"),
        "{e}"
    );
    assert!(r.file_at("HEAD", "a.txt").is_err());
    assert_eq!(r.starts(), 2);
}

fn gix_reader(root: &Path) -> Reader {
    let make_root: PathBuf = root.to_path_buf();
    Reader::open(root, move || {
        let (mut cmd, guard) = TestCommand::new("git").into_parts();
        drop(guard);
        cmd.arg("-C").arg(&make_root);
        cmd
    })
}

/// Reads named by full object ids are answered in process: no git start at all, and the same answers as git's.
#[test]
fn reads_by_object_id_start_no_git_and_answer_as_git_does() {
    let (d, first, second) = repo();
    let mut g = gix_reader(d.path());
    assert!(g.in_process());
    let mut c = reader(d.path());
    let specs = [
        format!("{first}:a.txt"),
        format!("{second}:a.txt"),
        format!("{first}:d/e/c.bin"),
        format!("{second}:d/b.txt"),
        format!("{first}^{{tree}}"),
        format!("{first}:d"),
        first.clone(),
        format!("{first}:no/such/file"),
    ];
    for spec in &specs {
        assert_eq!(g.object(spec).unwrap(), c.object(spec).unwrap(), "{spec}");
        assert_eq!(g.info(spec).unwrap(), c.info(spec).unwrap(), "{spec}");
    }
    for rev in [&first, &second] {
        assert_eq!(
            g.walk(&format!("{rev}^{{tree}}")).unwrap(),
            c.walk(&format!("{rev}^{{tree}}")).unwrap()
        );
    }
    assert_eq!(g.starts(), 0, "every read above was answered in process");
    // A ref name is git's to resolve: one start, then the same process for every later one.
    assert_eq!(
        g.file_at("main", "a.txt").unwrap().as_deref(),
        Some(&b"two\n"[..])
    );
    assert_eq!(
        g.file_at("main~1", "a.txt").unwrap().as_deref(),
        Some(&b"one\n"[..])
    );
    assert_eq!(g.starts(), 1);
}

/// A partial clone omits blobs: gix does not have them, so the reader asks git, which fetches them from the promisor remote.
#[test]
fn a_blob_a_partial_clone_omitted_is_read_through_git() {
    let (d, first, _) = repo();
    git(d.path(), &["config", "uploadpack.allowFilter", "true"]);
    let clone = paddock_testkit::tempdir();
    let origin = d.path().display().to_string();
    // conformance: path-text — a file:// URL for git clone (URLs use `/`), not a path compared with another
    let url = format!("file://{}", origin.replace('\\', "/"));
    let target = clone.path().join("c");
    let out = TestCommand::new("git")
        .args(["clone", "-q", "--no-checkout", "--filter=blob:none", &url])
        .arg(&target)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut g = gix_reader(&target);
    assert!(g.in_process());
    assert_eq!(
        g.file_at(&first, "a.txt").unwrap().as_deref(),
        Some(&b"one\n"[..])
    );
    assert_eq!(g.starts(), 1, "the omitted blob came through git");
    // Trees are in the partial clone: the listing stays in process.
    let names: Vec<String> = g
        .walk(&format!("{first}^{{tree}}"))
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|e| e.path)
        .collect();
    assert_eq!(names, ["a.txt", "d/b.txt", "d/e/c.bin"]);
    assert_eq!(g.starts(), 1);
}

#[test]
fn a_folder_gix_cannot_open_is_read_through_git_alone() {
    let d = paddock_testkit::tempdir();
    let g = gix_reader(d.path());
    assert!(!g.in_process());
}

/// An object over the reader's size cap is an error naming its size, on both paths, and gix never loads it to find out: the cap is
/// checked against the object's header first. The commit and trees on the way are under the cap.
#[test]
fn an_object_over_the_cap_is_an_error_in_process_and_through_git() {
    let (d, _, _) = repo();
    std::fs::write(d.path().join("big.txt"), "x".repeat(10_000)).unwrap();
    git(d.path(), &["add", "big.txt"]);
    git(d.path(), &["commit", "-q", "-m", "big"]);
    let head = git(d.path(), &["rev-parse", "HEAD"]).trim().to_string();
    let oid = git(d.path(), &["rev-parse", &format!("{head}:big.txt")])
        .trim()
        .to_string();
    let mut g = gix_reader(d.path()).with_max_object(5_000);
    assert!(g.in_process());
    for spec in [oid.clone(), format!("{head}:big.txt")] {
        match g.object(&spec) {
            Err(GitError::TooLarge { size, max, .. }) => {
                assert_eq!((size, max), (10_000, 5_000), "{spec}")
            }
            other => panic!("{spec}: {other:?}"),
        }
    }
    assert_eq!(
        g.info(&oid).unwrap().map(|i| i.size),
        Some(10_000),
        "info reads the header only, whatever the size"
    );
    assert_eq!(
        g.file_at(&head, "a.txt").unwrap().as_deref(),
        Some(b"two\n".as_slice()),
        "a file under the cap still reads"
    );
    assert_eq!(g.starts(), 0, "all of it in process");
    let mut r = reader(d.path()).with_max_object(5_000);
    match r.file_at(&head, "big.txt") {
        Err(GitError::TooLarge { size, max, .. }) => assert_eq!((size, max), (10_000, 5_000)),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        r.file_at(&head, "a.txt").unwrap().as_deref(),
        Some(b"two\n".as_slice()),
        "the next read starts a new git"
    );
    assert_eq!(r.starts(), 2);
}
