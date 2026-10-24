//! The demo's sample engine: two services the shell watches and two jobs it shows. Everything runs on threads of its
//! own (Paddock rule 10): commands return at once, repairs run on the health crate's thread, jobs on the [`JobPool`].

use paddock_health::{Cause, RepairAction, ServiceSpec};
use paddock_jobs::{
    JobContext, JobError, JobHandle, JobId, JobPool, JobRegistry, Phase, PoolClosed, Workers,
};
use serde::Deserialize;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// One step of the simulated work when installed (tests use no delay at all).
pub const TICK: Duration = Duration::from_millis(100);

/// The demo engine's id: its Restart works.
pub const ENGINE: &str = "sd.engine";
/// The search index's id: its Reset fails (no disk space), so the notification keeps its cause.
pub const INDEX: &str = "sd.index";

/// A service of the demo, parsed once from the UI's argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum DemoService {
    #[serde(rename = "sd.engine")]
    Engine,
    #[serde(rename = "sd.index")]
    Index,
}

/// A sample job, parsed once from the UI's argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DemoJob {
    /// Finding files (a growing count), reading them (bytes of a known total), writing the index (cannot be cancelled).
    Import,
    /// Waits for a device and reports nothing until it is cancelled (the UI shows "Still working").
    Quiet,
}

/// The services, in the order the status bar lists them. Each repair is the shell's code and never goes through the
/// broken service (rule 19); it runs on a thread of the health crate.
pub fn services(tick: Duration) -> Vec<ServiceSpec> {
    vec![
        ServiceSpec::new(ENGINE, "the demo engine").repair(move || {
            // Restarting takes a moment, then the engine serves again.
            std::thread::sleep(tick * 6);
            Ok(())
        }),
        ServiceSpec::new(INDEX, "the search index").repair(|| Err(index_unrepairable())),
    ]
}

/// Why the search index cannot be rebuilt (the demo's failing repair).
pub fn index_unrepairable() -> Cause {
    Cause::new("The search index could not be rebuilt.")
        .hint("Free some disk space, then try again.")
        .detail("rebuild: no space left on device (os error 28)")
        .code("sd.index.rebuild")
}

/// What a service's failure looks like, and what the shell offers to repair it.
pub fn failure(service: DemoService) -> (&'static str, Cause, RepairAction) {
    match service {
        DemoService::Engine => (
            ENGINE,
            Cause::new("The demo engine stopped unexpectedly.")
                .hint("Restarting it brings the engine data back.")
                .detail("demo-engine exited (signal: 6)")
                .code("sd.engine.stopped"),
            RepairAction::Restart,
        ),
        DemoService::Index => (
            INDEX,
            Cause::new("The search index stopped: a segment is damaged.")
                .hint(
                    "Search is unavailable until it is reset; resetting rebuilds it from the logs.",
                )
                .detail("index: checksum mismatch in segment 3")
                .code("sd.index.damaged"),
            RepairAction::Reset,
        ),
    }
}

/// The demo's managed state: its job pool, the tick of its simulated work and when it started.
pub struct Engine {
    pool: JobPool,
    tick: Duration,
    started: Instant,
}

impl Engine {
    /// Two workers: the sample jobs are tiny, and a third one queues (it shows `queued`).
    pub fn new(tick: Duration) -> Engine {
        Engine {
            pool: JobPool::new(Workers::new(2)),
            tick,
            started: Instant::now(),
        }
    }

    /// How long the engine has run.
    pub fn uptime(&self) -> Duration {
        self.started.elapsed()
    }

    /// Registers `job` on the shell's registry and runs it on the pool; returns at once with its id and a handle the
    /// caller may wait on (the command drops it).
    pub fn start(
        &self,
        jobs: &JobRegistry,
        job: DemoJob,
    ) -> Result<(JobId, JobHandle<()>), PoolClosed> {
        let tick = self.tick;
        let ctx = match job {
            DemoJob::Import => jobs.register("demo.import", "Importing logs", Some("3 folders")),
            DemoJob::Quiet => jobs.register("demo.quiet", "Update runner", Some("build-mac-2")),
        };
        let id = ctx.id();
        let handle = match job {
            DemoJob::Import => self.pool.spawn_job(ctx, move |ctx| import(ctx, tick))?,
            DemoJob::Quiet => self.pool.spawn_job(ctx, move |ctx| quiet(ctx, tick))?,
        };
        Ok((id, handle))
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.pool.shutdown();
    }
}

/// Files found by the import's first phase, and the bytes its second reads.
pub const IMPORT_FILES: u64 = 140;
pub const IMPORT_BYTES: u64 = 8 * 1024 * 1024;
const FIND_STEPS: u64 = 20;
const READ_STEPS: u64 = 50;

fn import(ctx: &JobContext, tick: Duration) -> Result<(), JobError> {
    ctx.phase(Phase::discovering("find", "Finding files"));
    for _ in 0..FIND_STEPS {
        ctx.check()?;
        std::thread::sleep(tick);
        ctx.add(IMPORT_FILES / FIND_STEPS);
    }
    ctx.phase(Phase::bytes("read", "Reading", Some(IMPORT_BYTES)));
    for _ in 0..READ_STEPS {
        ctx.check()?;
        std::thread::sleep(tick);
        ctx.add(IMPORT_BYTES / READ_STEPS);
    }
    ctx.set(IMPORT_BYTES);
    ctx.check()?;
    ctx.phase(Phase::items("index", "Writing the index", 1));
    {
        let _guard = ctx.uncancellable("Writing the index");
        std::thread::sleep(tick * 10);
        ctx.set(1);
    }
    Ok(())
}

fn quiet(ctx: &JobContext, tick: Duration) -> Result<(), JobError> {
    ctx.phase(Phase::discovering("wait", "Waiting for the device"));
    // Nothing to report until the device answers (it never does): only a cancel ends it. A zero tick (tests) still
    // yields, so the loop does not spin.
    let step = tick.max(Duration::from_millis(5));
    loop {
        ctx.check()?;
        std::thread::sleep(step);
    }
}

/// The shell's job registry of the app (the `paddock-jobs` state the plugin manages).
pub fn registry<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<Arc<JobRegistry>> {
    use tauri::Manager;
    app.try_state::<paddock_jobs::tauri::Jobs>()
        .map(|j| j.registry().clone())
}
