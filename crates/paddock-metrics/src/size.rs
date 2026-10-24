//! A folder's size, walked within limits.

use paddock_jobs::CancelToken;
use paddock_jobs::time::{Clock, Deadline};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// What stops a walk early.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    /// More entries than [`SizeLimits::max_entries`].
    Entries,
    /// The time limit passed.
    Time,
    /// The cancel token fired.
    Cancelled,
}

/// The bounds of a walk (a runner folder can hold millions of files).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeLimits {
    /// Most entries (files and folders) looked at.
    pub max_entries: usize,
    /// Deepest folder level entered (the root is 0); deeper folders are counted as skipped.
    pub max_depth: usize,
    /// Longest the walk may take on the clock.
    pub max_time: Duration,
}

impl Default for SizeLimits {
    fn default() -> Self {
        SizeLimits {
            max_entries: 2_000_000,
            max_depth: 64,
            max_time: Duration::from_secs(20),
        }
    }
}

/// The result of a walk. `bytes` is a lower bound whenever `stopped` is set or anything was `skipped`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FolderSize {
    /// Total size of the files seen.
    pub bytes: u64,
    /// How many files.
    pub files: u64,
    /// Folders and entries not entered or not readable (permissions, too deep, vanished while walking).
    pub skipped: u64,
    /// Folders on another volume (a mount inside the tree) that were not entered: they are not this folder's size.
    pub other_volumes: u64,
    /// Why the walk ended before the end, if it did.
    pub stopped: Option<Limit>,
}

impl FolderSize {
    /// Whether the figure is exact.
    pub fn is_exact(&self) -> bool {
        self.stopped.is_none() && self.skipped == 0
    }
}

/// What one folder entry is, for the walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A file of this many bytes.
    File(u64),
    /// A folder, with the id of its volume when the OS says (device id on Unix, volume serial number on Windows).
    Dir(Option<u64>),
    /// A link or something that is neither: counted as nothing, never followed.
    Other,
}

/// The entries of one folder, read one at a time: a folder of a million files is walked entry by entry, so the limits and the
/// cancel are looked at between reads, not after the whole listing.
pub type Entries = Box<dyn Iterator<Item = (PathBuf, EntryKind)>>;

fn list(dir: &Path) -> io::Result<Entries> {
    let read = std::fs::read_dir(dir)?;
    Ok(Box::new(read.filter_map(|entry| {
        let entry = entry.ok()?;
        let kind = match entry.metadata() {
            // `DirEntry::metadata` does not follow links.
            Ok(m) if m.is_file() => EntryKind::File(m.len()),
            Ok(m) if m.is_dir() => EntryKind::Dir(crate::native::dir_volume(&entry.path(), &m)),
            _ => EntryKind::Other,
        };
        Some((entry.path(), kind))
    })))
}

/// The size of the folder at `root`: depth first, no link is followed, folders on other volumes are not entered, bounded by
/// `limits` and stopped by `cancel`.
///
/// # Errors
/// The root itself cannot be read.
pub fn folder_size(
    root: &Path,
    limits: SizeLimits,
    clock: Arc<dyn Clock>,
    cancel: &CancelToken,
) -> io::Result<FolderSize> {
    folder_size_with(
        root,
        crate::native::volume_id(root),
        limits,
        clock,
        cancel,
        &list,
    )
}

/// [`folder_size`] over any listing of a folder (`list` gives the entries of a folder; tests use an in-memory tree and a
/// listing that counts how many entries were read). `root_volume` is the root's volume id: a folder with another one is not
/// entered; `None` on either side means unknown and the folder is entered.
///
/// # Errors
/// The root itself cannot be listed.
pub fn folder_size_with(
    root: &Path,
    root_volume: Option<u64>,
    limits: SizeLimits,
    clock: Arc<dyn Clock>,
    cancel: &CancelToken,
    list: &dyn Fn(&Path) -> io::Result<Entries>,
) -> io::Result<FolderSize> {
    let deadline = Deadline::after(clock, limits.max_time);
    let mut size = FolderSize::default();
    let mut seen = 0usize;
    // The root must be readable; below it, a folder that cannot be read is skipped and counted.
    let mut stack = vec![(0usize, list(root)?)];
    while let Some((depth, entries)) = stack.last_mut() {
        let depth = *depth;
        let Some((path, kind)) = entries.next() else {
            stack.pop();
            continue;
        };
        seen += 1;
        if seen > limits.max_entries {
            size.stopped = Some(Limit::Entries);
            return Ok(size);
        }
        if cancel.is_cancelled() {
            size.stopped = Some(Limit::Cancelled);
            return Ok(size);
        }
        if deadline.passed() {
            size.stopped = Some(Limit::Time);
            return Ok(size);
        }
        match kind {
            EntryKind::File(len) => {
                size.bytes = size.bytes.saturating_add(len);
                size.files += 1;
            }
            EntryKind::Dir(volume) if volume.zip(root_volume).is_some_and(|(v, r)| v != r) => {
                size.other_volumes += 1
            }
            EntryKind::Dir(_) if depth >= limits.max_depth => size.skipped += 1,
            EntryKind::Dir(_) => match list(&path) {
                Ok(children) => stack.push((depth + 1, children)),
                Err(_) => size.skipped += 1,
            },
            EntryKind::Other => {}
        }
    }
    Ok(size)
}
