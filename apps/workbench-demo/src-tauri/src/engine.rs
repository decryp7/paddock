//! The demo's one sample job: re-indexing the sample repository, on the shell's job registry. It runs on a pool of its own
//! (Paddock rule 10): the command returns at once and the Jobs view shows the phases and Cancel.

use paddock_jobs::{JobContext, JobError, JobId, JobPool, JobRegistry, Phase, PoolClosed, Workers};
use std::sync::Arc;
use std::time::Duration;

/// One step of the simulated work when installed (tests use no delay at all).
pub const TICK: Duration = Duration::from_millis(100);

/// Files the first phase finds, and the steps of each phase.
pub const FILES: u64 = 240;
const FIND_STEPS: u64 = 20;
const INDEX_STEPS: u64 = 40;

/// The demo's managed state: its job pool and the tick of its simulated work.
pub struct Engine {
    pool: JobPool,
    tick: Duration,
}

impl Engine {
    /// One worker: a second re-index queues behind the first.
    pub fn new(tick: Duration) -> Engine {
        Engine {
            pool: JobPool::new(Workers::new(1)),
            tick,
        }
    }

    /// Registers the re-index on the shell's registry and runs it on the pool; returns at once with its id.
    pub fn reindex(&self, jobs: &JobRegistry) -> Result<JobId, PoolClosed> {
        let ctx = jobs.register("wd.reindex", "Indexing the repository", Some("3 folders"));
        let id = ctx.id();
        let tick = self.tick;
        self.pool.spawn_job(ctx, move |ctx| reindex(ctx, tick))?;
        Ok(id)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.pool.shutdown();
    }
}

fn reindex(ctx: &JobContext, tick: Duration) -> Result<(), JobError> {
    ctx.phase(Phase::discovering("find", "Finding files"));
    for _ in 0..FIND_STEPS {
        ctx.check()?;
        std::thread::sleep(tick);
        ctx.add(FILES / FIND_STEPS);
    }
    ctx.phase(Phase::items("index", "Indexing", FILES));
    for _ in 0..INDEX_STEPS {
        ctx.check()?;
        std::thread::sleep(tick);
        ctx.add(FILES / INDEX_STEPS);
    }
    Ok(())
}

/// The shell's job registry of the app (the `paddock-jobs` state the plugin manages).
pub fn registry<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<Arc<JobRegistry>> {
    use tauri::Manager;
    app.try_state::<paddock_jobs::tauri::Jobs>()
        .map(|j| j.registry().clone())
}
