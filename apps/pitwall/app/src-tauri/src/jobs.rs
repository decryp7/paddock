//! Long operations as jobs (ER-17..19): a command that starts one returns the job id at once; progress, the terminal
//! result and a Cancel reach the UI as [`JobEvent`]s on the channel it passed. Nothing here has a timeout and the UI
//! must not add one: a job that is still running is "Still working" (the elapsed seconds keep growing), never abandoned.
//!
//! The core is pure over tokio's clock, so tests run it on tokio's paused (fake) clock without sleeping. Exactly one
//! terminal event is sent per job: whoever removes the job from the table (the job itself or [`Jobs::cancel`]) sends it.

use pitwall_core::mgmt::{CommandError, ErrorCode};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::Instant;

/// How often a running job reports that it is still working.
pub const TICK: Duration = Duration::from_secs(1);

/// One event of a job, as the UI receives it (`kind` is the tag, fields are camelCase).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum JobEvent {
    /// The job exists and has begun.
    #[serde(rename_all = "camelCase")]
    Started {
        /// The job's id.
        job_id: u64,
        /// What the job does, for the UI to show.
        title: String,
        /// Whether Cancel can stop it.
        cancellable: bool,
    },
    /// The job is working. The elapsed seconds only grow (the total is unknown, so there is no percentage).
    #[serde(rename_all = "camelCase")]
    Progress {
        /// The job's id.
        job_id: u64,
        /// The phase now running, in words.
        phase: String,
        /// Whole seconds since the job started.
        elapsed_secs: u64,
    },
    /// The job finished; `result` is what the old blocking command returned.
    #[serde(rename_all = "camelCase")]
    Done {
        /// The job's id.
        job_id: u64,
        /// The command's result.
        result: Value,
    },
    /// The job failed.
    #[serde(rename_all = "camelCase")]
    Failed {
        /// The job's id.
        job_id: u64,
        /// What went wrong and what to do.
        error: CommandError,
    },
    /// Cancel stopped the job.
    #[serde(rename_all = "camelCase")]
    Cancelled {
        /// The job's id.
        job_id: u64,
    },
}

/// Where a job's events go (the UI's channel, or a test's collector).
pub type JobSink = Arc<dyn Fn(JobEvent) + Send + Sync>;

/// What a job body holds to report its phase.
#[derive(Clone)]
pub struct JobContext {
    id: u64,
    started: Instant,
    phase: Arc<Mutex<String>>,
    sink: JobSink,
}

impl JobContext {
    /// Starts a new phase and reports it at once.
    pub fn phase(&self, name: &str) {
        *self.phase.lock().unwrap_or_else(|p| p.into_inner()) = name.to_string();
        self.report();
    }

    fn report(&self) {
        let phase = self.phase.lock().unwrap_or_else(|p| p.into_inner()).clone();
        (self.sink)(JobEvent::Progress {
            job_id: self.id,
            phase,
            elapsed_secs: self.started.elapsed().as_secs(),
        });
    }
}

/// What to run.
pub struct JobSpec {
    /// What the job does, for the UI.
    pub title: String,
    /// The first phase.
    pub phase: String,
    /// Whether Cancel may stop it (false while the OS asks the user, say).
    pub cancellable: bool,
}

/// The running jobs.
#[derive(Default)]
pub struct Jobs {
    next: AtomicU64,
    running: Mutex<HashMap<u64, Entry>>,
}

struct Entry {
    task: JoinHandle<()>,
    cancellable: bool,
    sink: JobSink,
}

impl Jobs {
    /// Starts `body` in the background and returns its id at once. Must be called inside a tokio runtime.
    pub fn start<F, Fut>(self: &Arc<Self>, spec: JobSpec, sink: JobSink, body: F) -> u64
    where
        F: FnOnce(JobContext) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Value, CommandError>> + Send + 'static,
    {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        sink(JobEvent::Started {
            job_id: id,
            title: spec.title,
            cancellable: spec.cancellable,
        });
        let ctx = JobContext {
            id,
            started: Instant::now(),
            phase: Arc::new(Mutex::new(spec.phase)),
            sink: sink.clone(),
        };
        let entry_sink = sink.clone();
        ctx.report();
        let jobs = self.clone();
        // The table lock is held across the spawn, so the job cannot finish before its entry is in.
        let mut table = self.running.lock().unwrap_or_else(|p| p.into_inner());
        let task = tokio::spawn(async move {
            let work = body(ctx.clone());
            tokio::pin!(work);
            let mut tick = tokio::time::interval_at(Instant::now() + TICK, TICK);
            let result = loop {
                tokio::select! {
                    r = &mut work => break r,
                    _ = tick.tick() => ctx.report(),
                }
            };
            if jobs.take(id).is_some() {
                sink(match result {
                    Ok(result) => JobEvent::Done { job_id: id, result },
                    Err(error) => JobEvent::Failed { job_id: id, error },
                });
            }
        });
        table.insert(
            id,
            Entry {
                task,
                cancellable: spec.cancellable,
                sink: entry_sink,
            },
        );
        id
    }

    fn take(&self, id: u64) -> Option<Entry> {
        self.running.lock().unwrap_or_else(|p| p.into_inner()).remove(&id)
    }

    /// Stops a job and sends its `Cancelled` event. An unknown or finished id is fine; a job that cannot be cancelled
    /// is refused with the reason.
    pub fn cancel(&self, id: u64) -> Result<(), CommandError> {
        let mut table = self.running.lock().unwrap_or_else(|p| p.into_inner());
        match table.get(&id) {
            None => Ok(()),
            Some(e) if !e.cancellable => Err(CommandError::new(
                ErrorCode::BadRequest,
                "this step cannot be cancelled now: answer the system prompt, or wait for it to finish",
            )),
            Some(_) => {
                if let Some(e) = table.remove(&id) {
                    e.task.abort();
                    (e.sink)(JobEvent::Cancelled { job_id: id });
                }
                Ok(())
            }
        }
    }

    /// How many jobs are running.
    pub fn running(&self) -> usize {
        self.running.lock().unwrap_or_else(|p| p.into_inner()).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    fn collector() -> (JobSink, Arc<Mutex<Vec<JobEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let e2 = events.clone();
        (Arc::new(move |e| e2.lock().unwrap().push(e)), events)
    }

    fn spec(cancellable: bool) -> JobSpec {
        JobSpec {
            title: "Loading the model".into(),
            phase: "Starting".into(),
            cancellable,
        }
    }

    fn kinds(events: &Mutex<Vec<JobEvent>>) -> Vec<String> {
        events
            .lock()
            .unwrap()
            .iter()
            .map(|e| serde_json::to_value(e).unwrap()["kind"].as_str().unwrap().to_string())
            .collect()
    }

    /// Lets the spawned job run until it waits on something.
    async fn settle() {
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_job_starts_at_once_reports_growing_progress_and_ends_with_its_result() {
        let (sink, events) = collector();
        let jobs = Arc::new(Jobs::default());
        let (finish, finished) = oneshot::channel::<()>();
        let id = jobs.start(spec(true), sink, |ctx| async move {
            ctx.phase("Loading weights");
            finished.await.ok();
            Ok(serde_json::json!({ "loaded": true }))
        });
        settle().await;
        // The call has returned and the job is still running.
        assert_eq!(jobs.running(), 1);
        assert_eq!(kinds(&events), ["started", "progress", "progress"]);
        tokio::time::advance(Duration::from_secs(3)).await;
        settle().await;
        let secs: Vec<u64> = events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                JobEvent::Progress { elapsed_secs, .. } => Some(*elapsed_secs),
                _ => None,
            })
            .collect();
        assert!(secs.windows(2).all(|w| w[0] <= w[1]), "progress never goes backwards: {secs:?}");
        assert!(secs.last().copied() >= Some(3), "still working: {secs:?}");
        finish.send(()).unwrap();
        settle().await;
        assert_eq!(jobs.running(), 0);
        let all = events.lock().unwrap();
        assert_eq!(
            all.last(),
            Some(&JobEvent::Done {
                job_id: id,
                result: serde_json::json!({ "loaded": true })
            })
        );
        assert!(matches!(&all[1], JobEvent::Progress { phase, .. } if phase == "Starting"));
        assert!(matches!(&all[2], JobEvent::Progress { phase, .. } if phase == "Loading weights"));
    }

    #[tokio::test(start_paused = true)]
    async fn a_job_that_takes_very_long_is_never_timed_out() {
        let (sink, events) = collector();
        let jobs = Arc::new(Jobs::default());
        jobs.start(spec(true), sink, |_| std::future::pending());
        settle().await;
        tokio::time::advance(Duration::from_secs(3 * 3600)).await;
        settle().await;
        assert_eq!(jobs.running(), 1, "no timeout ends it");
        assert!(!kinds(&events).iter().any(|k| k == "failed" || k == "done" || k == "cancelled"));
    }

    #[tokio::test(start_paused = true)]
    async fn a_failing_job_sends_failed_with_the_error() {
        let (sink, events) = collector();
        let jobs = Arc::new(Jobs::default());
        let id = jobs.start(spec(true), sink, |_| async {
            Err(CommandError::new(ErrorCode::Internal, "no engine"))
        });
        settle().await;
        assert!(matches!(
            events.lock().unwrap().last(),
            Some(JobEvent::Failed { job_id, error }) if *job_id == id && error.message == "no engine"
        ));
        assert_eq!(jobs.running(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_stops_the_job_and_sends_exactly_one_terminal_event() {
        let (sink, events) = collector();
        let jobs = Arc::new(Jobs::default());
        let id = jobs.start(spec(true), sink, |_| std::future::pending());
        settle().await;
        jobs.cancel(id).unwrap();
        // A second cancel and one of an unknown id are fine and send nothing.
        jobs.cancel(id).unwrap();
        jobs.cancel(999).unwrap();
        tokio::time::advance(Duration::from_secs(5)).await;
        settle().await;
        assert_eq!(jobs.running(), 0);
        let k = kinds(&events);
        assert_eq!(k.iter().filter(|k| *k == "cancelled").count(), 1);
        assert_eq!(k.last().map(String::as_str), Some("cancelled"), "no progress after the end: {k:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn a_step_that_cannot_be_cancelled_says_why_and_keeps_running() {
        let (sink, events) = collector();
        let jobs = Arc::new(Jobs::default());
        let (finish, finished) = oneshot::channel::<()>();
        let id = jobs.start(spec(false), sink, |_| async move {
            finished.await.ok();
            Ok(Value::Null)
        });
        settle().await;
        let err = jobs.cancel(id).unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
        assert_eq!(jobs.running(), 1);
        finish.send(()).unwrap();
        settle().await;
        assert_eq!(kinds(&events).last().map(String::as_str), Some("done"));
    }

    #[test]
    fn events_have_the_camel_case_wire_shape() {
        let v = serde_json::to_value(JobEvent::Progress {
            job_id: 7,
            phase: "x".into(),
            elapsed_secs: 2,
        })
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({ "kind": "progress", "jobId": 7, "phase": "x", "elapsedSecs": 2 })
        );
        let v = serde_json::to_value(JobEvent::Started {
            job_id: 1,
            title: "t".into(),
            cancellable: false,
        })
        .unwrap();
        assert_eq!(v["cancellable"], false);
    }
}
