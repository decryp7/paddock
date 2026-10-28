//! Long app-side work as jobs (Paddock rules 10-12, ADR 0008): cleaning a disk, registering a runner, pruning the
//! container engine and trimming the Actions cache. A command registers the job, queues it on the pool and returns its
//! id at once; the UI follows the job through the shell (phases, "Still working", Cancel) and never times the work out. The
//! service request itself cannot be interrupted once sent, so the job is cancellable until then and says so after.
//! A job's result (what the UI shows when it ends) is kept here and read once with `job_result`.

use crate::dto::{CommandError, CommandErrorCode};
use paddock_jobs::{JobContext, JobError, JobPool, JobRegistry, Phase, Workers};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, Runtime};

/// How many finished results are kept (the UI reads one right after its job ends).
const KEEP_RESULTS: usize = 32;

/// The pool and the results of the app's jobs (managed state).
pub struct Work {
    pool: JobPool,
    results: Arc<Mutex<Results>>,
}

#[derive(Default)]
struct Results {
    by_id: HashMap<u64, serde_json::Value>,
    order: VecDeque<u64>,
}

impl Results {
    fn put(&mut self, id: u64, value: serde_json::Value) {
        self.by_id.insert(id, value);
        self.order.push_back(id);
        while self.order.len() > KEEP_RESULTS {
            if let Some(old) = self.order.pop_front() {
                self.by_id.remove(&old);
            }
        }
    }
}

impl Default for Work {
    fn default() -> Self {
        // Two at a time: the service serialises most of this anyway.
        Work {
            pool: JobPool::new(Workers::new(2)),
            results: Arc::default(),
        }
    }
}

impl Work {
    /// The result of a finished job (once), `None` when it has none or was already read.
    pub fn take_result(&self, id: u64) -> Option<serde_json::Value> {
        let mut r = self.results.lock().unwrap_or_else(|p| p.into_inner());
        r.order.retain(|i| *i != id);
        r.by_id.remove(&id)
    }

    /// Registers a job and queues `body` on the pool; returns the id at once. `body` runs the (blocking) request; it is
    /// cancellable until it starts, then uncancellable while the service works.
    pub fn start<T, F>(&self, jobs: &JobRegistry, kind: &str, title: &str, subject: Option<&str>, body: F) -> Result<u64, CommandError>
    where
        T: Serialize + Send + 'static,
        F: FnOnce() -> Result<T, CommandError> + Send + 'static,
    {
        let ctx = jobs.register(kind, title, subject);
        let id = ctx.id().0;
        let results = self.results.clone();
        self.pool
            .spawn_job(ctx, move |ctx: &JobContext| {
                ctx.phase(Phase::discovering("service", "Waiting for the service"));
                ctx.check()?;
                let _guard = ctx.uncancellable("The service is already doing this");
                let out = body().map_err(|e| JobError::Failed(e.message))?;
                if let Ok(v) = serde_json::to_value(out) {
                    results.lock().unwrap_or_else(|p| p.into_inner()).put(id, v);
                }
                Ok(())
            })
            .map_err(|_| CommandError::new(CommandErrorCode::ServiceUnavailable, "Pitcrew is shutting down."))?;
        Ok(id)
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        self.pool.shutdown();
    }
}

/// The shell's job registry of the app.
pub fn registry<R: Runtime>(app: &AppHandle<R>) -> Result<Arc<JobRegistry>, CommandError> {
    app.try_state::<paddock_jobs::tauri::Jobs>()
        .map(|j| j.registry().clone())
        .ok_or_else(|| CommandError::internal("the job registry is not set up"))
}

/// `job_result`: what a finished job produced (once).
#[tauri::command]
pub async fn job_result<R: Runtime>(app: AppHandle<R>, id: u64) -> Result<Option<serde_json::Value>, CommandError> {
    Ok(app.state::<Work>().take_result(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_are_read_once_and_bounded() {
        let mut r = Results::default();
        for i in 0..(KEEP_RESULTS as u64 + 5) {
            r.put(i, serde_json::json!(i));
        }
        assert_eq!(r.by_id.len(), KEEP_RESULTS);
        assert!(!r.by_id.contains_key(&0));
        let w = Work {
            pool: JobPool::new(Workers::new(1)),
            results: Arc::new(Mutex::new(r)),
        };
        assert_eq!(w.take_result(36), Some(serde_json::json!(36)));
        assert_eq!(w.take_result(36), None);
    }
}
