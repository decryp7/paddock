#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]
//! C2: the `History` port over gix, the log, hot spots, and the trap rows for History (audit PN1, plan A3a). Repositories are
//! built here with the git binary under the same isolation the trap fixture uses; the product code never runs it.

use std::path::{Path, PathBuf};

use pacenotes_core::contract::{HistoryFixture, history_contract};
use pacenotes_core::ports::{ChangeKind, History, HistoryError};
use pacenotes_core::{CommitHash, RepoPath, Rev};
use pacenotes_git::{OpenError, open, rank_hot_spots};
use paddock_jobs::CancelToken;
use paddock_testkit::Scratch;

pub fn p(s: &str) -> RepoPath {
    RepoPath::parse(s).unwrap()
}

pub struct Repo {
    _scratch: Scratch,
    pub dir: PathBuf,
    home: PathBuf,
}

impl Repo {
    pub fn new() -> Repo {
        let scratch = Scratch::new();
        let dir = scratch.path("repo");
        std::fs::create_dir_all(&dir).unwrap();
        let home = scratch.home();
        let repo = Repo {
            _scratch: scratch,
            dir,
            home,
        };
        repo.git(&["init", "-q", "--template=", "-b", "main"], 0);
        repo
    }

    pub fn git(&self, args: &[&str], time: i64) -> String {
        let date = format!("@{time} +0000");
        let out = std::process::Command::new("git")
            .current_dir(&self.dir)
            .args(["-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null"])
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("HOME", &self.home)
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .output()
            .expect("git is installed");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    }

    /// Writes `files` (an empty content is an empty file), stages everything and commits at `time` by `author`.
    pub fn commit(&self, time: i64, author: &str, files: &[(&str, &str)]) -> Rev {
        for (name, content) in files {
            let path = self.dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        self.git(&["add", "-A"], time);
        let who = format!("user.name={author}");
        let mail = format!("user.email={author}@example.invalid");
        self.git(&["-c", &who, "-c", &mail, "commit", "-q", "--allow-empty", "-m", "c"], time);
        Rev::Commit(CommitHash::parse(self.git(&["rev-parse", "HEAD"], time).trim()).unwrap())
    }
}

pub fn live() -> CancelToken {
    CancelToken::new()
}

#[test]
fn the_history_contract_holds_on_a_real_repository() {
    let repo = Repo::new();
    let a = repo.commit(
        1_000,
        "ann",
        &[
            ("keep.rs", "fn keep() {}\n"),
            ("changed.rs", "v1"),
            ("empty.txt", ""),
            ("gone.rs", "old"),
        ],
    );
    std::fs::remove_file(repo.dir.join("gone.rs")).unwrap();
    let b = repo.commit(2_000, "bob", &[("changed.rs", "v2"), ("added.rs", "new")]);
    let history = open(&repo.dir).unwrap();
    history_contract(&HistoryFixture {
        history: &history,
        rev_a: a,
        rev_b: b,
        present: (p("changed.rs"), b"v2".to_vec()),
        empty: p("empty.txt"),
        missing: p("nope.rs"),
    });
}

#[test]
fn a_changed_file_is_modified_added_or_deleted_and_an_exact_rename_is_found() {
    let repo = Repo::new();
    let a = repo.commit(
        1_000,
        "ann",
        &[("src/old_name.rs", "same content\n"), ("src/edit.rs", "1"), ("src/gone.rs", "bye")],
    );
    repo.git(&["mv", "src/old_name.rs", "src/new_name.rs"], 2_000);
    std::fs::remove_file(repo.dir.join("src/gone.rs")).unwrap();
    let b = repo.commit(2_000, "ann", &[("src/edit.rs", "2"), ("src/fresh.rs", "fresh")]);
    let history = open(&repo.dir).unwrap();
    let set = history.changed_between(&a, &b, 100, &live()).unwrap();
    let kinds: Vec<_> = set.changes.iter().map(|c| (c.path.as_str().to_owned(), c.kind.clone())).collect();
    assert_eq!(
        kinds,
        vec![
            ("src/edit.rs".to_owned(), ChangeKind::Modified),
            ("src/fresh.rs".to_owned(), ChangeKind::Added),
            ("src/gone.rs".to_owned(), ChangeKind::Deleted),
            (
                "src/new_name.rs".to_owned(),
                ChangeKind::Renamed {
                    from: p("src/old_name.rs")
                }
            ),
        ]
    );
    assert!(!set.truncated);
    assert!(history.changed_between(&a, &a, 100, &live()).unwrap().changes.is_empty());
}

#[test]
fn unknown_and_working_tree_revisions_are_unknown_revs() {
    let repo = Repo::new();
    let a = repo.commit(1_000, "ann", &[("a.rs", "x")]);
    let history = open(&repo.dir).unwrap();
    let ghost = Rev::Commit(CommitHash::parse(&"ab".repeat(20)).unwrap());
    for rev in [&ghost, &Rev::WorkingTree(pacenotes_core::Generation(1))] {
        assert!(
            matches!(history.read_at(rev, &p("a.rs"), 10, &live()), Err(HistoryError::UnknownRev(_))),
            "{rev}"
        );
        assert!(matches!(
            history.changed_between(&a, rev, 10, &live()),
            Err(HistoryError::UnknownRev(_))
        ));
        assert!(matches!(history.log(rev, 10, &live()), Err(HistoryError::UnknownRev(_))));
    }
}

#[test]
fn a_folder_is_not_a_file_and_a_repository_without_commits_has_no_head() {
    let repo = Repo::new();
    let history = open(&repo.dir).unwrap();
    assert!(matches!(history.head(&live()), Err(HistoryError::UnknownRev(_))));
    let a = repo.commit(1_000, "ann", &[("dir/a.rs", "x")]);
    let history = open(&repo.dir).unwrap();
    assert_eq!(history.read_at(&a, &p("dir"), 10, &live()), Err(HistoryError::NotFound(p("dir"))));
    assert_eq!(history.head(&live()).unwrap(), a);
    assert_eq!(history.root(), repo.dir.canonicalize().unwrap());
}

#[test]
fn the_changed_file_list_is_cut_at_the_limit_and_says_so() {
    let repo = Repo::new();
    let a = repo.commit(1_000, "ann", &[("x", "1")]);
    let b = repo.commit(2_000, "ann", &[("b.rs", "1"), ("a.rs", "1"), ("c.rs", "1")]);
    let history = open(&repo.dir).unwrap();
    let set = history.changed_between(&a, &b, 2, &live()).unwrap();
    assert_eq!((set.changes.len(), set.truncated), (2, true));
    assert_eq!(set.changes[0].path, p("a.rs"));
}

#[test]
fn the_log_lists_each_commit_newest_first_and_stops_at_the_limit_or_a_cancel() {
    let repo = Repo::new();
    repo.commit(1_000, "ann", &[("a.rs", "1"), ("b.rs", "1")]);
    repo.commit(2_000, "bob", &[("a.rs", "2")]);
    let head = repo.commit(3_000, "ann", &[("c.rs", "1"), ("a.rs", "3")]);
    let history = open(&repo.dir).unwrap();
    let log = history.log(&head, 100, &live()).unwrap();
    assert_eq!(log.iter().map(|c| c.time).collect::<Vec<_>>(), [3_000, 2_000, 1_000]);
    assert_eq!(log[0].files.iter().map(RepoPath::as_str).collect::<Vec<_>>(), ["a.rs", "c.rs"]);
    assert_eq!(log[2].files.len(), 2, "a root commit lists all of its files");
    assert_eq!(log[1].author, "bob@example.invalid");
    assert_eq!(history.log(&head, 2, &live()).unwrap().len(), 2);
    let dead = CancelToken::new();
    dead.cancel();
    assert_eq!(history.log(&head, 100, &dead), Err(HistoryError::Cancelled));
    assert_eq!(history.head(&dead), Err(HistoryError::Cancelled));
}

/// The plan's failing test for C2: hot-spot ranking over a repository built in a scratch folder.
#[test]
fn hot_spots_rank_recently_and_often_changed_files_first() {
    const DAY: i64 = 86_400;
    let now = 1_000 * DAY;
    let repo = Repo::new();
    // old.rs: changed 4 times, long ago. busy.rs: 3 times this month. calm.rs: once, recently.
    for i in 0..4 {
        repo.commit(now - (400 + i) * DAY, "ann", &[("old.rs", &format!("{i}")), ("calm.rs", "x")]);
    }
    repo.commit(now - 20 * DAY, "bob", &[("busy.rs", "1")]);
    repo.commit(now - 10 * DAY, "ann", &[("busy.rs", "2")]);
    let head = repo.commit(now - DAY, "bob", &[("busy.rs", "3"), ("calm.rs", "y")]);
    let history = open(&repo.dir).unwrap();
    let log = history.log(&head, 1000, &live()).unwrap();
    let ranked = rank_hot_spots(&log, now, 30 * DAY as u64, 3);
    assert_eq!(
        ranked.iter().map(|h| h.path.as_str()).collect::<Vec<_>>(),
        ["busy.rs", "calm.rs", "old.rs"]
    );
    assert_eq!((ranked[0].commits, ranked[0].authors), (3, 2));
    assert!(ranked[0].score > ranked[1].score && ranked[1].score > ranked[2].score);
}

// ---- opening: nothing may lead out of the workspace --------------------------------------------------------------------

#[test]
fn a_folder_without_git_is_not_a_repository() {
    let scratch = Scratch::new();
    assert!(matches!(open(&scratch.path("missing")), Err(OpenError::NotARepository(_))));
    std::fs::create_dir_all(scratch.path("plain")).unwrap();
    assert!(matches!(open(&scratch.path("plain")), Err(OpenError::NotARepository(_))));
    std::fs::write(scratch.path("plain/.git"), "not a gitfile\n").unwrap();
    assert!(matches!(open(&scratch.path("plain")), Err(OpenError::NotARepository(_))));
    std::fs::write(scratch.path("plain/.git"), "gitdir: .git-real\n").unwrap();
    std::fs::create_dir_all(scratch.path("plain/.git-real")).unwrap();
    assert!(
        matches!(open(&scratch.path("plain")), Err(OpenError::Backend(_))),
        "a gitfile inside the workspace is followed, and an empty folder is no repository"
    );
}

fn outside(err: Result<pacenotes_git::GixHistory, OpenError>, what: &str) {
    match err {
        Err(OpenError::OutsideWorkspace { what: w, .. }) => assert_eq!(w, what),
        other => panic!("expected OutsideWorkspace({what}), got {:?}", other.map(|_| "opened")),
    }
}

#[test]
fn a_gitfile_commondir_objects_link_or_alternate_that_leads_out_is_refused() {
    // gitfile to a repository outside
    let repo = Repo::new();
    repo.commit(1_000, "ann", &[("a.rs", "1")]);
    let scratch = Scratch::new();
    let ws = scratch.path("ws");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join(".git"), format!("gitdir: {}\n", repo.dir.join(".git").display())).unwrap();
    outside(open(&ws), "gitdir");

    // commondir pointing out
    let inner = Repo::new();
    inner.commit(1_000, "ann", &[("a.rs", "1")]);
    std::fs::write(inner.dir.join(".git/commondir"), format!("{}\n", repo.dir.join(".git").display())).unwrap();
    outside(open(&inner.dir), "commondir");

    // objects folder is a symlink out
    #[cfg(unix)]
    {
        let linked = Repo::new();
        linked.commit(1_000, "ann", &[("a.rs", "1")]);
        let objects = linked.dir.join(".git/objects");
        std::fs::rename(&objects, linked.dir.join(".git/objects-moved")).unwrap();
        std::os::unix::fs::symlink(repo.dir.join(".git/objects"), &objects).unwrap();
        outside(open(&linked.dir), "objects");
    }

    // alternates: absolute outside, relative outside, and a chain that starts inside and ends outside
    for line in [
        format!("{}\n", repo.dir.join(".git/objects").display()),
        format!(
            "{}{}\n",
            "../".repeat(repo.dir.join(".git/objects").components().count() - 1),
            repo.dir.join(".git/objects").to_string_lossy().trim_start_matches('/')
        ),
    ] {
        let alt = Repo::new();
        alt.commit(1_000, "ann", &[("a.rs", "1")]);
        std::fs::create_dir_all(alt.dir.join(".git/objects/info")).unwrap();
        std::fs::write(alt.dir.join(".git/objects/info/alternates"), &line).unwrap();
        outside(open(&alt.dir), "alternates");
    }
    let chain = Repo::new();
    chain.commit(1_000, "ann", &[("a.rs", "1")]);
    let hop = chain.dir.join("hop/objects");
    std::fs::create_dir_all(hop.join("info")).unwrap();
    std::fs::write(
        hop.join("info/alternates"),
        format!("{}\n", repo.dir.join(".git/objects").display()),
    )
    .unwrap();
    std::fs::write(chain.dir.join(".git/objects/info/alternates"), "# comment\n\n../../hop/objects\n").unwrap();
    outside(open(&chain.dir), "alternates");
}

#[test]
fn many_alternates_are_refused() {
    let repo = Repo::new();
    repo.commit(1_000, "ann", &[("a.rs", "1")]);
    // .git/objects -> hop1 -> hop2 -> ... -> hop10, all inside the workspace
    let mut previous = repo.dir.join(".git/objects");
    for i in 1..=10 {
        let hop = repo.dir.join(format!(".git/hop{i}/objects"));
        std::fs::create_dir_all(hop.join("info")).unwrap();
        std::fs::create_dir_all(previous.join("info")).unwrap();
        std::fs::write(previous.join("info/alternates"), format!("{}\n", hop.display())).unwrap();
        previous = hop;
    }
    assert!(matches!(open(&repo.dir), Err(OpenError::AlternatesTooDeep(_))));
}

#[test]
fn a_damaged_object_database_is_a_backend_error_not_a_panic() {
    let repo = Repo::new();
    let a = repo.commit(1_000, "ann", &[("a.rs", "1")]);
    let blob = repo.git(&["rev-parse", "HEAD:a.rs"], 0).trim().to_owned();
    let history = open(&repo.dir).unwrap();
    std::fs::remove_file(repo.dir.join(format!(".git/objects/{}/{}", &blob[..2], &blob[2..]))).unwrap();
    assert!(matches!(
        history.read_at(&a, &p("a.rs"), 10, &live()),
        Err(HistoryError::Backend(_))
    ));
    let Rev::Commit(hash) = &a else { unreachable!() };
    let commit_file = repo
        .dir
        .join(format!(".git/objects/{}/{}", &hash.as_str()[..2], &hash.as_str()[2..]));
    std::fs::remove_file(commit_file).unwrap();
    assert!(matches!(history.log(&a, 10, &live()), Err(HistoryError::UnknownRev(_))));
}

#[test]
fn a_commondir_inside_the_workspace_is_allowed() {
    let repo = Repo::new();
    let a = repo.commit(1_000, "ann", &[("a.rs", "1")]);
    std::fs::write(repo.dir.join(".git/commondir"), ".\n").unwrap();
    let history = open(&repo.dir).unwrap();
    assert_eq!(history.head(&live()).unwrap(), a);
}

#[test]
fn an_alternate_inside_the_git_folder_is_allowed() {
    let repo = Repo::new();
    let a = repo.commit(1_000, "ann", &[("a.rs", "1")]);
    let inside = repo.dir.join(".git/spare/objects");
    std::fs::create_dir_all(inside.join("info")).unwrap();
    std::fs::write(repo.dir.join(".git/objects/info/alternates"), "../spare/objects\n").unwrap();
    let history = open(&repo.dir).unwrap();
    assert_eq!(history.read_at(&a, &p("a.rs"), 10, &live()).unwrap(), b"1");
}

// ---- the trap rows for History -----------------------------------------------------------------------------------------

#[test]
fn trap_row_a_trapped_repository_that_leads_out_is_refused_and_one_that_does_not_is_read_without_running_anything() {
    let scratch = Scratch::new();
    let trap = pacenotes_trap::generate(&scratch).unwrap();

    // (a) the nested checkout's gitfile points outside, and the workspace borrows objects from outside
    outside(open(&trap.nested), "gitdir");
    outside(open(&trap.workspace), "alternates");

    // (b) with the alternates line removed the repository is read: HEAD, a file, the changes and the log all work, while
    // the planted fsmonitor, filter and diff drivers (and .gitattributes binding them to every file) are in place
    std::fs::remove_file(trap.workspace.join(".git/objects/info/alternates")).unwrap();
    let history = open(&trap.workspace).unwrap();
    let head = history.head(&live()).unwrap();
    let text = history.read_at(&head, &p("src/lib.rs"), 1 << 20, &live()).unwrap();
    assert!(String::from_utf8_lossy(&text).contains("trap_macros::boom!()"));
    let log = history.log(&head, 10, &live()).unwrap();
    assert_eq!(log.len(), 2);
    // blame goes through the same trapped repository: every file is bound to the planted filter and diff driver, and it runs neither
    let blame = pacenotes_core::ports::Blamer::blame(&history, &head, &p("src/lib.rs"), &Default::default(), &|_| {}, &live()).unwrap();
    assert_eq!(
        blame.hunks.iter().map(|h| h.lines()).sum::<u32>() as usize,
        String::from_utf8_lossy(&text).lines().count()
    );
    let first = log[1].commit.clone();
    let changes = history.changed_between(&Rev::Commit(first), &head, 10, &live()).unwrap();
    assert_eq!(
        changes.changes.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(),
        ["src/extra.rs"]
    );

    // nothing ran: neither the marker, nor a file a driver could have written
    assert!(!trap.marker.exists(), "no filter, diff driver, fsmonitor, wrapper or macro ran");
    // the port reads the object database as it is; the .env blob is in the history and keeping it out of the index is the
    // denylist's job (A3a), so reading it here is not a breach
    assert!(history.read_at(&head, &p(".env"), 1 << 20, &live()).is_ok());
}

#[test]
fn the_sources_never_start_a_process_and_reach_blame_only_through_one_wrapper() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in std::fs::read_dir(src).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        for forbidden in ["Command::new", "std::process", "tokio::process", "gix::command", "gix_command"] {
            assert!(!text.contains(forbidden), "{} mentions {forbidden}", path.display());
        }
        // the ways to a blob diff, a filter or an attribute lookup exist in one file only, and the repository-level shortcuts that read the
        // index or the attributes of the repository are in none
        let is_blame = path.file_name().is_some_and(|n| n == "blame.rs");
        for forbidden in [
            "diff_resource_cache(",
            "Mode::ToWorktree",
            "ToWorktreeAndBinaryToText",
            "filter_pipeline",
            "blame_file(",
            "attributes_only(",
            "index_or_load_from_head",
        ] {
            assert!(!text.contains(forbidden), "{} mentions {forbidden}", path.display());
        }
        for wrapper_only in ["resource_cache(", "blame::file(", "worktree::Stack"] {
            assert!(
                is_blame || !text.contains(wrapper_only),
                "{} mentions {wrapper_only}, which only blame.rs may",
                path.display()
            );
        }
    }
}

// ---- the two ways out that the first review found, and the rest of the git folder ---------------------------------------

fn alternates_file(repo: &Repo) -> PathBuf {
    let dir = repo.dir.join(".git/objects/info");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("alternates")
}

#[test]
fn a_quoted_alternates_line_is_judged_by_the_parser_that_reads_it() {
    let secret = Repo::new();
    secret.commit(1_000, "ann", &[("secret.txt", "outside content")]);
    let out = secret.dir.join(".git/objects");
    for (what, line) in [
        ("quoted", format!("\"{}\"\n", out.display())),
        // octal escapes hide the slash and the letters from a naive check
        (
            "quoted with escapes",
            format!("\"\\057{}\"\n", out.to_string_lossy().trim_start_matches('/')),
        ),
        ("quoted with a tab escape", format!("\"{}\\t\"\n", out.display())),
    ] {
        let repo = Repo::new();
        repo.commit(1_000, "ann", &[("a.rs", "1")]);
        std::fs::write(alternates_file(&repo), &line).unwrap();
        let result = open(&repo.dir);
        assert!(
            matches!(
                result,
                Err(OpenError::OutsideWorkspace { what: "alternates", .. }) | Err(OpenError::Backend(_))
            ),
            "{what}: {:?}",
            result.map(|_| "opened")
        );
    }
}

#[test]
fn a_quoted_alternates_line_that_stays_in_the_git_folder_is_allowed() {
    let repo = Repo::new();
    let a = repo.commit(1_000, "ann", &[("a.rs", "1")]);
    std::fs::create_dir_all(repo.dir.join(".git/spare/objects")).unwrap();
    std::fs::write(
        alternates_file(&repo),
        format!("\"{}\"\n", repo.dir.join(".git/spare/objects").display()),
    )
    .unwrap();
    assert_eq!(open(&repo.dir).unwrap().read_at(&a, &p("a.rs"), 10, &live()).unwrap(), b"1");
}

#[cfg(unix)]
fn symlinked(repo: &Repo, target: &Path, link: &str) -> Result<pacenotes_git::GixHistory, OpenError> {
    let at = repo.dir.join(".git").join(link);
    if at.is_dir() {
        std::fs::remove_dir_all(&at).unwrap();
    } else {
        let _ = std::fs::remove_file(&at);
    }
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(target, &at).unwrap();
    open(&repo.dir)
}

#[cfg(unix)]
#[test]
fn a_symlink_anywhere_in_the_git_folder_is_refused() {
    let other = Repo::new();
    other.commit(1_000, "ann", &[("secret.txt", "outside content")]);
    other.git(&["gc", "-q"], 0);
    for (what, target, link) in [
        ("objects/pack", other.dir.join(".git/objects/pack"), "objects/pack"),
        ("packed-refs", other.dir.join(".git/packed-refs"), "packed-refs"),
        ("refs/heads/x", other.dir.join(".git/refs/heads/main"), "refs/heads/x"),
        ("HEAD", other.dir.join(".git/HEAD"), "HEAD"),
        ("objects/ab", other.dir.join(".git/objects/info"), "objects/ab"),
    ] {
        let repo = Repo::new();
        repo.commit(1_000, "ann", &[("a.rs", "1")]);
        match symlinked(&repo, &target, link) {
            Err(OpenError::SymlinkInGitDir(path)) => assert!(path.ends_with(link), "{what}: {path:?}"),
            other => panic!("{what}: {:?}", other.map(|_| "opened")),
        }
    }
    // .git itself a link to a repository elsewhere
    let ws = Scratch::new();
    std::os::unix::fs::symlink(other.dir.join(".git"), ws.path("ws-dot-git")).ok();
    std::fs::create_dir_all(ws.path("ws")).unwrap();
    std::os::unix::fs::symlink(other.dir.join(".git"), ws.path("ws/.git")).unwrap();
    assert!(matches!(open(&ws.path("ws")), Err(OpenError::SymlinkInGitDir(_))));
}

#[cfg(unix)]
#[test]
fn a_symlink_inside_the_common_dir_is_refused_too() {
    let repo = Repo::new();
    repo.commit(1_000, "ann", &[("a.rs", "1")]);
    let elsewhere = repo.dir.join("common");
    std::fs::create_dir_all(&elsewhere).unwrap();
    for entry in std::fs::read_dir(repo.dir.join(".git")).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() != "HEAD" {
            std::fs::rename(entry.path(), elsewhere.join(entry.file_name())).unwrap();
        }
    }
    std::fs::write(repo.dir.join(".git/commondir"), "../common\n").unwrap();
    std::os::unix::fs::symlink("/etc", elsewhere.join("refs/heads/escape")).unwrap();
    assert!(matches!(open(&repo.dir), Err(OpenError::SymlinkInGitDir(_))));
    std::fs::remove_file(elsewhere.join("refs/heads/escape")).unwrap();
    assert!(
        !matches!(open(&repo.dir), Err(OpenError::SymlinkInGitDir(_))),
        "without the link the common dir passes the link check"
    );
}

#[test]
fn file_names_that_are_not_repo_paths_are_counted_not_dropped() {
    let repo = Repo::new();
    let a = repo.commit(1_000, "ann", &[("ok.rs", "1"), ("weird\\kept.txt", "1"), ("weird\\gone.txt", "1")]);
    std::fs::remove_file(repo.dir.join("weird\\gone.txt")).unwrap();
    let b = repo.commit(2_000, "ann", &[("weird\\name.txt", "x"), ("weird\\kept.txt", "2"), ("ok.rs", "2")]);
    let history = open(&repo.dir).unwrap();
    let set = history.changed_between(&a, &b, 10, &live()).unwrap();
    assert_eq!(set.changes.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), ["ok.rs"]);
    assert_eq!(set.skipped_names, 3, "one added, one modified and one deleted");
    let log = history.log(&b, 10, &live()).unwrap();
    assert_eq!((log[0].files.len(), log[0].skipped_names, log[0].truncated), (1, 3, false));
}

#[test]
fn a_huge_commit_is_cut_at_the_limit_by_the_walk_itself() {
    let repo = Repo::new();
    let a = repo.commit(1_000, "ann", &[("x", "1")]);
    let many: Vec<(String, String)> = (0..300).map(|i| (format!("f{i:03}.txt"), i.to_string())).collect();
    let files: Vec<(&str, &str)> = many.iter().map(|(n, c)| (n.as_str(), c.as_str())).collect();
    let b = repo.commit(2_000, "ann", &files);
    let history = open(&repo.dir).unwrap();
    let set = history.changed_between(&a, &b, 5, &live()).unwrap();
    assert!(set.truncated && set.changes.len() == 5, "{} changes", set.changes.len());
    assert_eq!(set.changes[0].path.as_str(), "f000.txt");
}

#[test]
fn the_trap_with_extra_ways_out_is_refused() {
    let scratch = Scratch::new();
    let trap = pacenotes_trap::generate(&scratch).unwrap();
    let outside_objects = trap.outside_gitdir.parent().unwrap().join("objects");
    // a quoted alternates line to the outside folder
    std::fs::write(
        trap.workspace.join(".git/objects/info/alternates"),
        format!("\"{}\"\n", outside_objects.display()),
    )
    .unwrap();
    let r = open(&trap.workspace);
    assert!(
        matches!(r, Err(OpenError::OutsideWorkspace { what: "alternates", .. })),
        "{:?}",
        r.map(|_| "opened")
    );
    // an alternate inside the workspace but outside .git, whose pack folder is a link out
    #[cfg(unix)]
    {
        let vendor = trap.workspace.join("vendor/objects");
        std::fs::create_dir_all(vendor.join("info")).unwrap();
        std::os::unix::fs::symlink(outside_objects.join("pack"), vendor.join("pack")).unwrap();
        std::fs::write(
            trap.workspace.join(".git/objects/info/alternates"),
            format!("{}\n", vendor.display()),
        )
        .unwrap();
        let r = open(&trap.workspace);
        assert!(matches!(r, Err(OpenError::AlternateOutsideGitDir(_))), "{:?}", r.map(|_| "opened"));
    }
    std::fs::remove_file(trap.workspace.join(".git/objects/info/alternates")).unwrap();
    #[cfg(unix)]
    {
        let pack = trap.workspace.join(".git/objects/pack");
        std::fs::remove_dir_all(&pack).unwrap();
        std::os::unix::fs::symlink(outside_objects.join("pack"), &pack).unwrap();
        assert!(matches!(open(&trap.workspace), Err(OpenError::SymlinkInGitDir(_))));
    }
    assert!(!trap.marker.exists());
}

#[test]
fn a_missing_alternate_inside_the_workspace_is_ignored_and_one_that_cannot_be_resolved_is_refused() {
    let repo = Repo::new();
    repo.commit(1_000, "ann", &[("a.rs", "1")]);
    std::fs::write(
        alternates_file(&repo),
        format!("{}\n", repo.dir.join(".git/no-such/objects").display()),
    )
    .unwrap();
    assert!(open(&repo.dir).is_ok(), "git ignores an alternate that is not there");
    std::fs::write(alternates_file(&repo), format!("{}x\n", "../".repeat(60))).unwrap();
    assert!(
        matches!(open(&repo.dir), Err(OpenError::Backend(_))),
        "a path that climbs above the root cannot be judged, so it is refused"
    );
}

#[cfg(unix)]
#[test]
fn an_alternate_in_the_workspace_but_outside_the_git_folder_is_refused() {
    // the second review's probe: vendor/objects is inside the workspace, but its pack folder is a link to an outside pack
    let outside = Repo::new();
    outside.commit(1_000, "ann", &[("secret.txt", "outside content")]);
    outside.git(&["gc", "-q"], 0);
    let repo = Repo::new();
    let a = repo.commit(1_000, "ann", &[("a.rs", "1")]);
    let vendor = repo.dir.join("vendor/objects");
    std::fs::create_dir_all(vendor.join("info")).unwrap();
    std::os::unix::fs::symlink(outside.dir.join(".git/objects/pack"), vendor.join("pack")).unwrap();
    std::fs::write(alternates_file(&repo), format!("{}\n", vendor.display())).unwrap();
    let r = open(&repo.dir);
    match r {
        Err(OpenError::AlternateOutsideGitDir(path)) => assert!(path.ends_with("vendor/objects"), "{path:?}"),
        other => panic!(
            "the link-bearing alternate opened or failed otherwise: {:?}",
            other.map(|_| "opened")
        ),
    }
    // and the repository itself is still readable without that alternate
    std::fs::remove_file(alternates_file(&repo)).unwrap();
    assert_eq!(open(&repo.dir).unwrap().read_at(&a, &p("a.rs"), 10, &live()).unwrap(), b"1");
}

/// A measurement helper, not a test: `PN_WALK_REPO=<a repository> cargo test -p pacenotes-git --test it -- --ignored
/// --nocapture measure_open` prints how long `open` (the git-folder walk plus gix) takes (rule 48/51; results are in
/// docs/performance.md).
#[test]
#[ignore = "a measurement, run by hand with PN_WALK_REPO"]
fn measure_open() {
    let repo = std::env::var("PN_WALK_REPO").expect("PN_WALK_REPO names a repository");
    let started = std::time::Instant::now();
    let result = open(Path::new(&repo)).map(|_| ());
    println!("open({repo}) -> {result:?} in {:?}", started.elapsed());
}

#[cfg(unix)]
#[test]
fn folders_gix_never_reads_are_not_walked_but_a_link_in_their_place_is_refused() {
    let repo = Repo::new();
    repo.commit(1_000, "ann", &[("a.rs", "1")]);
    std::fs::create_dir_all(repo.dir.join(".git/hooks/inner")).unwrap();
    std::os::unix::fs::symlink("/etc", repo.dir.join(".git/hooks/inner/link")).unwrap();
    assert!(
        open(&repo.dir).is_ok(),
        "hooks are never opened, so a link inside them is not walked"
    );
    std::fs::remove_dir_all(repo.dir.join(".git/hooks")).unwrap();
    std::os::unix::fs::symlink("/etc", repo.dir.join(".git/hooks")).unwrap();
    assert!(
        matches!(open(&repo.dir), Err(OpenError::SymlinkInGitDir(_))),
        "a link named hooks is itself refused"
    );
}
