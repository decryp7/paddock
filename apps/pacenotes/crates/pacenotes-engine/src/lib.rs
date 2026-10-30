//! The engine edge of Pacenotes (ADR 0006): everything that joins the pure index to the shared job, watch and file crates.
//!
//! - [`JobProgress`] adapts a job's reporter to the index's `Progress` port (ER-18: phase-labelled, from the first moment).
//! - [`RepoFilesIgnore`] answers the index's `Ignore` port from `paddock-repo-files`, so `.gitignore` rules are the shared
//!   crate's and not a copy (ER-22). The denylist still wins: it is checked after this and never relaxed by it.
//! - [`IndexService`] runs the index as a job and keeps it fresh with a folder watcher.

mod ignore;
mod job;
mod progress;
mod watcher;

pub use ignore::{IgnoreError, RepoFilesIgnore};
pub use job::{EngineError, IndexOutcome, IndexService, IndexSetup, JOB_KIND};
pub use progress::JobProgress;
pub use watcher::WatchTiming;
