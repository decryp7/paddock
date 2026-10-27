//! Bounded file-system measuring shared by the scanner, the re-check and the disk map: sizes and newest changes of a
//! folder, never following links, stopping at an entry budget or a deadline.

use dyno_core::time::Timestamp;
use std::fs::Metadata;
use std::path::Path;
use std::time::{Instant, SystemTime};

/// What a measurement may spend.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    /// The most entries to look at.
    pub entries: usize,
    /// Stop at this time.
    pub deadline: Instant,
}

impl Budget {
    /// A budget of `entries` entries and `secs` seconds from now.
    pub fn new(entries: usize, secs: u64) -> Budget {
        Budget {
            entries,
            deadline: Instant::now() + std::time::Duration::from_secs(secs),
        }
    }

    /// Spends one entry; false when the budget is used up.
    pub fn spend(&mut self) -> bool {
        if self.entries == 0 || Instant::now() >= self.deadline {
            self.entries = 0;
            return false;
        }
        self.entries -= 1;
        true
    }

    /// Whether the budget ran out.
    pub fn exhausted(&self) -> bool {
        self.entries == 0 || Instant::now() >= self.deadline
    }
}

/// A file or folder's measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Measure {
    /// Bytes (files; links count as zero).
    pub bytes: u64,
    /// The newest modification of the item or anything in it.
    pub newest: Timestamp,
    /// False when the budget ran out before everything was looked at.
    pub complete: bool,
}

/// A modification time as a [`Timestamp`] (the epoch when the OS can't tell).
pub fn modified(meta: &Metadata) -> Timestamp {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(Timestamp(0), |d| Timestamp(i64::try_from(d.as_millis()).unwrap_or(i64::MAX)))
}

/// Measures `path` (already known to be a plain file or folder, not a link) with `meta`, skipping the direct child
/// called `skip` of the top folder (the build output when asking how long a project has been idle).
pub fn measure(path: &Path, meta: &Metadata, skip: Option<&std::ffi::OsStr>, budget: &mut Budget) -> Measure {
    let mut m = Measure {
        bytes: 0,
        newest: modified(meta),
        complete: true,
    };
    if meta.is_dir() {
        descend(path, skip, budget, &mut m);
    } else {
        m.bytes = meta.len();
    }
    m
}

fn descend(dir: &Path, skip: Option<&std::ffi::OsStr>, budget: &mut Budget, m: &mut Measure) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if skip.is_some_and(|s| e.file_name() == s) {
            continue;
        }
        if !budget.spend() {
            m.complete = false;
            return;
        }
        let Ok(meta) = e.path().symlink_metadata() else { continue };
        let t = modified(&meta);
        if t > m.newest {
            m.newest = t;
        }
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            descend(&e.path(), None, budget, m);
            if !m.complete {
                return;
            }
        } else {
            m.bytes = m.bytes.saturating_add(meta.len());
        }
    }
}

/// The user id that owns `path` (Unix), to skip other users' files in shared folders such as `/tmp`.
#[cfg(unix)]
pub fn owner(meta: &Metadata) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    Some(meta.uid())
}

/// The user id that owns `path` (not tracked off Unix).
#[cfg(not(unix))]
pub fn owner(_meta: &Metadata) -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_a_folder_without_following_links() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a/b")).unwrap();
        std::fs::write(dir.path().join("a/b/f"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.path().join("a/g"), vec![0u8; 20]).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/", dir.path().join("a/link")).unwrap();
        let meta = dir.path().join("a").symlink_metadata().unwrap();
        let m = measure(&dir.path().join("a"), &meta, None, &mut Budget::new(1000, 5));
        assert_eq!(m.bytes, 120);
        assert!(m.complete);
        assert!(m.newest.millis() > 0);
        let skipped = measure(
            &dir.path().join("a"),
            &meta,
            Some(std::ffi::OsStr::new("b")),
            &mut Budget::new(1000, 5),
        );
        assert_eq!(skipped.bytes, 20);
    }

    #[test]
    fn a_used_up_budget_says_so() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..5 {
            std::fs::write(dir.path().join(format!("f{i}")), b"x").unwrap();
        }
        let meta = dir.path().symlink_metadata().unwrap();
        let m = measure(dir.path(), &meta, None, &mut Budget::new(2, 5));
        assert!(!m.complete);
        let mut expired = Budget::new(100, 0);
        assert!(!expired.spend() && expired.exhausted());
        let file = dir.path().join("f0");
        let fm = measure(&file, &file.symlink_metadata().unwrap(), None, &mut Budget::new(1, 1));
        assert_eq!(fm.bytes, 1);
    }
}
