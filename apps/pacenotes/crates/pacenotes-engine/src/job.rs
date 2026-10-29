//! The index run as a job.

use crate::{JobProgress, RepoFilesIgnore};
use pacenotes_index::{Denylist, DiskFs, IndexError, IndexLimits, IndexReport, IndexStore, Indexer, Progress, RebuildReason, SymbolSource};
use paddock_jobs::{CancelReason, JobContext, JobError, JobHandle, JobPool, JobRegistry, PoolClosed, Workers};
use paddock_repo_files::WalkLimits;
use std::path::PathBuf;
use std::sync::Arc;

/// The job kind the registry shows for an index run.
pub const JOB_KIND: &str = "pacenotes.index";

/// What an index run needs, fixed for the life of the workspace.
pub struct IndexSetup {
    /// The repository folder.
    pub root: PathBuf,
    /// The index database file (outside the repository).
    pub store_path: PathBuf,
    /// Where facts come from: the parser worker pool, never the process itself.
    pub source: Arc<dyn SymbolSource>,
    /// The secret denylist.
    pub denylist: Arc<Denylist>,
    /// Limits of a run.
    pub limits: IndexLimits,
    /// Threads for the read pass.
    pub read_workers: Workers,
    /// Threads for the extraction pass.
    pub parse_workers: Workers,
}

/// What a finished run returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexOutcome {
    /// What the run did.
    pub report: IndexReport,
    /// Set when an unreadable or outdated index file was replaced: the user is told the index was rebuilt.
    pub rebuilt: Option<RebuildReason>,
}

/// Runs the index as a job and keeps it fresh.
pub struct IndexService {
    pub(crate) registry: Arc<JobRegistry>,
    pub(crate) pool: Arc<JobPool>,
    pub(crate) setup: Arc<IndexSetup>,
}

impl IndexService {
    /// A service over a registry (events), a pool (threads for the job) and the setup.
    pub fn new(registry: Arc<JobRegistry>, pool: Arc<JobPool>, setup: Arc<IndexSetup>) -> IndexService {
        IndexService { registry, pool, setup }
    }

    /// Registers the job and returns at once; the run happens on the pool and reports through the registry's events.
    pub fn start(&self) -> Result<JobHandle<IndexOutcome>, PoolClosed> {
        let subject = self.setup.root.file_name().map(|n| n.to_string_lossy().into_owned());
        let ctx = self.registry.register(JOB_KIND, "Indexing the repository", subject.as_deref());
        let setup = self.setup.clone();
        self.pool.spawn_job(ctx, move |ctx| run(&setup, ctx))
    }
}

fn failed(e: impl std::fmt::Display) -> JobError {
    JobError::Failed(e.to_string())
}

fn run(setup: &IndexSetup, ctx: &JobContext) -> Result<IndexOutcome, JobError> {
    let progress = JobProgress::new(ctx);
    progress.phase("walk", "Finding files", None);
    let fs = DiskFs::new(&setup.root).map_err(failed)?;
    let limits = WalkLimits {
        max_files: setup.limits.snapshot.max_entries,
        max_depth: setup.limits.snapshot.max_depth,
    };
    let ignore = RepoFilesIgnore::scan(fs.root(), limits).map_err(failed)?;
    ctx.check()?;
    if let Some(dir) = setup.store_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| JobError::Failed(format!("could not create the index folder: {}", e.kind())))?;
    }
    let opened = IndexStore::open(&setup.store_path).map_err(failed)?;
    let (mut store, rebuilt) = (opened.store, opened.rebuilt);
    let indexer = Indexer {
        fs: &fs,
        source: setup.source.as_ref(),
        denylist: &setup.denylist,
        limits: setup.limits,
        progress: &progress,
        read_workers: setup.read_workers,
        parse_workers: setup.parse_workers,
        clock: &pacenotes_index::SystemEpoch,
        ignore: &ignore,
    };
    match indexer.run(&mut store, ctx.cancel_token()) {
        Ok(report) => Ok(IndexOutcome { report, rebuilt }),
        Err(IndexError::Cancelled) => Err(JobError::Cancelled(ctx.cancel_token().reason().unwrap_or(CancelReason::User))),
        Err(e) => Err(failed(e)),
    }
}
