//! `.gitignore` rules for the walk, from the shared file walker.

use pacenotes_core::RepoPath;
use pacenotes_index::{Ignore, Progress};
use paddock_jobs::{CancelToken, Counter, Decision, JobError, WalkEntry, WalkOptions, Workers, par_walk};
use paddock_repo_files::{RepoFilesError, WalkLimits, walk};
use std::collections::HashSet;
use std::path::Path;
use std::sync::mpsc;

/// The walk of the ignore rules failed.
#[derive(Debug, thiserror::Error)]
#[error("could not read the ignore rules: {0}")]
pub struct IgnoreError(#[from] RepoFilesError);

/// Which files and folders `.gitignore` leaves visible, learned from one [`paddock_repo_files::walk`] of the workspace.
///
/// The shared walker applies the rules (nested `.gitignore` files, negations, no git repository needed) and skips `.git`.
/// A folder is visible when a visible file lies below it, so an ignored folder is never entered by the index's own walk,
/// and a folder with nothing visible in it is left out. The cost is a second walk of the tree; it is measured in
/// `docs/performance.md`.
pub struct RepoFilesIgnore {
    files: HashSet<String>,
    dirs: HashSet<String>,
}

impl RepoFilesIgnore {
    /// Walks `root` with the shared walker.
    pub fn scan(root: &Path, limits: WalkLimits) -> Result<RepoFilesIgnore, IgnoreError> {
        Ok(Self::from_paths(walk(root, limits)?.into_iter().map(|f| f.path)))
    }

    /// As [`scan`](Self::scan), but cancellable and with a live count (ER-18, ER-19).
    ///
    /// The shared `paddock_repo_files::walk` cannot be cancelled and reports nothing until it ends (it took 7.5 s for 120,000
    /// files). So it runs on a thread of its own, and this thread does two things meanwhile: a parallel counting walk
    /// (`paddock_jobs::par_walk`, cancellable) feeds `progress` with the files found so far, and the wait for the shared walk
    /// ends the moment `cancel` fires. On cancel the shared walk is **abandoned**: it finishes by itself, holds only its own data
    /// and its result is dropped. The count ignores `.gitignore` (it is the size of the tree, an upper bound on what is kept) and
    /// costs one extra parallel walk.
    ///
    /// TODO (after the shared-code freeze): `paddock_repo_files::walk` takes a cancel token and a progress counter, and then this
    /// is one walk again.
    pub fn scan_cancellable(
        root: &Path,
        limits: WalkLimits,
        workers: Workers,
        cancel: &CancelToken,
        progress: &dyn Progress,
    ) -> Result<RepoFilesIgnore, JobError> {
        let walk_root = root.to_path_buf();
        let count_root = root.to_path_buf();
        Self::scan_with(
            move || Ok(walk(&walk_root, limits)?.into_iter().map(|f| f.path).collect()),
            || {
                let found = Found(progress, std::sync::atomic::AtomicU64::new(0));
                let options = WalkOptions {
                    workers,
                    ..WalkOptions::default()
                }
                .with_filter(|e: &WalkEntry| {
                    if e.path.file_name().is_some_and(|n| n == ".git") {
                        Decision::SkipSubtree
                    } else {
                        Decision::Include
                    }
                });
                par_walk(&count_root, &options, cancel, &found)?;
                progress.set_total(found.1.load(std::sync::atomic::Ordering::Relaxed));
                Ok(())
            },
            cancel,
        )
    }

    /// The core of [`scan_cancellable`](Self::scan_cancellable) with both walks supplied (a test blocks the shared one).
    pub fn scan_with(
        shared_walk: impl FnOnce() -> Result<Vec<String>, IgnoreError> + Send + 'static,
        count: impl FnOnce() -> Result<(), JobError>,
        cancel: &CancelToken,
    ) -> Result<RepoFilesIgnore, JobError> {
        enum Message {
            Walked(Result<Vec<String>, IgnoreError>),
            Cancelled,
        }
        let (tx, rx) = mpsc::channel();
        let walked = tx.clone();
        std::thread::Builder::new()
            .name("ignore-walk".into())
            .spawn(move || {
                let _ = walked.send(Message::Walked(shared_walk()));
            })
            .map_err(|e| JobError::Failed(format!("could not start the ignore-rule walk: {}", e.kind())))?;
        // wakes the wait below at once; unregistered when dropped
        let _wake = cancel.on_cancel(move || {
            let _ = tx.send(Message::Cancelled);
        });
        count()?;
        cancel.check().map_err(JobError::from)?;
        match rx.recv() {
            Ok(Message::Walked(paths)) => {
                cancel.check().map_err(JobError::from)?;
                paths.map(RepoFilesIgnore::from_paths).map_err(|e| JobError::Failed(e.to_string()))
            }
            Ok(Message::Cancelled) | Err(_) => Err(JobError::from(
                cancel
                    .check()
                    .err()
                    .unwrap_or(paddock_jobs::Cancelled(paddock_jobs::CancelReason::User)),
            )),
        }
    }

    /// The visible set from relative `/`-separated file paths.
    pub fn from_paths(paths: impl IntoIterator<Item = String>) -> RepoFilesIgnore {
        let mut files = HashSet::new();
        let mut dirs = HashSet::new();
        for path in paths {
            let mut end = 0;
            while let Some(i) = path[end..].find('/') {
                end += i;
                dirs.insert(path[..end].to_owned());
                end += 1;
            }
            files.insert(path);
        }
        RepoFilesIgnore { files, dirs }
    }
}

impl Ignore for RepoFilesIgnore {
    fn visible_file(&self, path: &RepoPath) -> bool {
        self.files.contains(path.as_str())
    }

    fn visible_dir(&self, path: &RepoPath) -> bool {
        self.dirs.contains(path.as_str())
    }
}

/// Counts what a walk finds into the job's progress.
struct Found<'a>(&'a dyn Progress, std::sync::atomic::AtomicU64);

impl Counter for Found<'_> {
    fn add(&self, n: u64) {
        self.1.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
        self.0.add(n);
    }
}
