//! Bounded sizing and confined removal of files and folders. Plain `std::fs`, no Tauri.
//!
//! - Links (and Windows junctions) are never followed: sizing counts them as nothing, removal removes the link itself and leaves
//!   its target alone.
//! - Sizing stops at an entry budget, a depth limit and a deadline shared by the whole scan, and stays on the volume it started on
//!   (Unix); the result says so (`capped`), and the UI shows the sizes as lower bounds.
//! - Removal reports what it could not delete (in use, read-only) instead of failing, and stops at Cancel.

use crate::model::Skipped;
use crate::run::Run;
use paddock_jobs::time::{Clock, Deadline};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Entries one usage scan inspects at most (all categories together).
pub const MAX_ENTRIES: u64 = 500_000;
/// How long one usage scan may take.
pub const MAX_SCAN_TIME: Duration = Duration::from_secs(10);
/// Folder nesting below which nothing is sized or removed.
pub const MAX_DEPTH: usize = 64;
/// Skipped items one result lists.
pub const MAX_SKIPPED_LISTED: usize = 20;

/// What a scan may still inspect.
pub struct Budget {
    left: u64,
    deadline: Deadline,
}

impl Budget {
    /// `entries` entries and `time` on `clock`.
    pub fn new(entries: u64, time: Duration, clock: Arc<dyn Clock>) -> Budget {
        Budget {
            left: entries,
            deadline: Deadline::after(clock, time),
        }
    }

    /// Spends one entry; false once the budget or the time is used up.
    pub fn take(&mut self) -> bool {
        if self.left == 0 || self.deadline.passed() {
            return false;
        }
        self.left -= 1;
        true
    }
}

/// Bytes and files found; `capped` when the scan stopped early (the numbers are then lower bounds).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    /// Bytes in files.
    pub bytes: u64,
    /// Files counted.
    pub files: u64,
    /// The newest modification time among the files and folders seen.
    pub newest: Option<SystemTime>,
    /// The walk stopped early.
    pub capped: bool,
}

#[cfg(unix)]
pub(crate) fn device(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.dev()
}

#[cfg(not(unix))]
pub(crate) fn device(_: &std::fs::Metadata) -> u64 {
    0
}

fn newer(a: Option<SystemTime>, b: Option<SystemTime>) -> Option<SystemTime> {
    a.max(b)
}

/// The size of a file or a folder (recursively), without following links or leaving the volume. Missing paths are empty.
/// `run` is asked for Cancel between entries (a cancelled walk is `capped`).
pub fn size_of(path: &Path, budget: &mut Budget, run: &dyn Run) -> Size {
    let mut size = Size::default();
    if !budget.take() {
        size.capped = true;
        return size;
    }
    let Ok(start) = std::fs::symlink_metadata(path) else {
        return size;
    };
    let volume = device(&start);
    let mut stack = vec![(path.to_path_buf(), 0usize)];
    while let Some((p, depth)) = stack.pop() {
        let Ok(meta) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        let ft = meta.file_type();
        if ft.is_symlink() {
            continue;
        }
        size.newest = newer(size.newest, meta.modified().ok());
        if ft.is_file() {
            size.bytes += meta.len();
            size.files += 1;
            continue;
        }
        if !ft.is_dir() || device(&meta) != volume {
            continue;
        }
        if depth >= MAX_DEPTH {
            size.capped = true;
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&p) else {
            continue;
        };
        for entry in entries.flatten() {
            if run.cancelled() || !budget.take() {
                size.capped = true;
                return size;
            }
            stack.push((entry.path(), depth + 1));
        }
    }
    size
}

/// Why something could not be deleted, in words for the user.
pub fn reason(e: &std::io::Error) -> String {
    reason_on(e, cfg!(windows))
}

fn reason_on(e: &std::io::Error, windows: bool) -> String {
    // ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION: another process has it open.
    if windows && matches!(e.raw_os_error(), Some(32 | 33)) {
        return "in use by a running program".into();
    }
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        return "in use, read-only or not permitted".into();
    }
    e.to_string()
}

/// What a removal left behind.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Removal {
    /// Bytes of the files that were deleted.
    pub freed_bytes: u64,
    /// Files deleted.
    pub removed_files: u64,
    /// The first items left in place (at most [`MAX_SKIPPED_LISTED`]).
    pub skipped: Vec<Skipped>,
    /// All items left in place.
    pub skipped_count: u64,
}

impl Removal {
    /// Records an item left in place.
    pub fn skip(&mut self, path: &Path, reason: impl Into<String>) {
        self.skipped_count += 1;
        if self.skipped.len() < MAX_SKIPPED_LISTED {
            self.skipped.push(Skipped {
                path: path.to_string_lossy().into_owned(),
                reason: reason.into(),
            });
        }
    }
}

/// Removes a file, a link (not its target) or a folder with everything in it. Whatever cannot be deleted is recorded in `out` and
/// left in place, together with the folders that contain it. Returns whether `path` is gone. Stops early (returning false) once
/// `run` reports Cancel.
pub fn remove(path: &Path, run: &dyn Run, out: &mut Removal) -> bool {
    remove_at(path, run, out, 0)
}

fn remove_at(path: &Path, run: &dyn Run, out: &mut Removal, depth: usize) -> bool {
    if run.cancelled() {
        return false;
    }
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return true,
        Err(e) => {
            out.skip(path, reason(&e));
            return false;
        }
    };
    let ft = meta.file_type();
    let gone = |r: std::io::Result<()>, out: &mut Removal| match r {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Err(e) => {
            out.skip(path, reason(&e));
            false
        }
    };
    if ft.is_symlink() {
        // Never what it points to. A link to a folder is a directory entry on Windows (`remove_dir`).
        let r = std::fs::remove_file(path).or_else(|_| std::fs::remove_dir(path));
        return gone(r, out);
    }
    if ft.is_dir() {
        if depth >= MAX_DEPTH {
            out.skip(path, "nested too deeply");
            return false;
        }
        let entries = match std::fs::read_dir(path) {
            Ok(it) => it,
            Err(e) => {
                out.skip(path, reason(&e));
                return false;
            }
        };
        let mut all = true;
        // An entry that cannot even be read leaves its folder non-empty, which the final `remove_dir` reports.
        for entry in entries.flatten() {
            all &= remove_at(&entry.path(), run, out, depth + 1);
        }
        // A folder that still holds something it could not delete (or a cancelled one) stays.
        return all && gone(std::fs::remove_dir(path), out);
    }
    let removed = gone(std::fs::remove_file(path), out);
    if removed {
        out.freed_bytes += meta.len();
        out.removed_files += 1;
    }
    removed
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::run::Recorder;
    use paddock_jobs::time::{FakeClock, SystemClock};

    pub fn write(path: &Path, bytes: usize) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![b'x'; bytes]).unwrap();
    }

    #[allow(dead_code)] // the tests that need links run on Unix; Windows keeps the helper for them
    /// Links `link` to the folder `target`; false when the platform or the rights do not allow it.
    pub fn link_dir(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_dir(target, link).is_ok()
        }
    }

    pub fn standard() -> Budget {
        Budget::new(MAX_ENTRIES, MAX_SCAN_TIME, Arc::new(SystemClock))
    }

    #[test]
    fn sizes_files_and_folders_recursively() {
        let d = tempfile::tempdir().unwrap();
        write(&d.path().join("a.bin"), 10);
        write(&d.path().join("sub/b.bin"), 20);
        write(&d.path().join("sub/deeper/c.bin"), 30);
        let s = size_of(d.path(), &mut standard(), &());
        assert_eq!((s.bytes, s.files, s.capped), (60, 3, false));
        assert!(s.newest.is_some());
        assert_eq!(
            size_of(&d.path().join("a.bin"), &mut standard(), &()).bytes,
            10
        );
        assert_eq!(
            size_of(&d.path().join("missing"), &mut standard(), &()),
            Size::default()
        );
    }

    #[test]
    fn sizing_stops_at_the_entry_budget_the_deadline_and_cancel_and_says_so() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..20 {
            write(&d.path().join(format!("f{i}")), 1);
        }
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let s = size_of(
            d.path(),
            &mut Budget::new(5, Duration::from_secs(60), clock),
            &(),
        );
        assert!(s.capped && s.files < 20);
        // The deadline is on the injected clock: advancing it ends the walk, no sleeping.
        let fake = FakeClock::new();
        let mut b = Budget::new(1_000, Duration::from_secs(10), Arc::new(fake.clone()));
        fake.advance(Duration::from_secs(11));
        let s = size_of(d.path(), &mut b, &());
        assert!(s.capped);
        assert_eq!(s.files, 0);
        // A spent budget caps every later category too.
        let mut spent = Budget::new(0, Duration::from_secs(60), Arc::new(SystemClock));
        assert!(size_of(d.path(), &mut spent, &()).capped);
        // Cancel ends the walk, and the result is a lower bound.
        let cancelled = Recorder::new();
        cancelled.cancel();
        assert!(size_of(d.path(), &mut standard(), &cancelled).capped);
    }

    #[cfg(unix)]
    #[test]
    fn sizing_does_not_follow_links_and_stops_at_the_depth_limit() {
        let d = tempfile::tempdir().unwrap();
        let outside = d.path().join("outside");
        write(&outside.join("big.bin"), 1000);
        let root = d.path().join("root");
        write(&root.join("small.bin"), 1);
        assert!(link_dir(&outside, &root.join("link")));
        assert_eq!(size_of(&root, &mut standard(), &()).bytes, 1);
        let mut deep = d.path().join("deep");
        for _ in 0..MAX_DEPTH + 2 {
            deep = deep.join("d");
        }
        write(&deep.join("f"), 1);
        assert!(size_of(&d.path().join("deep"), &mut standard(), &()).capped);
    }

    #[test]
    fn removal_deletes_everything_and_counts_what_it_freed() {
        let d = tempfile::tempdir().unwrap();
        write(&d.path().join("x/a.bin"), 5);
        write(&d.path().join("x/y/b.bin"), 7);
        let mut out = Removal::default();
        assert!(remove(&d.path().join("x"), &(), &mut out));
        assert!(!d.path().join("x").exists());
        assert_eq!(
            (out.freed_bytes, out.removed_files, out.skipped_count),
            (12, 2, 0)
        );
        assert!(
            remove(&d.path().join("x"), &(), &mut out),
            "already gone counts as removed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn removal_removes_a_link_but_never_its_target() {
        let d = tempfile::tempdir().unwrap();
        let outside = d.path().join("outside");
        write(&outside.join("keep.bin"), 3);
        let root = d.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        assert!(
            link_dir(&outside, &root.join("link")),
            "links are available here"
        );
        let mut out = Removal::default();
        assert!(remove(&root, &(), &mut out));
        assert!(!root.exists());
        assert!(
            outside.join("keep.bin").is_file(),
            "the link's target must survive"
        );
        assert_eq!(out.freed_bytes, 0);
    }

    #[test]
    fn a_cancelled_removal_stops_and_keeps_the_rest() {
        let d = tempfile::tempdir().unwrap();
        write(&d.path().join("x/a.bin"), 5);
        let cancelled = Recorder::new();
        cancelled.cancel();
        assert!(!remove(
            &d.path().join("x"),
            &cancelled,
            &mut Removal::default()
        ));
        assert!(d.path().join("x/a.bin").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn what_cannot_be_deleted_is_listed_with_its_reason_and_its_folder_stays() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        write(&d.path().join("x/locked/a.bin"), 5);
        write(&d.path().join("x/b.bin"), 2);
        let locked = d.path().join("x/locked");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        let mut out = Removal::default();
        let root_user = std::fs::remove_file(locked.join("a.bin")).is_ok();
        let gone = remove(&d.path().join("x"), &(), &mut out);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        if !root_user {
            assert!(!gone);
            assert!(
                out.skipped_count >= 1 && out.skipped[0].reason.contains("not permitted"),
                "{out:?}"
            );
            assert!(!d.path().join("x/b.bin").exists(), "the rest was removed");
            assert!(d.path().join("x/locked").exists());
        }
    }

    #[test]
    fn skipped_items_are_counted_but_listed_only_up_to_the_cap() {
        let mut r = Removal::default();
        for i in 0..(MAX_SKIPPED_LISTED + 5) {
            r.skip(Path::new(&format!("f{i}")), "in use");
        }
        assert_eq!(r.skipped.len(), MAX_SKIPPED_LISTED);
        assert_eq!(r.skipped_count, MAX_SKIPPED_LISTED as u64 + 5);
    }

    #[test]
    fn reasons_are_in_words() {
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(reason(&denied).contains("not permitted"));
        assert_eq!(reason(&std::io::Error::other("boom")), "boom");
        let sharing = std::io::Error::from_raw_os_error(32);
        assert_eq!(reason_on(&sharing, true), "in use by a running program");
        assert_ne!(reason_on(&sharing, false), "in use by a running program");
    }

    #[test]
    fn the_newest_time_is_the_later_one_and_nothing_is_older_than_something() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(5);
        assert_eq!(newer(Some(t), None), Some(t));
        assert_eq!(newer(None, Some(t)), Some(t));
        assert_eq!(newer(None, None), None);
        assert_eq!(
            newer(Some(t), Some(t + Duration::from_secs(1))),
            Some(t + Duration::from_secs(1))
        );
    }

    #[test]
    fn a_folder_nested_deeper_than_the_limit_is_left_in_place_with_a_reason() {
        let d = tempfile::tempdir().unwrap();
        let mut deep = d.path().join("deep");
        for _ in 0..MAX_DEPTH + 2 {
            deep = deep.join("d");
        }
        write(&deep.join("f"), 1);
        let mut out = Removal::default();
        assert!(!remove(&d.path().join("deep"), &(), &mut out));
        assert!(out.skipped.iter().any(|s| s.reason == "nested too deeply"));
    }

    #[cfg(unix)]
    #[test]
    fn sockets_and_other_special_files_are_not_sized_and_unreadable_folders_are_skipped_in_words() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        write(&d.path().join("f"), 3);
        let _socket = std::os::unix::net::UnixListener::bind(d.path().join("sock")).unwrap();
        let s = size_of(d.path(), &mut standard(), &());
        assert_eq!(
            (s.bytes, s.files),
            (3, 1),
            "a socket is neither a file nor a folder"
        );
        // A folder nobody may read, and a path behind one.
        let closed = d.path().join("closed");
        write(&closed.join("inner/x"), 1);
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o000)).unwrap();
        let root_user = std::fs::read_dir(&closed).is_ok();
        let unreadable = size_of(&closed, &mut standard(), &());
        let mut out = Removal::default();
        let behind = remove(&closed.join("inner"), &(), &mut out);
        let itself = remove(&closed, &(), &mut out);
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o755)).unwrap();
        if !root_user {
            assert_eq!(unreadable.files, 0);
            assert!(!behind && !itself);
            assert!(out.skipped_count >= 2, "{out:?}");
        }
    }
}
