//! The settle rule (from Debrief's watched folders, ADR 0048): decide which files that appear in a folder are ready to import.
//!
//! The decision is a pure state machine ([`Settle`]) fed with snapshots of the folder ([`scan_folder`] makes
//! them), so every awkward case is a unit test: a file still being written (its size or time keeps changing, or it
//! has a temporary name), an empty file, a file that vanished before it was read, one that is locked (retried with
//! backoff, then given up), and one too large to import automatically. Nothing here imports; the caller does that and
//! reports back with [`Settle::imported`] or [`Settle::failed`].

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Quiet time a file needs (unchanged size and modification time) before it is imported.
pub const DEFAULT_DEBOUNCE_MS: u64 = 2_000;
/// Largest file imported automatically; bigger ones are reported once and left to the user.
pub const DEFAULT_MAX_FILE_BYTES: u64 = 4 << 30;
/// Import attempts for a file that fails (locked, unreadable) before it is given up.
pub const MAX_ATTEMPTS: u32 = 4;
/// Files remembered as pending at once; more are ignored until some settle.
pub const MAX_PENDING: usize = 10_000;
/// Directory entries a scan reads at most.
pub const MAX_SCAN_ENTRIES: usize = 50_000;
/// Deepest folder a scan enters.
pub const MAX_SCAN_DEPTH: usize = 16;

/// Quiet time with no further file becoming ready before the ready ones are imported together.
pub const DEFAULT_BATCH_QUIET_MS: u64 = 3_000;
/// The longest the first ready file waits for the batch to fill.
pub const DEFAULT_BATCH_MAX_WAIT_MS: u64 = 30_000;
/// Ready files that flush a batch at once.
pub const DEFAULT_BATCH_MAX_FILES: usize = 500;

/// Decides when the ready files of a folder are imported as one batch (one import job, one position rebuild, instead
/// of one per file): after the number of ready files stopped growing for `quiet_ms`, or when the first has waited
/// `max_wait_ms`, or at once when `max_files` are ready. Pure: the caller supplies the clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Batcher {
    /// Quiet time with the ready count unchanged.
    pub quiet_ms: u64,
    /// Longest the first ready file waits.
    pub max_wait_ms: u64,
    /// Ready files that flush at once.
    pub max_files: usize,
    first_ready: Option<u64>,
    last_count: usize,
    last_change: u64,
}

impl Default for Batcher {
    fn default() -> Self {
        Batcher {
            quiet_ms: DEFAULT_BATCH_QUIET_MS,
            max_wait_ms: DEFAULT_BATCH_MAX_WAIT_MS,
            max_files: DEFAULT_BATCH_MAX_FILES,
            first_ready: None,
            last_count: 0,
            last_change: 0,
        }
    }
}

impl Batcher {
    /// A batcher with explicit timing and size.
    pub fn new(quiet_ms: u64, max_wait_ms: u64, max_files: usize) -> Self {
        Self {
            quiet_ms,
            max_wait_ms,
            max_files,
            ..Self::default()
        }
    }

    /// Whether to import the `ready` files now (`now` in ms). Nothing ready resets the batch; a flush starts the next.
    pub fn flush(&mut self, ready: usize, now: u64) -> bool {
        if ready == 0 {
            self.first_ready = None;
            self.last_count = 0;
            return false;
        }
        let first = *self.first_ready.get_or_insert(now);
        if ready != self.last_count {
            self.last_count = ready;
            self.last_change = now;
        }
        let due = ready >= self.max_files
            || now.saturating_sub(self.last_change) >= self.quiet_ms
            || now.saturating_sub(first) >= self.max_wait_ms;
        if due {
            self.first_ready = None;
            self.last_count = 0;
        }
        due
    }
}

/// How the watcher decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettleConfig {
    /// Quiet time a file needs before it is ready.
    pub debounce_ms: u64,
    /// Largest file taken automatically.
    pub max_file_bytes: u64,
}

impl Default for SettleConfig {
    fn default() -> Self {
        SettleConfig {
            debounce_ms: DEFAULT_DEBOUNCE_MS,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
        }
    }
}

/// A file as the folder showed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStamp {
    /// The file.
    pub path: PathBuf,
    /// Size in bytes.
    pub size: u64,
    /// Last modification (epoch ms).
    pub modified_ms: i64,
}

/// Why a file is not imported automatically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skipped {
    /// Bigger than [`SettleConfig::max_file_bytes`].
    TooLarge {
        /// Its size in bytes.
        size: u64,
    },
    /// Failed [`MAX_ATTEMPTS`] times; the last reason.
    GaveUp {
        /// The last failure.
        reason: String,
    },
}

/// What [`Settle::observe`] found ready.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ready {
    /// Files not seen before: open them.
    pub new: Vec<PathBuf>,
    /// Files already imported that have grown or changed since: refresh them.
    pub changed: Vec<PathBuf>,
    /// Files reported for the first time as not imported.
    pub skipped: Vec<(PathBuf, Skipped)>,
}

impl Ready {
    /// Nothing is ready and nothing was skipped.
    pub fn is_empty(&self) -> bool {
        self.new.is_empty() && self.changed.is_empty() && self.skipped.is_empty()
    }
}

#[derive(Debug)]
struct Pending {
    stamp: FileStamp,
    /// Since when the stamp has not changed (ms on the caller's clock).
    stable_since: u64,
    attempts: u32,
    /// Not before this time after a failure.
    retry_at: u64,
    /// Whether it was imported before (a refresh, not an open).
    known: bool,
}

/// What has been imported, what is settling, and what was already reported.
#[derive(Debug, Default)]
pub struct Settle {
    config: SettleConfig,
    imported: HashMap<PathBuf, (u64, i64)>,
    pending: HashMap<PathBuf, Pending>,
    reported: HashSet<PathBuf>,
    batcher: Batcher,
}

/// Whether a name looks like a file that is still being copied or downloaded, or an editor's scratch file.
pub fn is_temporary_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    name.starts_with('.')
        || name.starts_with("~$")
        || name.ends_with('~')
        || [
            ".tmp",
            ".temp",
            ".part",
            ".partial",
            ".crdownload",
            ".download",
            ".filepart",
            ".swp",
            ".lock",
        ]
        .iter()
        .any(|s| lower.ends_with(s))
}

impl Settle {
    /// A tracker with nothing imported yet.
    pub fn new(config: SettleConfig) -> Settle {
        Settle {
            config,
            ..Settle::default()
        }
    }

    /// Starts from files that are already in the store (their sizes and times), so a restart does not read them again.
    pub fn with_imported(mut self, files: impl IntoIterator<Item = FileStamp>) -> Settle {
        for f in files {
            self.imported.insert(f.path, (f.size, f.modified_ms));
        }
        self
    }

    /// Takes a snapshot of the folder at time `now` (ms) and returns what is ready. Files missing from the snapshot
    /// are forgotten (they vanished); files whose stamp changed start their quiet time again.
    pub fn observe(&mut self, scan: &[FileStamp], now: u64) -> Ready {
        let present: HashSet<&Path> = scan.iter().map(|f| f.path.as_path()).collect();
        self.pending.retain(|p, _| present.contains(p.as_path()));
        self.reported.retain(|p| present.contains(p.as_path()));
        let mut ready = Ready::default();
        for f in scan {
            if f.path
                .file_name()
                .and_then(|n| n.to_str())
                .is_none_or(is_temporary_name)
                || f.size == 0
            {
                self.pending.remove(&f.path);
                continue;
            }
            let known = self.imported.get(&f.path).copied();
            if known == Some((f.size, f.modified_ms)) {
                self.pending.remove(&f.path);
                continue;
            }
            if f.size > self.config.max_file_bytes {
                if self.reported.insert(f.path.clone()) {
                    ready
                        .skipped
                        .push((f.path.clone(), Skipped::TooLarge { size: f.size }));
                }
                continue;
            }
            if self.reported.contains(&f.path) {
                // Given up on this very stamp; a changed file is tried again.
                if self.pending.get(&f.path).is_none_or(|p| p.stamp == *f) {
                    continue;
                }
                self.reported.remove(&f.path);
            }
            if !self.pending.contains_key(&f.path) && self.pending.len() >= MAX_PENDING {
                continue;
            }
            let entry = self
                .pending
                .entry(f.path.clone())
                .or_insert_with(|| Pending {
                    stamp: f.clone(),
                    stable_since: now,
                    attempts: 0,
                    retry_at: 0,
                    known: known.is_some(),
                });
            if entry.stamp != *f {
                entry.stamp = f.clone();
                entry.stable_since = now;
                entry.attempts = 0;
                entry.retry_at = 0;
                entry.known = known.is_some();
            }
            if now.saturating_sub(entry.stable_since) >= self.config.debounce_ms
                && now >= entry.retry_at
            {
                if entry.known {
                    ready.changed.push(f.path.clone());
                } else {
                    ready.new.push(f.path.clone());
                }
            }
        }
        ready.new.sort();
        ready.changed.sort();
        ready.skipped.sort_by(|a, b| a.0.cmp(&b.0));
        ready
    }

    /// The file was imported as `stamp` (what was seen when it was read).
    pub fn imported(&mut self, stamp: &FileStamp) {
        self.imported
            .insert(stamp.path.clone(), (stamp.size, stamp.modified_ms));
        self.pending.remove(&stamp.path);
    }

    /// Reading the file failed (locked, vanished, unreadable). Returns `Some` when it is given up on; otherwise it is
    /// tried again after a growing delay.
    pub fn failed(&mut self, path: &Path, reason: &str, now: u64) -> Option<Skipped> {
        let p = self.pending.get_mut(path)?;
        p.attempts += 1;
        if p.attempts >= MAX_ATTEMPTS {
            self.reported.insert(path.to_path_buf());
            return Some(Skipped::GaveUp {
                reason: reason.to_string(),
            });
        }
        p.retry_at = now + self.config.debounce_ms.saturating_mul(1 << p.attempts);
        None
    }

    /// Whether the files `ready` are to be imported now, together (see [`Batcher`]); the others wait for more.
    pub fn flush_due(&mut self, ready: &Ready, now: u64) -> bool {
        self.batcher
            .flush(ready.new.len() + ready.changed.len(), now)
    }

    /// Files waiting for their quiet time or a retry.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

/// What [`scan_folder`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
    /// The accepted files, sorted by path.
    pub files: Vec<FileStamp>,
    /// A limit ([`MAX_SCAN_ENTRIES`]) cut the scan short.
    pub truncated: bool,
}

/// The files under `root` that `accept` takes (given the path relative to `root`), without following symbolic links, at
/// most [`MAX_SCAN_ENTRIES`] entries and [`MAX_SCAN_DEPTH`] levels deep. [`Scan::truncated`] says a limit cut the scan. Unreadable
/// folders and files that vanish during the scan are skipped.
pub fn scan_folder(root: &Path, accept: &dyn Fn(&Path) -> bool) -> Scan {
    scan_folder_limited(root, accept, MAX_SCAN_ENTRIES, MAX_SCAN_DEPTH)
}

/// [`scan_folder`] with explicit limits (tests use small ones).
pub fn scan_folder_limited(
    root: &Path,
    accept: &dyn Fn(&Path) -> bool,
    max_entries: usize,
    max_depth: usize,
) -> Scan {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut entries = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            entries += 1;
            if entries > max_entries {
                return Scan {
                    files: out,
                    truncated: true,
                };
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                if depth < max_depth {
                    stack.push((path, depth + 1));
                }
            } else if kind.is_file() {
                let rel = path.strip_prefix(root).unwrap_or(&path);
                if !accept(rel) {
                    continue;
                }
                let Ok(meta) = entry.metadata() else { continue };
                let modified_ms = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .and_then(|d| i64::try_from(d.as_millis()).ok())
                    .unwrap_or(0);
                out.push(FileStamp {
                    path,
                    size: meta.len(),
                    modified_ms,
                });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Scan {
        files: out,
        truncated: false,
    }
}
