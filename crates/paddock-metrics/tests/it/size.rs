//! Folder size: totals, every limit, links, unreadable folders, on an in-memory tree and a real one.

use paddock_jobs::CancelToken;
use paddock_jobs::time::{Clock, FakeClock};
use paddock_metrics::{Entries, EntryKind, Limit, SizeLimits, folder_size, folder_size_with};
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
            (p("/r/d"), EntryKind::Dir(None)),
            (p("/r/l"), EntryKind::Other),
        ],
    );
    t.insert(
        p("/r/d"),
        vec![
            (p("/r/d/b"), EntryKind::File(5)),
            (p("/r/d/e"), EntryKind::Dir(None)),
            (p("/r/d/gone"), EntryKind::Dir(None)),
        ],
    );
    t.insert(p("/r/d/e"), vec![(p("/r/d/e/c"), EntryKind::File(1))]);
    t
}

fn lister(t: Tree) -> impl Fn(&Path) -> io::Result<Entries> {
    move |dir| {
        t.get(dir)
            .cloned()
            .map(|v| Box::new(v.into_iter()) as Entries)
            .ok_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
    }
}

fn run(limits: SizeLimits, cancel: &CancelToken) -> paddock_metrics::FolderSize {
    folder_size_with(
        Path::new("/r"),
        Some(1),
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
        Some(1),
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
        Some(1),
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
        None,
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
    assert_eq!(
        (s.bytes, s.files, s.skipped, s.other_volumes, s.stopped),
        (123, 2, 0, 0, None)
    );
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

fn one_dir_tree(volume: Option<u64>) -> Tree {
    let p = |s: &str| PathBuf::from(s);
    let mut t = Tree::new();
    t.insert(
        p("/r"),
        vec![
            (p("/r/a"), EntryKind::File(10)),
            (p("/r/mnt"), EntryKind::Dir(volume)),
        ],
    );
    t.insert(
        p("/r/mnt"),
        vec![(p("/r/mnt/big"), EntryKind::File(1_000_000))],
    );
    t
}

fn walk(t: Tree, root_volume: Option<u64>) -> paddock_metrics::FolderSize {
    folder_size_with(
        Path::new("/r"),
        root_volume,
        SizeLimits::default(),
        Arc::new(FakeClock::new()),
        &CancelToken::new(),
        &lister(t),
    )
    .unwrap()
}

#[test]
fn a_folder_on_another_volume_is_not_entered_and_is_counted() {
    let s = walk(one_dir_tree(Some(2)), Some(1));
    assert_eq!(
        (s.bytes, s.files, s.other_volumes, s.skipped),
        (10, 1, 1, 0)
    );
    assert!(
        s.is_exact(),
        "not crossing a volume is the intent, not a lower bound"
    );
}

#[test]
fn the_same_volume_or_an_unknown_one_is_entered() {
    assert_eq!(walk(one_dir_tree(Some(1)), Some(1)).bytes, 1_000_010);
    assert_eq!(
        walk(one_dir_tree(None), Some(1)).bytes,
        1_000_010,
        "unknown folder volume"
    );
    assert_eq!(
        walk(one_dir_tree(Some(2)), None).bytes,
        1_000_010,
        "unknown root volume"
    );
}

/// A listing of `n` files that runs `on_read` as each entry is read, counting the reads.
fn endless(
    reads: Arc<std::sync::atomic::AtomicUsize>,
    n: usize,
    on_read: impl Fn(usize) + 'static,
) -> Entries {
    Box::new((0..n).map(move |i| {
        reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        on_read(i);
        (PathBuf::from(format!("/r/{i}")), EntryKind::File(1))
    }))
}

#[test]
fn a_cancel_is_seen_between_reads_of_one_huge_folder() {
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let cancel = CancelToken::new();
    let (r, c) = (reads.clone(), cancel.clone());
    let list = move |_: &Path| -> io::Result<Entries> {
        Ok(endless(r.clone(), 1_000_000, {
            let c = c.clone();
            move |i| {
                if i == 2 {
                    c.cancel()
                }
            }
        }))
    };
    let s = folder_size_with(
        Path::new("/r"),
        None,
        SizeLimits::default(),
        Arc::new(FakeClock::new()),
        &cancel,
        &list,
    )
    .unwrap();
    assert_eq!(s.stopped, Some(Limit::Cancelled));
    assert_eq!(
        reads.load(std::sync::atomic::Ordering::SeqCst),
        3,
        "the other 999997 entries were never read"
    );
    assert_eq!(s.files, 2);
}

#[test]
fn the_time_limit_is_seen_between_reads_of_one_huge_folder() {
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let clock = FakeClock::new();
    let (r, c) = (reads.clone(), clock.clone());
    let list = move |_: &Path| -> io::Result<Entries> {
        Ok(endless(r.clone(), 1_000_000, {
            let c = c.clone();
            move |i| {
                if i == 4 {
                    c.advance(Duration::from_secs(21))
                }
            }
        }))
    };
    let s = folder_size_with(
        Path::new("/r"),
        None,
        SizeLimits::default(),
        Arc::new(clock),
        &CancelToken::new(),
        &list,
    )
    .unwrap();
    assert_eq!(s.stopped, Some(Limit::Time));
    assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 5);
}
