use paddock_jobs::*;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

fn tree(root: &Path) {
    for d in ["a", "a/b", "a/b/c", "z", ".git", "node_modules/x"] {
        fs::create_dir_all(root.join(d)).unwrap();
    }
    for f in [
        "top.txt",
        "a/1.log",
        "a/b/2.log",
        "a/b/c/3.log",
        "z/4.txt",
        ".git/HEAD",
        "node_modules/x/5.js",
    ] {
        fs::write(root.join(f), f.as_bytes()).unwrap();
    }
}

fn names(r: &WalkResult, root: &Path) -> Vec<String> {
    r.entries
        .iter()
        .map(|e| {
            paddock_env::winpath::lexical_text(
                e.path.strip_prefix(root).unwrap(),
                paddock_env::Os::current(),
            )
        })
        .collect()
}

#[test]
fn walks_everything_sorted_and_counts() {
    let t = tempfile::tempdir().unwrap();
    tree(t.path());
    let found = AtomicU64::new(0);
    let r = par_walk(
        t.path(),
        &WalkOptions::default(),
        &CancelToken::new(),
        &found,
    )
    .unwrap();
    assert_eq!(
        names(&r, t.path()),
        [
            ".git/HEAD",
            "a/1.log",
            "a/b/2.log",
            "a/b/c/3.log",
            "node_modules/x/5.js",
            "top.txt",
            "z/4.txt"
        ]
    );
    assert_eq!(found.load(Ordering::Relaxed), 7);
    assert!(r.issues.is_empty() && r.issues_total == 0);
    assert!(
        r.entries
            .iter()
            .all(|e| e.kind == EntryKind::File && e.size.is_none())
    );
}

#[test]
fn result_is_identical_for_any_worker_count() {
    let t = tempfile::tempdir().unwrap();
    tree(t.path());
    let run = |n| {
        let o = WalkOptions {
            workers: Workers::new(n),
            include_dirs: true,
            ..WalkOptions::default()
        };
        par_walk(t.path(), &o, &CancelToken::new(), &())
            .unwrap()
            .entries
    };
    let one = run(1);
    assert!(one.iter().any(|e| e.kind == EntryKind::Dir));
    for n in [2, 5, 16] {
        assert_eq!(run(n), one);
    }
}

#[test]
fn filter_hook_skips_and_prunes() {
    let t = tempfile::tempdir().unwrap();
    tree(t.path());
    let o = WalkOptions::default().with_filter(|e| {
        let n = e.path.file_name().unwrap().to_string_lossy();
        match (e.kind, n.as_ref()) {
            (EntryKind::Dir, ".git" | "node_modules") => Decision::SkipSubtree,
            (EntryKind::Dir, "z") => Decision::Skip,
            (EntryKind::File, "top.txt") => Decision::Skip,
            _ => Decision::Include,
        }
    });
    let r = par_walk(
        t.path(),
        &WalkOptions {
            include_dirs: true,
            ..o
        },
        &CancelToken::new(),
        &(),
    )
    .unwrap();
    // "z" is not reported but still entered; the pruned folders are neither reported nor entered.
    assert_eq!(
        names(&r, t.path()),
        [
            "a",
            "a/1.log",
            "a/b",
            "a/b/2.log",
            "a/b/c",
            "a/b/c/3.log",
            "z/4.txt"
        ]
    );
}

#[test]
fn depth_limit_and_sizes() {
    let t = tempfile::tempdir().unwrap();
    tree(t.path());
    let o = WalkOptions {
        max_depth: Some(0),
        with_size: true,
        include_dirs: true,
        ..WalkOptions::default()
    };
    let r = par_walk(t.path(), &o, &CancelToken::new(), &()).unwrap();
    assert_eq!(
        names(&r, t.path()),
        [".git", "a", "node_modules", "top.txt", "z"]
    );
    assert_eq!(
        r.entries
            .iter()
            .find(|e| e.kind == EntryKind::File)
            .unwrap()
            .size,
        Some(7)
    );
    let o = WalkOptions {
        max_depth: Some(1),
        ..WalkOptions::default()
    };
    let r = par_walk(t.path(), &o, &CancelToken::new(), &()).unwrap();
    assert!(
        names(&r, t.path()).contains(&"a/1.log".to_string())
            && !names(&r, t.path()).contains(&"a/b/2.log".to_string())
    );
}

#[test]
fn root_errors_are_failures_not_panics() {
    let t = tempfile::tempdir().unwrap();
    let missing = par_walk(
        &t.path().join("nope"),
        &WalkOptions::default(),
        &CancelToken::new(),
        &(),
    )
    .unwrap_err();
    assert!(missing.to_string().contains("cannot read"));
    fs::write(t.path().join("f"), "x").unwrap();
    let file = par_walk(
        &t.path().join("f"),
        &WalkOptions::default(),
        &CancelToken::new(),
        &(),
    )
    .unwrap_err();
    assert!(file.to_string().contains("not a folder"));
}

#[test]
fn empty_folder() {
    let t = tempfile::tempdir().unwrap();
    let r = par_walk(t.path(), &WalkOptions::default(), &CancelToken::new(), &()).unwrap();
    assert!(r.entries.is_empty());
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn symlink_policies() {
        let t = tempfile::tempdir().unwrap();
        tree(t.path());
        symlink(t.path().join("a"), t.path().join("link_to_a")).unwrap();
        symlink(t.path().join("top.txt"), t.path().join("link_to_top")).unwrap();
        symlink(t.path(), t.path().join("a/loop")).unwrap();
        symlink(t.path().join("gone"), t.path().join("broken")).unwrap();
        let run = |p| {
            let o = WalkOptions {
                symlinks: p,
                ..WalkOptions::default()
            };
            par_walk(t.path(), &o, &CancelToken::new(), &()).unwrap()
        };
        let ignore = run(SymlinkPolicy::Ignore);
        assert_eq!(ignore.entries.len(), 7);
        let report = run(SymlinkPolicy::Report);
        let links: Vec<_> = report
            .entries
            .iter()
            .filter(|e| e.kind == EntryKind::Symlink)
            .collect();
        assert_eq!(links.len(), 4, "links reported, none followed");
        assert_eq!(report.entries.len(), 11);
        let sock = tempfile::tempdir().unwrap();
        let _listener = std::os::unix::net::UnixListener::bind(sock.path().join("s")).unwrap();
        symlink(sock.path().join("s"), t.path().join("link_to_socket")).unwrap();
        let follow = run(SymlinkPolicy::Follow);
        assert!(
            follow
                .entries
                .iter()
                .any(|e| e.path.ends_with("link_to_socket") && e.kind == EntryKind::Other)
        );
        // link_to_a is the same real folder as a, entered once; the loop back to the root is cut; link_to_top is a file.
        assert!(
            follow
                .entries
                .iter()
                .any(|e| e.path.ends_with("link_to_top") && e.kind == EntryKind::File)
        );
        assert_eq!(
            follow
                .entries
                .iter()
                .filter(|e| e.path.ends_with("3.log"))
                .count(),
            1,
            "no duplicate walk of a"
        );
        assert_eq!(follow.issues.len(), 1);
        assert!(follow.issues[0].path.ends_with("broken") && !follow.issues[0].message.is_empty());
    }

    #[test]
    fn unreadable_folders_are_issues_and_the_walk_continues() {
        let t = tempfile::tempdir().unwrap();
        tree(t.path());
        let locked = t.path().join("a/b");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let readable = fs::read_dir(&locked).is_ok(); // running as root ignores modes
        let r = par_walk(
            t.path(),
            &WalkOptions {
                issue_cap: 0,
                ..WalkOptions::default()
            },
            &CancelToken::new(),
            &(),
        )
        .unwrap();
        let r2 = par_walk(t.path(), &WalkOptions::default(), &CancelToken::new(), &()).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        if !readable {
            assert_eq!(r.issues_total, 1);
            assert!(r.issues.is_empty(), "cap 0 keeps none but counts");
            assert_eq!(r2.issues.len(), 1);
            assert!(r2.entries.iter().any(|e| e.path.ends_with("top.txt")));
        }
    }
}

/// Cancels at the first entries reported and counts what is reported in all.
struct CancelAtFirst<'a>(&'a CancelToken, AtomicU64);

impl Counter for CancelAtFirst<'_> {
    fn add(&self, n: u64) {
        self.1.fetch_add(n, Ordering::SeqCst);
        self.0.cancel();
    }
}

fn files(root: &Path, dirs: usize, per_dir: usize) {
    for d in 0..dirs {
        let dir = root.join(format!("d{d}"));
        fs::create_dir(&dir).unwrap();
        for f in 0..per_dir {
            fs::write(dir.join(format!("{f}.txt")), "").unwrap();
        }
    }
}

#[test]
fn cancel_stops_a_big_walk_promptly() {
    // Promptness is counted in entries, never in wall-clock time, with the check interval as a parameter: a handful of
    // files prove it (thousands of files only measured the file system, and timed out on Windows).
    let every = 4;
    // Inside one directory: the first look at the token after the cancel ends the walk.
    let t = tempfile::tempdir().unwrap();
    files(t.path(), 1, 40);
    let cancel = CancelToken::new();
    let c = CancelAtFirst(&cancel, AtomicU64::new(0));
    let o = WalkOptions {
        workers: Workers::one(),
        cancel_check_items: every,
        ..WalkOptions::default()
    };
    let r = par_walk(t.path(), &o, &cancel, &c);
    assert!(matches!(r, Err(JobError::Cancelled(CancelReason::User))));
    assert!(
        c.1.load(Ordering::SeqCst) <= every as u64,
        "{}",
        c.1.load(Ordering::SeqCst)
    );
    // Across workers and directories: every report cancels, and a worker looks at the token right after each report, so
    // each worker reports at most one batch.
    let t = tempfile::tempdir().unwrap();
    files(t.path(), 20, 20);
    let cancel = CancelToken::new();
    let c = CancelAtFirst(&cancel, AtomicU64::new(0));
    let workers = 2;
    let o = WalkOptions {
        workers: Workers::new(workers),
        cancel_check_items: every,
        ..WalkOptions::default()
    };
    let r = par_walk(t.path(), &o, &cancel, &c);
    assert!(matches!(r, Err(JobError::Cancelled(CancelReason::User))));
    let reported = c.1.load(Ordering::SeqCst);
    assert!(reported <= (workers * every) as u64, "{reported} of 400");
}

#[test]
fn the_walk_looks_at_the_cancel_token_every_cancel_check_items_entries_by_default() {
    assert_eq!(
        WalkOptions::default().cancel_check_items,
        paddock_jobs::CANCEL_CHECK_ITEMS
    );
}

#[test]
fn filter_panic_is_contained() {
    let t = tempfile::tempdir().unwrap();
    tree(t.path());
    let o = WalkOptions::default().with_filter(|_| panic!("rule bug"));
    let r = par_walk(t.path(), &o, &CancelToken::new(), &());
    assert!(matches!(r, Err(JobError::Panicked(m)) if m == "rule bug"));
}

#[test]
fn live_count_grows_during_the_walk() {
    let t = tempfile::tempdir().unwrap();
    for d in 0..5 {
        fs::create_dir(t.path().join(format!("d{d}"))).unwrap();
        for f in 0..600 {
            fs::write(t.path().join(format!("d{d}/{f}")), "").unwrap();
        }
    }
    let found = AtomicU64::new(0);
    let r = par_walk(
        t.path(),
        &WalkOptions {
            workers: Workers::one(),
            ..WalkOptions::default()
        },
        &CancelToken::new(),
        &found,
    )
    .unwrap();
    assert_eq!(found.load(Ordering::Relaxed), 3000);
    assert_eq!(r.entries.len(), 3000);
}

#[test]
fn with_modified_reports_the_modification_time_of_files_from_the_same_stat_as_the_size() {
    let t = tempfile::tempdir().unwrap();
    tree(t.path());
    let found = AtomicU64::new(0);
    let opts = WalkOptions {
        with_size: true,
        with_modified: true,
        ..WalkOptions::default()
    };
    let r = par_walk(t.path(), &opts, &CancelToken::new(), &found).unwrap();
    let file = r
        .entries
        .iter()
        .find(|e| e.path.ends_with("top.txt"))
        .unwrap();
    assert_eq!(file.size, Some(7));
    let want = fs::metadata(t.path().join("top.txt"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(file.modified, Some(want));
    // Directories have neither.
    let plain = par_walk(
        t.path(),
        &WalkOptions::default(),
        &CancelToken::new(),
        &found,
    )
    .unwrap();
    assert!(
        plain
            .entries
            .iter()
            .all(|e| e.modified.is_none() && e.size.is_none()),
        "off unless asked for"
    );
    let only_time = par_walk(
        t.path(),
        &WalkOptions {
            with_modified: true,
            ..WalkOptions::default()
        },
        &CancelToken::new(),
        &found,
    )
    .unwrap();
    assert!(
        only_time
            .entries
            .iter()
            .all(|e| e.size.is_none() && e.modified.is_some())
    );
}
