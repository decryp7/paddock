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

/// The one kind of job a dialog window starts and may read the result of (the register window's).
pub const DIALOG_JOB_KIND: &str = "pitcrew.register-runner";

#[derive(Default)]
struct Results {
    by_id: HashMap<u64, (String, serde_json::Value)>,
    order: VecDeque<u64>,
}

impl Results {
    fn put(&mut self, id: u64, kind: &str, value: serde_json::Value) {
        self.by_id.insert(id, (kind.to_string(), value));
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
    /// The result of a finished job (once), `None` when it has none, was already read, or the reader may not have it: a dialog
    /// window may only read the result of the kind of job a dialog starts ([`DIALOG_JOB_KIND`]); the result stays for the
    /// window that may.
    pub fn take_result(&self, id: u64, from_dialog: bool) -> Option<serde_json::Value> {
        let mut r = self.results.lock().unwrap_or_else(|p| p.into_inner());
        if from_dialog && r.by_id.get(&id).is_some_and(|(kind, _)| kind != DIALOG_JOB_KIND) {
            return None;
        }
        r.order.retain(|i| *i != id);
        r.by_id.remove(&id).map(|(_, v)| v)
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
        let kind_owned = kind.to_string();
        self.pool
            .spawn_job(ctx, move |ctx: &JobContext| {
                ctx.phase(Phase::discovering("service", "Waiting for the service"));
                ctx.check()?;
                let _guard = ctx.uncancellable("The service is already doing this");
                let out = body().map_err(|e| JobError::Failed(e.message))?;
                if let Ok(v) = serde_json::to_value(out) {
                    results.lock().unwrap_or_else(|p| p.into_inner()).put(id, &kind_owned, v);
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

/// `job_result`: what a finished job produced (once). A dialog window (`d-...`) only gets the result of the job kind a dialog starts.
#[tauri::command]
pub async fn job_result<R: Runtime>(
    app: AppHandle<R>,
    window: tauri::WebviewWindow<R>,
    id: u64,
) -> Result<Option<serde_json::Value>, CommandError> {
    Ok(app.state::<Work>().take_result(id, window.label().starts_with("d-")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_are_read_once_and_bounded() {
        let mut r = Results::default();
        for i in 0..(KEEP_RESULTS as u64 + 5) {
            r.put(i, DIALOG_JOB_KIND, serde_json::json!(i));
        }
        assert_eq!(r.by_id.len(), KEEP_RESULTS);
        assert!(!r.by_id.contains_key(&0));
        let w = Work {
            pool: JobPool::new(Workers::new(1)),
            results: Arc::new(Mutex::new(r)),
        };
        assert_eq!(w.take_result(36, false), Some(serde_json::json!(36)));
        assert_eq!(w.take_result(36, false), None);
    }

    #[test]
    fn a_dialog_reads_only_the_result_of_the_job_kind_a_dialog_starts() {
        let mut r = Results::default();
        r.put(1, "pitcrew.clean-disk", serde_json::json!("report"));
        r.put(2, DIALOG_JOB_KIND, serde_json::json!("registered"));
        let w = Work {
            pool: JobPool::new(Workers::new(1)),
            results: Arc::new(Mutex::new(r)),
        };
        // Not its job's kind: refused, and the result stays for the main window.
        assert_eq!(w.take_result(1, true), None);
        assert_eq!(w.take_result(1, false), Some(serde_json::json!("report")));
        assert_eq!(w.take_result(2, true), Some(serde_json::json!("registered")));
    }
}
