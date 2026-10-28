//! The service's own long jobs (runner downloads) in the shell's Jobs list. The service pushes every snapshot as
//! `ServiceEvent::Job`; each one is mirrored onto a job of the app's registry, so the list, "Still working" and Cancel are the
//! shell's, and Cancel reaches the service as `Request::CancelJob` in every phase (rules 10-12). Nothing here times a job out.

use crate::commands::SharedClient;
use paddock_jobs::{JobContext, JobRegistry, JobSnapshot, JobState};
use pitcrew_core::protocol::Request;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How often a mirrored job looks for Cancel.
const CANCEL_POLL: Duration = Duration::from_millis(150);

struct Mirrored {
    ctx: JobContext,
    started: bool,
    phase: Option<String>,
    /// Stops the cancel watcher.
    done: Arc<AtomicBool>,
}

/// The mirrored service jobs, by the service's job id (managed state).
#[derive(Default)]
pub struct Mirror {
    jobs: Mutex<HashMap<u64, Mirrored>>,
}

impl Mirror {
    /// Shows `snap` (a snapshot from the service) on the app's registry: a job is registered at its first snapshot, follows
    /// its phases and counts, ends as the service says, and asks the service to cancel when the user cancels it.
    pub fn apply(&self, registry: &Arc<JobRegistry>, client: &SharedClient, snap: &JobSnapshot) {
        let mut jobs = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
        let entry = jobs.entry(snap.id.0).or_insert_with(|| {
            let ctx = registry.register(&snap.kind.0, &snap.title, snap.subject.as_deref());
            let done = Arc::new(AtomicBool::new(false));
            watch_cancel(ctx.clone(), snap.id.0, client.clone(), done.clone());
            Mirrored {
                ctx,
                started: false,
                phase: None,
                done,
            }
        });
        match &snap.state {
            JobState::Queued => {}
            JobState::Running { phase } => {
                if !entry.started {
                    entry.started = true;
                    entry.ctx.reporter().start();
                }
                if let Some(p) = phase {
                    if entry.phase.as_deref() != Some(p.phase.id.as_str()) {
                        entry.phase = Some(p.phase.id.clone());
                        entry.ctx.phase(p.phase.clone());
                    } else if let Some(total) = p.phase.total {
                        entry.ctx.set_total(total);
                    }
                    entry.ctx.set(p.done);
                }
            }
            end => {
                if !entry.started {
                    entry.ctx.reporter().start();
                }
                entry.ctx.reporter().flush();
                entry.ctx.reporter().finish(end.clone());
                entry.done.store(true, Ordering::SeqCst);
                jobs.remove(&snap.id.0);
            }
        }
    }

    /// How many service jobs are mirrored now (tests).
    pub fn len(&self) -> usize {
        self.jobs.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    /// Whether no service job is mirrored.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Tells the service to cancel when the user cancels the mirrored job (once), until the job ends.
fn watch_cancel(ctx: JobContext, service_id: u64, client: SharedClient, done: Arc<AtomicBool>) {
    let spawned = std::thread::Builder::new().name("pitcrew-job-cancel".into()).spawn(move || {
        while !done.load(Ordering::SeqCst) {
            if ctx.cancel_token().is_cancelled() {
                let _ = client.request(Request::CancelJob { id: service_id });
                return;
            }
            std::thread::sleep(CANCEL_POLL);
        }
    });
    // Without the watcher the job still ends as the service says; only the user's Cancel would not arrive.
    drop(spawned);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service_client::{ClientError, ServiceClient};
    use paddock_jobs::{EventSink, JobId, JobKind, Phase, PhaseProgress};
    use pitcrew_core::protocol::Response;

    #[derive(Default)]
    struct Sink(Mutex<Vec<JobSnapshot>>);
    impl EventSink for Sink {
        fn emit(&self, s: &JobSnapshot) {
            self.0.lock().unwrap().push(s.clone());
        }
    }

    #[derive(Default)]
    struct Client(Mutex<Vec<Request>>);
    impl ServiceClient for Client {
        fn request(&self, r: Request) -> Result<Response, ClientError> {
            self.0.lock().unwrap().push(r);
            Ok(Response::Ok)
        }
    }

    fn snap(id: u64, state: JobState) -> JobSnapshot {
        JobSnapshot {
            id: JobId(id),
            kind: JobKind::new("pitcrew.download"),
            title: "Downloading the runner".into(),
            subject: Some("Gitea runner 5.2.0".into()),
            seq: 1,
            elapsed_ms: 0,
            eta_ms: None,
            cancellable: true,
            not_cancellable_reason: None,
            state,
        }
    }

    fn running(phase: &str, done: u64) -> JobState {
        JobState::Running {
            phase: Some(PhaseProgress {
                phase: Phase::bytes(phase, phase, Some(1000)),
                done,
            }),
        }
    }

    fn setup() -> (Arc<Sink>, Arc<JobRegistry>, Arc<Client>, SharedClient, Mirror) {
        let sink = Arc::new(Sink::default());
        let registry = Arc::new(JobRegistry::new(sink.clone()));
        let client = Arc::new(Client::default());
        let shared: SharedClient = client.clone();
        (sink, registry, client, shared, Mirror::default())
    }

    #[test]
    fn a_service_job_follows_its_phases_and_ends_as_the_service_says() {
        let (sink, registry, _client, shared, mirror) = setup();
        mirror.apply(&registry, &shared, &snap(7, JobState::Queued));
        mirror.apply(&registry, &shared, &snap(7, running("download", 100)));
        mirror.apply(&registry, &shared, &snap(7, running("download", 600)));
        mirror.apply(&registry, &shared, &snap(7, running("verify", 0)));
        assert_eq!(mirror.len(), 1);
        mirror.apply(&registry, &shared, &snap(7, JobState::Done));
        assert!(mirror.is_empty(), "an ended job is forgotten");
        let seen = sink.0.lock().unwrap();
        assert!(seen
            .iter()
            .all(|s| s.kind.0 == "pitcrew.download" && s.title == "Downloading the runner"));
        let phases: Vec<String> = seen
            .iter()
            .filter_map(|s| match &s.state {
                JobState::Running { phase: Some(p) } => Some(p.phase.id.clone()),
                _ => None,
            })
            .fold(Vec::new(), |mut a, id| {
                if a.last() != Some(&id) {
                    a.push(id);
                }
                a
            });
        assert_eq!(phases, ["download", "verify"]);
        assert!(matches!(seen.last().unwrap().state, JobState::Done));
    }

    #[test]
    fn cancel_in_the_jobs_list_reaches_the_service_in_every_phase() {
        let (sink, registry, client, shared, mirror) = setup();
        mirror.apply(&registry, &shared, &snap(9, running("download", 10)));
        let app_id = sink.0.lock().unwrap().last().unwrap().id;
        assert!(registry.cancel(app_id), "the shell's job_cancel");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while client.0.lock().unwrap().is_empty() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(*client.0.lock().unwrap(), [Request::CancelJob { id: 9 }]);
        // The service then reports the job cancelled; the mirror ends it the same way.
        mirror.apply(
            &registry,
            &shared,
            &snap(
                9,
                JobState::Cancelled {
                    reason: paddock_jobs::CancelReason::User,
                },
            ),
        );
        assert!(mirror.is_empty());
    }

    #[test]
    fn a_failed_download_ends_failed_with_the_services_words() {
        let (sink, registry, _client, shared, mirror) = setup();
        mirror.apply(&registry, &shared, &snap(3, running("connect", 0)));
        mirror.apply(
            &registry,
            &shared,
            &snap(
                3,
                JobState::Failed {
                    message: "the disk is full".into(),
                },
            ),
        );
        let last = sink.0.lock().unwrap().last().unwrap().clone();
        assert!(
            matches!(last.state, JobState::Failed { ref message } if message == "the disk is full"),
            "{last:?}"
        );
    }
}
