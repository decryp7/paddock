use crate::CANCEL_CHECK_ITEMS;
use crate::cancel::CancelToken;
use crate::error::{JobError, panic_text};
use crate::reporter::Counter;
use crate::workers::Workers;
use std::collections::HashSet;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// What an entry is, without following symlinks (unless the policy says so).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Other,
}

/// One discovered entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkEntry {
    pub path: PathBuf,
    pub kind: EntryKind,
    /// File size when [`WalkOptions::with_size`] is set and the entry is a file.
    pub size: Option<u64>,
    /// Modification time when [`WalkOptions::with_modified`] is set and the entry is a file (from the same `stat` as the size).
    pub modified: Option<std::time::SystemTime>,
}

/// The ignore-rules hook's answer for an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Report it (and descend, for a directory).
    Include,
    /// Do not report it, but still descend into a directory.
    Skip,
    /// Do not report it and do not descend (a directory such as `node_modules`, `.git`).
    SkipSubtree,
}

/// What to do with symbolic links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymlinkPolicy {
    /// Never report or follow them.
    Ignore,
    /// Report them as [`EntryKind::Symlink`] without following (the safe default).
    Report,
    /// Follow links to directories and files; each directory is entered once (loops are cut by canonical path).
    Follow,
}

/// A directory or entry that could not be read; the walk continues past it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkIssue {
    pub path: PathBuf,
    pub message: String,
}

/// The boxed ignore-rules hook.
pub type Filter = Box<dyn Fn(&WalkEntry) -> Decision + Send + Sync>;

/// Options of [`par_walk`].
pub struct WalkOptions {
    pub workers: Workers,
    pub symlinks: SymlinkPolicy,
    /// Directories deeper than this below the root are not read (`Some(0)`: only the root's children).
    pub max_depth: Option<usize>,
    /// Also report directories.
    pub include_dirs: bool,
    /// Fill [`WalkEntry::size`] for reported files (one `stat` each).
    pub with_size: bool,
    /// Fill [`WalkEntry::modified`] for reported files; shares the one `stat` with [`with_size`](Self::with_size).
    pub with_modified: bool,
    /// At most this many issues are kept (the rest are only counted).
    pub issue_cap: usize,
    /// The ignore-rules hook; see [`WalkOptions::with_filter`].
    pub filter: Option<Filter>,
    /// Entries read in a directory between two looks at the cancel token ([`CANCEL_CHECK_ITEMS`]; 0 counts as 1). A
    /// parameter so a test proves promptness with a handful of files instead of thousands.
    pub cancel_check_items: usize,
}

impl Default for WalkOptions {
    fn default() -> Self {
        WalkOptions {
            workers: Workers::auto(),
            symlinks: SymlinkPolicy::Report,
            max_depth: None,
            include_dirs: false,
            with_size: false,
            with_modified: false,
            issue_cap: 1000,
            filter: None,
            cancel_check_items: CANCEL_CHECK_ITEMS,
        }
    }
}

impl WalkOptions {
    /// Installs the ignore-rules hook. It sees path and kind only (no size), runs on worker threads, and must be cheap and pure.
    pub fn with_filter(
        mut self,
        f: impl Fn(&WalkEntry) -> Decision + Send + Sync + 'static,
    ) -> Self {
        self.filter = Some(Box::new(f));
        self
    }
}

/// The outcome of a finished walk. Entries are sorted by path, so the result does not depend on thread scheduling.
#[derive(Debug, Default)]
pub struct WalkResult {
    pub entries: Vec<WalkEntry>,
    pub issues: Vec<WalkIssue>,
    /// All issues, including those past `issue_cap`.
    pub issues_total: u64,
}

struct Queue {
    dirs: Vec<(PathBuf, usize)>,
    active: usize,
    failed: Option<String>,
}

struct Walk<'a> {
    opts: &'a WalkOptions,
    cancel: &'a CancelToken,
    found: &'a dyn Counter,
    queue: Mutex<Queue>,
    cv: Condvar,
    visited: Mutex<HashSet<PathBuf>>,
    issues: Mutex<(Vec<WalkIssue>, u64)>,
    entries: Mutex<Vec<WalkEntry>>,
}

fn kind_of(t: fs::FileType) -> EntryKind {
    if t.is_dir() {
        EntryKind::Dir
    } else if t.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

fn guard<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Walks `root` on a bounded pool of workers (directories are the unit of work, so the walk stays busy on wide and deep trees),
/// adding 1 to `found` per reported entry as it is discovered (a live "12,345 found"), and checking `cancel` at least every
/// [`WalkOptions::cancel_check_items`] entries. Unreadable directories are collected in [`WalkResult::issues`]; only a
/// missing or unreadable root is an error.
pub fn par_walk(
    root: &Path,
    opts: &WalkOptions,
    cancel: &CancelToken,
    found: &dyn Counter,
) -> Result<WalkResult, JobError> {
    let meta = fs::metadata(root)
        .map_err(|e| JobError::Failed(format!("cannot read {}: {e}", root.display())))?;
    if !meta.is_dir() {
        return Err(JobError::Failed(format!(
            "{} is not a folder",
            root.display()
        )));
    }
    let mut visited = HashSet::new();
    if opts.symlinks == SymlinkPolicy::Follow
        && let Ok(c) = fs::canonicalize(root)
    {
        visited.insert(c);
    }
    let walk = Walk {
        opts,
        cancel,
        found,
        queue: Mutex::new(Queue {
            dirs: vec![(root.to_path_buf(), 0)],
            active: 0,
            failed: None,
        }),
        cv: Condvar::new(),
        visited: Mutex::new(visited),
        issues: Mutex::new((Vec::new(), 0)),
        entries: Mutex::new(Vec::new()),
    };
    std::thread::scope(|s| {
        for _ in 0..opts.workers.get() {
            s.spawn(|| walk.worker());
        }
    });
    cancel.check()?;
    if let Some(msg) = guard(&walk.queue).failed.take() {
        return Err(JobError::Panicked(msg));
    }
    let mut entries = std::mem::take(&mut *guard(&walk.entries));
    entries.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    let (mut issues, issues_total) = std::mem::take(&mut *guard(&walk.issues));
    issues.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    Ok(WalkResult {
        entries,
        issues,
        issues_total,
    })
}

impl Walk<'_> {
    fn worker(&self) {
        loop {
            let job = {
                let mut q = guard(&self.queue);
                loop {
                    if self.cancel.is_cancelled() || q.failed.is_some() {
                        self.cv.notify_all();
                        return;
                    }
                    if let Some(d) = q.dirs.pop() {
                        q.active += 1;
                        break d;
                    }
                    if q.active == 0 {
                        self.cv.notify_all();
                        return;
                    }
                    q = self
                        .cv
                        .wait_timeout(q, Duration::from_millis(20))
                        .unwrap_or_else(|p| p.into_inner())
                        .0;
                }
            };
            let result = catch_unwind(AssertUnwindSafe(|| self.read_dir(&job.0, job.1)));
            let mut q = guard(&self.queue);
            q.active -= 1;
            match result {
                Ok(children) => q.dirs.extend(children),
                Err(p) => q.failed = Some(panic_text(&*p)),
            }
            drop(q);
            self.cv.notify_all();
        }
    }

    fn issue(&self, path: &Path, message: String) {
        let mut g = guard(&self.issues);
        g.1 += 1;
        if g.0.len() < self.opts.issue_cap {
            g.0.push(WalkIssue {
                path: path.to_path_buf(),
                message,
            });
        }
    }

    /// Unwraps an I/O result, recording the failure as an issue (the walk continues).
    fn note<T>(&self, path: &Path, r: std::io::Result<T>) -> Option<T> {
        r.map_err(|e| self.issue(path, e.to_string())).ok()
    }

    fn decide(&self, e: &WalkEntry) -> Decision {
        self.opts
            .filter
            .as_ref()
            .map_or(Decision::Include, |f| f(e))
    }

    /// Reads one directory; returns the subdirectories to queue.
    fn read_dir(&self, dir: &Path, depth: usize) -> Vec<(PathBuf, usize)> {
        let mut subdirs = Vec::new();
        let mut local = Vec::new();
        let Some(rd) = self.note(dir, fs::read_dir(dir)) else {
            return subdirs;
        };
        let mut seen = 0usize;
        let mut reported = 0u64;
        for item in rd {
            seen += 1;
            if seen.is_multiple_of(self.opts.cancel_check_items.max(1)) {
                self.flush(&mut local, &mut reported);
                if self.cancel.is_cancelled() {
                    return subdirs;
                }
            }
            let Some(item) = self.note(dir, item) else {
                continue;
            };
            let path = item.path();
            let Some(file_type) = self.note(&path, item.file_type()) else {
                continue;
            };
            let kind = if file_type.is_symlink() {
                let Some(k) = self.symlinks(&path) else {
                    continue;
                };
                k
            } else {
                kind_of(file_type)
            };
            let mut entry = WalkEntry {
                path,
                kind,
                size: None,
                modified: None,
            };
            let decision = self.decide(&entry);
            if decision == Decision::SkipSubtree {
                continue;
            }
            if kind == EntryKind::Dir
                && self.opts.max_depth.is_none_or(|m| depth < m)
                && self.enter(&entry.path)
            {
                subdirs.push((entry.path.clone(), depth + 1));
            }
            let reportable = kind != EntryKind::Dir || self.opts.include_dirs;
            if decision == Decision::Include && reportable {
                if (self.opts.with_size || self.opts.with_modified) && kind == EntryKind::File {
                    // One stat for both.
                    if let Ok(m) = fs::metadata(&entry.path) {
                        entry.size = self.opts.with_size.then_some(m.len());
                        entry.modified = if self.opts.with_modified {
                            m.modified().ok()
                        } else {
                            None
                        };
                    }
                }
                local.push(entry);
                reported += 1;
            }
        }
        self.flush(&mut local, &mut reported);
        subdirs
    }

    fn flush(&self, local: &mut Vec<WalkEntry>, reported: &mut u64) {
        if !local.is_empty() {
            guard(&self.entries).append(local);
            self.found.add(*reported);
            *reported = 0;
        }
    }

    /// The kind of a symlink under the policy, or `None` to skip it.
    fn symlinks(&self, path: &Path) -> Option<EntryKind> {
        match self.opts.symlinks {
            SymlinkPolicy::Ignore => None,
            SymlinkPolicy::Report => Some(EntryKind::Symlink),
            SymlinkPolicy::Follow => self
                .note(path, fs::metadata(path))
                .map(|m| kind_of(m.file_type())),
        }
    }

    /// Whether this directory should be entered; under `Follow`, each real directory is entered once.
    fn enter(&self, path: &Path) -> bool {
        if self.opts.symlinks != SymlinkPolicy::Follow {
            return true;
        }
        self.note(path, fs::canonicalize(path))
            .is_some_and(|c| guard(&self.visited).insert(c))
    }
}
