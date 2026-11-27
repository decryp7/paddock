//! The port of the lock folder: the few file operations the queue, the tickets, the markers and the slot locks need. [`RealStore`] is
//! the OS adapter (the real folder and [`crate::slotfile`]); a test puts an in-memory store in [`crate::lock::Throttle::store`] to
//! explore the queue rules with no disk (ADR 0103's size classes: file-system work is not what the model measures).

use crate::slotfile;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// What a file looks like: its size and its modification time (epoch seconds; `None` when the store cannot say).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stat {
    /// The size in bytes.
    pub len: u64,
    /// The modification time, epoch seconds.
    pub mtime: Option<u64>,
}

/// A held slot lock. Dropping it frees the slot; [`SlotLock::release`] names the intent and reports a failure.
pub trait SlotLock: Send {
    /// Gives the slot back now.
    fn release(self: Box<Self>) -> io::Result<()>;
}

/// The operations of the lock folder. Every method is one OS call or fewer in the real store; none of them blocks on a lock.
pub trait Store: Send + Sync {
    /// Makes the folder (and its parents); a failure is left to the next write to report.
    fn create_dir_all(&self, dir: &Path);
    /// Writes (creates or replaces) the file.
    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    /// Moves `from` over `to` (replacing it).
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    /// Removes the file.
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    /// Reads at most [`crate::record::MAX_RECORD`] bytes.
    fn read_bounded(&self, path: &Path) -> io::Result<Vec<u8>>;
    /// Reads a whole text file.
    fn read_to_string(&self, path: &Path) -> io::Result<String>;
    /// Size and modification time; `None` when there is no such file.
    fn stat(&self, path: &Path) -> Option<Stat>;
    /// The files directly in `dir` (unordered); an unreadable folder is empty.
    fn list(&self, dir: &Path) -> Vec<PathBuf>;
    /// Appends one line (the folder is made when missing); a failure is ignored (a log line is never worth a failed run).
    fn append_line(&self, path: &Path, line: &str);
    /// Tries to take the slot lock at `path` (created when missing). `Ok(None)`: held by another.
    fn take(&self, path: &Path) -> io::Result<Option<Box<dyn SlotLock>>>;
    /// Whether some holder has the slot lock at `path` now (a missing file is free).
    fn is_held(&self, path: &Path) -> io::Result<bool>;
}

impl SlotLock for slotfile::Held {
    fn release(self: Box<Self>) -> io::Result<()> {
        (*self).release()
    }
}

/// The real folder.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealStore;

impl Store for RealStore {
    fn create_dir_all(&self, dir: &Path) {
        let _ = fs::create_dir_all(dir);
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        fs::write(path, bytes)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn read_bounded(&self, path: &Path) -> io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        fs::File::open(path)?
            .take(crate::record::MAX_RECORD as u64)
            .read_to_end(&mut buf)?;
        Ok(buf)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }

    fn stat(&self, path: &Path) -> Option<Stat> {
        let m = fs::metadata(path).ok()?;
        let mtime = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        Some(Stat {
            len: m.len(),
            mtime,
        })
    }

    fn list(&self, dir: &Path) -> Vec<PathBuf> {
        match fs::read_dir(dir) {
            Ok(rd) => rd.filter_map(Result::ok).map(|e| e.path()).collect(),
            Err(_) => Vec::new(),
        }
    }

    fn append_line(&self, path: &Path, line: &str) {
        use std::io::Write;
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }

    fn take(&self, path: &Path) -> io::Result<Option<Box<dyn SlotLock>>> {
        Ok(slotfile::take(path)?.map(|h| Box::new(h) as Box<dyn SlotLock>))
    }

    fn is_held(&self, path: &Path) -> io::Result<bool> {
        slotfile::is_held(path)
    }
}
