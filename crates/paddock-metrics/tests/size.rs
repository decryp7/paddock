//! Folder size: totals, every limit, links, unreadable folders, on an in-memory tree and a real one.

use paddock_jobs::CancelToken;
use paddock_jobs::time::{Clock, FakeClock};
use paddock_metrics::{EntryKind, Limit, SizeLimits, folder_size, folder_size_with};
use paddock_testkit::Scratch;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

type Tree = HashMap<PathBuf, Vec<(PathBuf, EntryKind)>>;

fn tree() -> Tree {
    let p = |s: &str| PathBuf::from(s);
    let mut t = Tree::new();
    t.insert(
        p("/r"),
        vec![
            (p("/r/a"), EntryKind::File(10)),
            (p("/r/d"), EntryKind::Dir),
            (p("/r/l"), EntryKind::Other),
        ],
    );
    t.insert(
        p("/r/d"),
        vec![
            (p("/r/d/b"), EntryKind::File(5)),
            (p("/r/d/e"), EntryKind::Dir),
            (p("/r/d/gone"), EntryKind::Dir),
        ],
    );
    t.insert(p("/r/d/e"), vec![(p("/r/d/e/c"), EntryKind::File(1))]);
    t
}

fn lister(t: Tree) -> impl Fn(&Path) -> io::Result<Vec<(PathBuf, EntryKind)>> {
    move |dir| {
        t.get(dir)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
    }
}

fn run(limits: SizeLimits, cancel: &CancelToken) -> paddock_metrics::FolderSize {
    folder_size_with(
        Path::new("/r"),
        limits,
        Arc::new(FakeClock::new()),
        cancel,
        &lister(tree()),
    )
    .unwrap()
}

#[test]
fn it_adds_files_and_counts_what_it_could_not_enter() {
    let s = run(SizeLimits::default(), &CancelToken::new());
    assert_eq!((s.bytes, s.files, s.skipped, s.stopped), (16, 3, 1, None));
    assert!(
        !s.is_exact(),
        "an unreadable folder makes the figure a lower bound"
    );
}

#[test]
fn too_deep_folders_are_skipped() {
    let s = run(
        SizeLimits {
            max_depth: 1,
            ..SizeLimits::default()
        },
        &CancelToken::new(),
    );
    assert_eq!((s.bytes, s.skipped), (15, 2)); // /r/d is entered (depth 1), its two folders are not
}

#[test]
fn the_entry_limit_stops_the_walk() {
    let s = run(
        SizeLimits {
            max_entries: 2,
            ..SizeLimits::default()
        },
        &CancelToken::new(),
    );
    assert_eq!(s.stopped, Some(Limit::Entries));
    assert!(s.bytes <= 10);
}

#[test]
fn a_cancelled_walk_stops_at_once() {
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        run(SizeLimits::default(), &cancel).stopped,
        Some(Limit::Cancelled)
    );
}

#[test]
fn the_time_limit_runs_on_the_clock() {
    let clock = FakeClock::new();
    let l = lister(tree());
    let limits = SizeLimits {
        max_time: Duration::from_secs(5),
        ..SizeLimits::default()
    };
    let ok = folder_size_with(
        Path::new("/r"),
        limits,
        Arc::new(clock.clone()),
        &CancelToken::new(),
        &l,
    )
    .unwrap();
    assert_eq!(ok.stopped, None);
    clock.advance(Duration::from_secs(5));
    let late = folder_size_with(
        Path::new("/r"),
        SizeLimits {
            max_time: Duration::ZERO,
            ..limits
        },
        Arc::new(clock),
        &CancelToken::new(),
        &l,
    )
    .unwrap();
    assert_eq!(late.stopped, Some(Limit::Time));
}

#[test]
fn an_unreadable_root_is_an_error() {
    let r = folder_size_with(
        Path::new("/nope"),
        SizeLimits::default(),
        Arc::new(FakeClock::new()),
        &CancelToken::new(),
        &lister(tree()),
    );
    assert_eq!(r.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn a_real_folder_is_measured_and_links_are_not_followed() {
    let scratch = Scratch::new();
    let root = scratch.work().join("root");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("a.bin"), vec![0u8; 100]).unwrap();
    std::fs::write(root.join("sub").join("b.bin"), vec![0u8; 23]).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&root, root.join("loop")).unwrap();
    let s = folder_size(
        &root,
        SizeLimits::default(),
        Arc::new(FakeClock::new()),
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!((s.bytes, s.files, s.skipped, s.stopped), (123, 2, 0, None));
    assert!(s.is_exact());
    assert!(
        folder_size(
            &root.join("missing"),
            SizeLimits::default(),
            Arc::new(FakeClock::new()),
            &CancelToken::new()
        )
        .is_err()
    );
    assert_eq!(FakeClock::new().monotonic(), Duration::ZERO);
}
