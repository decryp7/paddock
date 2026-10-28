//! Long operations as jobs (ER-17..19) on the shared executor (`paddock-jobs`): a command that starts one returns the
//! job id at once; phase, progress, the terminal result and Cancel reach the UI as [`JobEvent`]s on the channel it passed.
//! Nothing here has a timeout and the UI must not add one: a job that is still running is "Still working" (its count
//! keeps growing), never abandoned.
//!
//! Layers: [`paddock_jobs::JobRegistry`] owns the ids, cancel tokens and snapshots; [`Router`] is its event sink and
//! turns each [`paddock_jobs::JobSnapshot`] into a [`JobEvent`] for the channel of the command that started the job
//! (the one thing the snapshot lacks, the job's result, travels beside it); [`start`] runs a body in the background.
//! Jobs are registered in one place ([`register`]), so keys (`register_keyed`) drop in there later.
//!
//! The core is pure over tokio's clock and a [`paddock_jobs::Clock`], so tests run it on a fake clock without sleeping.

use paddock_jobs::{
    CancelReason, CancelToken, Clock, EventSink, JobContext, JobRegistry, JobSnapshot, JobState, Phase, DEFAULT_MIN_INTERVAL_MS,
};
use pitwall_core::mgmt::{CommandError, ErrorCode};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// How often a running job counts one more second of work (its total is unknown, so there is no percentage).
pub const TICK: Duration = Duration::from_secs(1);

/// One event of a job, as the UI receives it (`kind` is the tag, fields are camelCase).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum JobEvent {
    /// The job exists and is running.
    #[serde(rename_all = "camelCase")]
    Started {
        /// The job's id.
        job_id: u64,
        /// What the job does, for the UI to show.
        title: String,
    },
    /// The job is working. `done` only grows within a phase; with no `total` it counts seconds of work.
    #[serde(rename_all = "camelCase")]
    Progress {
        /// The job's id.
        job_id: u64,
        /// The phase now running, in words.
        phase: String,
        /// Progress within the phase.
        done: u64,
        /// The phase's size when known.
        total: Option<u64>,
        /// Whole seconds since the job started.
        elapsed_secs: u64,
        /// Whether Cancel can stop the job in this phase. False while a step cannot be interrupted (`not_cancellable_reason`
        /// says why); the UI then shows the reason instead of an enabled Cancel.
        cancellable: bool,
        /// Why Cancel is off.
        not_cancellable_reason: Option<String>,
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

/// How a job body ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// It finished with this result.
    Done(Value),
    /// It stopped because Cancel asked, and left things consistent.
    Cancelled,
}

/// What to run.
pub struct JobSpec {
    /// The slug the UI groups by (`load-model`, `benchmark`, ...).
    pub kind: &'static str,
    /// What the job does, for the UI.
    pub title: String,
    /// The job's phase, in words.
    pub phase: String,
    /// Set while the job cannot be interrupted (the OS prompt is the user's to answer): Cancel is off for the whole job
    /// and the events carry this reason.
    pub not_cancellable: Option<String>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

struct Route {
    sink: JobSink,
    started: bool,
    outcome: Option<Result<Value, CommandError>>,
}

/// The registry's event sink: sends each snapshot to the channel of the command that started the job.
struct Router {
    routes: Mutex<HashMap<u64, Route>>,
}

impl Router {
    fn route(&self, id: u64, sink: JobSink) {
        lock(&self.routes).insert(
            id,
            Route {
                sink,
                started: false,
                outcome: None,
            },
        );
    }

    fn set_outcome(&self, id: u64, outcome: Result<Value, CommandError>) {
        if let Some(r) = lock(&self.routes).get_mut(&id) {
            r.outcome = Some(outcome);
        }
    }
}

impl EventSink for Router {
    fn emit(&self, s: &JobSnapshot) {
        let job_id = s.id.0;
        let (sink, event) = {
            let mut routes = lock(&self.routes);
            let Some(route) = routes.get_mut(&job_id) else {
                // The first (queued) snapshot of a job that has no route yet: nothing to tell the UI.
                return;
            };
            let event = match &s.state {
                JobState::Queued => return,
                JobState::Running { phase } => {
                    let first = !std::mem::replace(&mut route.started, true);
                    let progress = JobEvent::Progress {
                        job_id,
                        phase: phase.as_ref().map_or_else(|| "Starting".to_string(), |p| p.phase.label.clone()),
                        done: phase.as_ref().map_or(0, |p| p.done),
                        total: phase.as_ref().and_then(|p| p.phase.total),
                        elapsed_secs: s.elapsed_ms / 1000,
                        cancellable: s.cancellable,
                        not_cancellable_reason: s.not_cancellable_reason.clone(),
                    };
                    if first {
                        // Started and the first progress go out together, in this order.
                        (route.sink)(JobEvent::Started {
                            job_id,
                            title: s.title.clone(),
                        });
                    }
                    progress
                }
                JobState::Done => JobEvent::Done {
                    job_id,
                    result: match route.outcome.take() {
                        Some(Ok(v)) => v,
                        _ => Value::Null,
                    },
                },
                JobState::Failed { message } => JobEvent::Failed {
                    job_id,
                    error: match route.outcome.take() {
                        Some(Err(e)) => e,
                        _ => CommandError::new(ErrorCode::Internal, message.clone()),
                    },
                },
                JobState::Cancelled { .. } => JobEvent::Cancelled { job_id },
            };
            let sink = route.sink.clone();
            if s.state.is_terminal() {
                routes.remove(&job_id);
            }
            (sink, event)
        };
        sink(event);
    }
}

/// The app's long operations: the shared registry plus the router that feeds the channels. Managed Tauri state.
pub struct Jobs {
    registry: JobRegistry,
    router: Arc<Router>,
}

impl Default for Jobs {
    fn default() -> Self {
        Self::with_clock(Arc::new(paddock_jobs::SystemClock), DEFAULT_MIN_INTERVAL_MS)
    }
}

impl Jobs {
    /// A registry on `clock` (a `FakeClock` in tests); progress is throttled to one event per `min_interval_ms`.
    pub fn with_clock(clock: Arc<dyn Clock>, min_interval_ms: u64) -> Self {
        let router = Arc::new(Router { routes: Mutex::default() });
        Jobs {
            registry: JobRegistry::with_clock(router.clone(), clock, min_interval_ms),
            router,
        }
    }

    /// The one place a job is registered (`register_keyed` joins it once keyed jobs land).
    fn register(&self, spec: &JobSpec, sink: &JobSink) -> JobContext {
        // The queued snapshot registering emits has no route yet and tells the UI nothing; the first running one does.
        let ctx = self.registry.register(spec.kind, &spec.title, None);
        self.router.route(ctx.id().0, sink.clone());
        ctx
    }

    /// Asks a job to stop. `Ok(true)` when the request was made; `Ok(false)` when there is nothing to stop (unknown or
    /// finished) or the job cannot be interrupted right now (its events say so: `cancellable: false`).
    pub fn cancel(&self, id: u64) -> bool {
        let id = paddock_jobs::JobId(id);
        let Some(snap) = self.registry.snapshots().into_iter().find(|s| s.id == id) else {
            return false;
        };
        if snap.state.is_terminal() || !snap.cancellable {
            return false;
        }
        self.registry.cancel(id)
    }

    /// How many jobs have not ended.
    pub fn running(&self) -> usize {
        self.registry.snapshots().iter().filter(|s| !s.state.is_terminal()).count()
    }
}

/// Resolves once the job is cancelled (for a body to react: tell the server, stop a download).
pub async fn cancelled(token: &CancelToken) {
    let notify = Arc::new(tokio::sync::Notify::new());
    let n2 = notify.clone();
    // `notify_one` keeps a permit, so a cancel that came before this wait is not lost.
    let _registered = token.on_cancel(move || n2.notify_one());
    notify.notified().await;
}

/// Starts `body` in the background and returns the job's id at once. Must be called inside a tokio runtime.
///
/// The body gets the job's context (its cancel token, to react to Cancel). It ends with [`Outcome::Cancelled`] when it
/// stopped for Cancel; an error after a Cancel counts as the cancel taking effect.
pub fn start<F, Fut>(jobs: &Arc<Jobs>, spec: JobSpec, sink: JobSink, body: F) -> u64
where
    F: FnOnce(JobContext) -> Fut + Send + 'static,
    Fut: Future<Output = Result<Outcome, CommandError>> + Send + 'static,
{
    jobs.registry.forget_finished();
    let ctx = jobs.register(&spec, &sink);
    let id = ctx.id().0;
    let jobs = jobs.clone();
    // Declared before the job starts, so its very first event already says Cancel is off.
    let guard = spec.not_cancellable.as_deref().map(|why| ctx.uncancellable(why));
    ctx.reporter().start();
    ctx.phase(Phase::discovering("work", &spec.phase));
    tokio::spawn(async move {
        let _guard = guard;
        let work = tokio::spawn(body(ctx.clone()));
        tokio::pin!(work);
        let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + TICK, TICK);
        let joined = loop {
            tokio::select! {
                r = &mut work => break r,
                _ = tick.tick() => ctx.add(1),
            }
        };
        let result = joined.unwrap_or_else(|e| {
            Err(CommandError::new(
                ErrorCode::Internal,
                format!("the operation stopped unexpectedly: {e}"),
            ))
        });
        let cancel = ctx.cancel_token().reason();
        ctx.reporter().flush();
        let end = match (result, cancel) {
            (Ok(Outcome::Done(v)), _) => {
                jobs.router.set_outcome(id, Ok(v));
                JobState::Done
            }
            (Ok(Outcome::Cancelled), reason) | (Err(_), reason @ Some(_)) => JobState::Cancelled {
                reason: reason.unwrap_or(CancelReason::User),
            },
            (Err(e), None) => {
                let message = e.message.clone();
                jobs.router.set_outcome(id, Err(e));
                JobState::Failed { message }
            }
        };
        ctx.reporter().finish(end);
    });
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_jobs::FakeClock;
    use tokio::sync::oneshot;

    type Events = Arc<Mutex<Vec<JobEvent>>>;

    fn collector() -> (JobSink, Events) {
        let events: Events = Arc::default();
        let e2 = events.clone();
        (Arc::new(move |e| lock(&e2).push(e)), events)
    }

    fn jobs() -> Arc<Jobs> {
        // Every progress update goes out (no throttle), on a clock only the test moves.
        Arc::new(Jobs::with_clock(Arc::new(FakeClock::new()), 0))
    }

    fn spec(not_cancellable: Option<&str>) -> JobSpec {
        JobSpec {
            kind: "load-model",
            title: "Loading the model".into(),
            phase: "Loading weights".into(),
            not_cancellable: not_cancellable.map(str::to_string),
        }
    }

    fn kinds(events: &Events) -> Vec<String> {
        lock(events)
            .iter()
            .map(|e| serde_json::to_value(e).unwrap()["kind"].as_str().unwrap().to_string())
            .collect()
    }

    /// Lets the spawned job run until it waits on something.
    async fn settle() {
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_job_starts_at_once_reports_growing_progress_and_ends_with_its_result() {
        let (sink, events) = collector();
        let jobs = jobs();
        let (finish, finished) = oneshot::channel::<()>();
        let id = start(&jobs, spec(None), sink, |_| async move {
            finished.await.ok();
            Ok(Outcome::Done(serde_json::json!({ "loaded": true })))
        });
        settle().await;
        // The call has returned and the job is still running, in its phase.
        assert_eq!(jobs.running(), 1);
        assert_eq!(kinds(&events)[0], "started");
        assert!(matches!(&lock(&events)[1], JobEvent::Progress { phase, .. } if phase == "Starting"));
        assert!(matches!(&lock(&events)[2], JobEvent::Progress { phase, cancellable: true, .. } if phase == "Loading weights"));
        tokio::time::advance(Duration::from_secs(3)).await;
        settle().await;
        let done: Vec<u64> = lock(&events)
            .iter()
            .filter_map(|e| match e {
                JobEvent::Progress { done, .. } => Some(*done),
                _ => None,
            })
            .collect();
        assert!(done.windows(2).all(|w| w[0] <= w[1]), "progress never goes backwards: {done:?}");
        assert_eq!(done.last(), Some(&3), "still working: {done:?}");
        finish.send(()).unwrap();
        settle().await;
        assert_eq!(jobs.running(), 0);
        assert_eq!(
            lock(&events).last(),
            Some(&JobEvent::Done {
                job_id: id,
                result: serde_json::json!({ "loaded": true })
            })
        );
        assert_eq!(kinds(&events).iter().filter(|k| *k == "done").count(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_job_that_takes_very_long_is_never_timed_out() {
        let (sink, events) = collector();
        let jobs = jobs();
        start(&jobs, spec(None), sink, |_| std::future::pending());
        settle().await;
        tokio::time::advance(Duration::from_secs(3 * 3600)).await;
        settle().await;
        assert_eq!(jobs.running(), 1, "no timeout ends it");
        assert!(!kinds(&events).iter().any(|k| k == "failed" || k == "done" || k == "cancelled"));
    }

    #[tokio::test(start_paused = true)]
    async fn a_failing_job_sends_failed_with_the_error() {
        let (sink, events) = collector();
        let jobs = jobs();
        let id = start(&jobs, spec(None), sink, |_| async {
            Err(CommandError::new(ErrorCode::NotAvailable, "no engine"))
        });
        settle().await;
        assert!(matches!(
            lock(&events).last(),
            Some(JobEvent::Failed { job_id, error }) if *job_id == id && error.message == "no engine" && error.code == ErrorCode::NotAvailable
        ));
        assert_eq!(jobs.running(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_body_that_panics_fails_the_job_and_nothing_else() {
        let (sink, events) = collector();
        let jobs = jobs();
        start(&jobs, spec(None), sink, |_| async { panic!("boom") });
        settle().await;
        assert!(matches!(lock(&events).last(), Some(JobEvent::Failed { .. })));
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_reaches_the_body_which_ends_in_a_consistent_state_with_one_terminal_event() {
        let (sink, events) = collector();
        let jobs = jobs();
        let (undone, undo) = oneshot::channel::<&'static str>();
        let id = start(&jobs, spec(None), sink, |ctx| async move {
            cancelled(ctx.cancel_token()).await;
            // The body undoes what it began, then says it stopped.
            undone.send("undone").unwrap();
            Ok(Outcome::Cancelled)
        });
        settle().await;
        assert!(jobs.cancel(id));
        settle().await;
        assert_eq!(undo.await.unwrap(), "undone");
        assert_eq!(jobs.running(), 0);
        // A second cancel and one of an unknown id are fine and change nothing.
        assert!(!jobs.cancel(id) && !jobs.cancel(999));
        tokio::time::advance(Duration::from_secs(5)).await;
        settle().await;
        let k = kinds(&events);
        assert_eq!(k.iter().filter(|k| *k == "cancelled").count(), 1);
        assert_eq!(k.last().map(String::as_str), Some("cancelled"), "no progress after the end: {k:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn an_error_after_a_cancel_counts_as_the_cancel_taking_effect() {
        let (sink, events) = collector();
        let jobs = jobs();
        let id = start(&jobs, spec(None), sink, |ctx| async move {
            cancelled(ctx.cancel_token()).await;
            Err(CommandError::new(ErrorCode::ServerUnavailable, "gone"))
        });
        settle().await;
        assert!(jobs.cancel(id));
        settle().await;
        assert_eq!(kinds(&events).last().map(String::as_str), Some("cancelled"));
    }

    #[tokio::test(start_paused = true)]
    async fn a_step_that_cannot_be_cancelled_says_so_in_its_events_and_keeps_running() {
        let (sink, events) = collector();
        let jobs = jobs();
        let (finish, finished) = oneshot::channel::<()>();
        let id = start(&jobs, spec(Some("answer the system prompt")), sink, |_| async move {
            finished.await.ok();
            Ok(Outcome::Done(Value::Null))
        });
        settle().await;
        assert!(!jobs.cancel(id), "no error, just no effect");
        assert_eq!(jobs.running(), 1);
        assert!(matches!(
            &lock(&events)[1],
            JobEvent::Progress { cancellable: false, not_cancellable_reason: Some(r), .. } if r == "answer the system prompt"
        ));
        finish.send(()).unwrap();
        settle().await;
        assert_eq!(kinds(&events).last().map(String::as_str), Some("done"));
    }

    #[tokio::test(start_paused = true)]
    async fn two_jobs_each_report_on_their_own_channel() {
        let (sink_a, a) = collector();
        let (sink_b, b) = collector();
        let jobs = jobs();
        let ida = start(&jobs, spec(None), sink_a, |_| async { Ok(Outcome::Done(Value::Null)) });
        let idb = start(&jobs, spec(None), sink_b, |_| async { Ok(Outcome::Done(Value::Null)) });
        settle().await;
        assert_ne!(ida, idb);
        assert!(lock(&a).iter().all(|e| !format!("{e:?}").contains(&format!("job_id: {idb}"))));
        assert!(lock(&b).iter().all(|e| !format!("{e:?}").contains(&format!("job_id: {ida}"))));
        assert_eq!(kinds(&a).last().map(String::as_str), Some("done"));
        assert_eq!(kinds(&b).last().map(String::as_str), Some("done"));
    }

    #[test]
    fn events_have_the_camel_case_wire_shape() {
        let v = serde_json::to_value(JobEvent::Progress {
            job_id: 7,
            phase: "x".into(),
            done: 1,
            total: None,
            elapsed_secs: 2,
            cancellable: false,
            not_cancellable_reason: Some("why".into()),
        })
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({ "kind": "progress", "jobId": 7, "phase": "x", "done": 1, "total": null, "elapsedSecs": 2, "cancellable": false, "notCancellableReason": "why" })
        );
    }
}
