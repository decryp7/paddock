//! The shell's side of long operations: a registry of running jobs (so the UI can cancel one by id) and the
//! `spotter://job` events that carry their phase-labelled progress to the windows. The progress and cancel types are
//! `spotter_core::job`; this module only names jobs, bounds them and forwards their reports.

use crate::dto::CommandError;
use serde::Serialize;
use spotter_core::job::{Cancel, Job, Progress};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

/// The event the UI listens to for job progress.
pub const JOB_EVENT: &str = "spotter://job";
/// Longest job id accepted from the webview.
const MAX_ID: usize = 64;
/// Most jobs running at once; a webview asking for more is refused.
const MAX_JOBS: usize = 32;

/// What a job is, so the UI knows where to show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobKind {
    /// Downloading or copying a release.
    Download,
    /// Installing the browser for JavaScript pages.
    InstallRenderer,
    /// Checking one watch.
    Check,
    /// Checking every watch.
    CheckAll,
    /// The wizard's test of a source.
    TestSource,
}

/// How a job stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    /// Working; `progress` says where.
    Running,
    /// Done.
    Finished,
    /// Stopped by the user.
    Cancelled,
    /// Ended with an error (the command's answer carries it).
    Failed,
}

/// `JobEvent`: the payload on `spotter://job`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobEventDto {
    /// The id the UI gave the command.
    pub job_id: String,
    /// What the job is.
    pub kind: JobKind,
    /// How it stands.
    pub state: JobState,
    /// The phase it is in (running jobs only).
    pub progress: Option<Progress>,
}

/// Sends an event to the windows (the app's `emit`, or a recorder in tests).
pub type Emit = Arc<dyn Fn(JobEventDto) + Send + Sync>;

/// The running jobs, by id.
#[derive(Default)]
pub struct Jobs {
    running: Mutex<HashMap<String, Cancel>>,
}

fn id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_ID && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl Jobs {
    fn table(&self) -> std::sync::MutexGuard<'_, HashMap<String, Cancel>> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Registers a job under the webview's `id` and returns the [`Job`] to hand to the operation plus the guard that
    /// ends it. Invalid, duplicate and excess ids are refused (the webview is not trusted).
    pub fn begin(self: &Arc<Self>, emit: Emit, id: &str, kind: JobKind) -> Result<(Job, JobGuard), CommandError> {
        if !id_ok(id) {
            return Err(CommandError::bad("The job id is not valid."));
        }
        let cancel = Cancel::new();
        {
            let mut table = self.table();
            if table.len() >= MAX_JOBS || table.contains_key(id) {
                return Err(CommandError::bad("Too many operations are running, or that one already is."));
            }
            table.insert(id.to_string(), cancel.clone());
        }
        let (job_id, sink_emit) = (id.to_string(), emit.clone());
        let job = Job::new(cancel, move |p| {
            sink_emit(JobEventDto {
                job_id: job_id.clone(),
                kind,
                state: JobState::Running,
                progress: Some(p.clone()),
            });
        });
        let guard = JobGuard {
            jobs: self.clone(),
            emit,
            id: id.to_string(),
            kind,
            ended: false,
        };
        Ok((job, guard))
    }

    /// Asks a running job to stop. Unknown ids are fine (it may just have ended).
    pub fn cancel(&self, id: &str) {
        if let Some(c) = self.table().get(id) {
            c.cancel();
        }
    }

    /// How many jobs run now.
    pub fn count(&self) -> usize {
        self.table().len()
    }
}

/// Ends a job: unregisters it and tells the windows how it ended. Dropping it unfinished (a panic) counts as failed.
pub struct JobGuard {
    jobs: Arc<Jobs>,
    emit: Emit,
    id: String,
    kind: JobKind,
    ended: bool,
}

impl JobGuard {
    /// Ends the job as `state`.
    pub fn end(mut self, state: JobState) {
        self.close(state);
    }

    /// Ends the job from the operation's outcome: finished, cancelled (when `job` was cancelled) or failed.
    pub fn end_with(self, job: &Job, ok: bool) {
        let state = if ok {
            JobState::Finished
        } else if job.is_cancelled() {
            JobState::Cancelled
        } else {
            JobState::Failed
        };
        self.end(state);
    }

    fn close(&mut self, state: JobState) {
        if std::mem::replace(&mut self.ended, true) {
            return;
        }
        self.jobs.table().remove(&self.id);
        (self.emit)(JobEventDto {
            job_id: self.id.clone(),
            kind: self.kind,
            state,
            progress: None,
        });
    }
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        self.close(JobState::Failed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spotter_core::job::{Phase, Unit};

    fn recorder() -> (Emit, Arc<Mutex<Vec<JobEventDto>>>) {
        let seen: Arc<Mutex<Vec<JobEventDto>>> = Arc::default();
        let sink = seen.clone();
        (Arc::new(move |e| sink.lock().unwrap().push(e)), seen)
    }

    #[test]
    fn progress_becomes_running_events_and_the_end_is_announced_once() {
        let jobs = Arc::new(Jobs::default());
        let (emit, seen) = recorder();
        let (job, guard) = jobs.begin(emit, "dl-1", JobKind::Download).unwrap();
        assert_eq!(jobs.count(), 1);
        job.report(Phase::Downloading, Unit::Bytes, 5, Some(10));
        guard.end_with(&job, true);
        assert_eq!(jobs.count(), 0);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!((seen[0].state, seen[0].kind), (JobState::Running, JobKind::Download));
        assert_eq!(seen[0].progress.as_ref().unwrap().done, 5);
        assert_eq!((seen[1].state, seen[1].progress.is_none()), (JobState::Finished, true));
        let json = serde_json::to_value(&seen[0]).unwrap();
        assert_eq!(json["jobId"], "dl-1");
        assert_eq!(json["progress"]["label"], "Downloading");
    }

    #[test]
    fn cancel_reaches_the_running_job_and_ends_it_as_cancelled() {
        let jobs = Arc::new(Jobs::default());
        let (emit, seen) = recorder();
        let (job, guard) = jobs.begin(emit, "c_1", JobKind::CheckAll).unwrap();
        jobs.cancel("nobody");
        assert!(!job.is_cancelled());
        jobs.cancel("c_1");
        assert!(job.is_cancelled());
        guard.end_with(&job, false);
        assert_eq!(seen.lock().unwrap().last().unwrap().state, JobState::Cancelled);
    }

    #[test]
    fn a_failure_and_a_dropped_guard_are_announced_as_failed() {
        let jobs = Arc::new(Jobs::default());
        let (emit, seen) = recorder();
        let (job, guard) = jobs.begin(emit.clone(), "f1", JobKind::Check).unwrap();
        guard.end_with(&job, false);
        assert_eq!(seen.lock().unwrap().last().unwrap().state, JobState::Failed);
        let (_job, guard) = jobs.begin(emit, "f2", JobKind::Check).unwrap();
        drop(guard);
        assert_eq!(seen.lock().unwrap().last().unwrap().state, JobState::Failed);
        assert_eq!(jobs.count(), 0, "a dropped guard does not leak the job");
    }

    #[test]
    fn ids_and_numbers_are_bounded() {
        let jobs = Arc::new(Jobs::default());
        let (emit, _) = recorder();
        for bad in ["", "has space", "../x", &"a".repeat(MAX_ID + 1), "nul\0"] {
            assert!(jobs.begin(emit.clone(), bad, JobKind::Download).is_err(), "{bad:?}");
        }
        let _kept = jobs.begin(emit.clone(), "same", JobKind::Download).unwrap();
        assert!(
            jobs.begin(emit.clone(), "same", JobKind::Download).is_err(),
            "no two jobs with one id"
        );
        let mut guards = Vec::new();
        for i in 0..MAX_JOBS - 1 {
            guards.push(jobs.begin(emit.clone(), &format!("j{i}"), JobKind::Check).unwrap());
        }
        assert!(jobs.begin(emit, "one-too-many", JobKind::Check).is_err());
    }
}
