//! Spotter's long operations on the shell's job registry (`paddock-jobs`, ADR 0008): the engine keeps reporting through
//! `spotter_core::job::Job` (pure, no app types); this module registers each operation as a shell job, forwards the
//! engine's phase reports to it and lets the shell's Cancel (the Jobs list, `job_cancel`) stop the engine's work.
//! The shell owns ids, events and the end state; there is no second job channel.

use paddock_download::Downloader;
use paddock_jobs::{CancelReason, JobContext, JobRegistry, JobState, Phase as ShellPhase};
use serde::Serialize;
use spotter_core::job::{Cancel, Job, Phase, Progress, Unit};
use spotter_core::release::Release;
use spotter_sources::download::{Downloaded, Fetch};
use spotter_sources::http::Http;
use std::sync::Arc;

/// What a job is, as the shell's `kind` slug (the UI picks icons and grouping from it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Kind {
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

impl Kind {
    /// The shell's slug.
    pub fn slug(self) -> &'static str {
        match self {
            Kind::Download => "sp.download",
            Kind::InstallRenderer => "sp.install-renderer",
            Kind::Check => "sp.check",
            Kind::CheckAll => "sp.check-all",
            Kind::TestSource => "sp.test-source",
        }
    }
}

/// A registered operation: what to run it with, and how to end it.
pub struct Operation {
    ctx: JobContext,
    /// The handle the engine reports to and polls.
    pub job: Job,
}

/// The stable slug of a phase (tests and translations).
fn slug(phase: Phase) -> &'static str {
    match phase {
        Phase::FetchingChecksum => "checksum",
        Phase::CheckingExisting => "existing",
        Phase::Downloading => "download",
        Phase::Copying => "copy",
        Phase::Unpacking => "unpack",
        Phase::SelfTest => "test",
        Phase::Finding => "find",
        Phase::Settling => "settle",
        Phase::Checking => "check",
    }
}

/// The shell's phase for an engine report.
pub fn shell_phase(p: &Progress) -> ShellPhase {
    match (p.unit, p.total) {
        (Unit::Bytes, total) => ShellPhase::bytes(slug(p.phase), p.label, total),
        (Unit::Count, Some(total)) => ShellPhase::items(slug(p.phase), p.label, total),
        (Unit::Count, None) => ShellPhase::discovering(slug(p.phase), p.label),
    }
}

/// Registers an operation (queued, then running) and builds the engine's `Job` for it: its reports become the shell
/// job's phases and counts, and the shell's cancel token stops it.
pub fn begin(registry: &JobRegistry, kind: Kind, title: &str, subject: Option<&str>) -> Operation {
    let ctx = registry.register(kind.slug(), title, subject);
    ctx.reporter().start();
    let source = ctx.cancel_token().clone();
    let sink_ctx = ctx.clone();
    let state = std::sync::Mutex::new((None::<Phase>, None::<u64>));
    let job = Job::new(Cancel::linked(move || source.is_cancelled()), move |p| {
        let mut seen = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if seen.0 != Some(p.phase) {
            sink_ctx.phase(shell_phase(p));
            *seen = (Some(p.phase), p.total);
        } else if p.total != seen.1 {
            if let Some(total) = p.total {
                sink_ctx.set_total(total);
            }
            seen.1 = p.total;
        }
        sink_ctx.set(p.done);
    });
    Operation { ctx, job }
}

impl Operation {
    /// The shell job the network phases of `paddock-download` report to.
    pub fn ctx(&self) -> &JobContext {
        &self.ctx
    }

    /// The shell's id of the job.
    pub fn id(&self) -> paddock_jobs::JobId {
        self.ctx.id()
    }

    /// Ends the operation from its outcome: done, cancelled (the user asked) or failed with `message`.
    pub fn end(self, ok: bool, message: &str) {
        let cancelled = self.job.is_cancelled();
        self.end_as(ok, cancelled, message);
    }

    /// As [`end`](Self::end), with the caller saying whether the user cancelled (a download learns it from the shell's
    /// token inside `paddock-download`).
    pub fn end_as(self, ok: bool, cancelled: bool, message: &str) {
        self.ctx.reporter().flush();
        let state = if ok {
            JobState::Done
        } else if cancelled {
            JobState::Cancelled {
                reason: CancelReason::User,
            }
        } else {
            JobState::Failed {
                message: message.to_string(),
            }
        };
        self.ctx.reporter().finish(state);
    }
}

/// Downloads as shell jobs: the job registry, the app's one downloader (its per-host caps hold across every download, the
/// user's and the automatic ones) and the metadata client. Cheap to clone.
#[derive(Clone)]
pub struct Downloads {
    registry: Arc<JobRegistry>,
    downloader: Arc<Downloader>,
    http: Http,
}

impl Downloads {
    /// The download service over `registry` and `downloader`.
    pub fn new(registry: Arc<JobRegistry>, downloader: Arc<Downloader>, http: Http) -> Downloads {
        Downloads {
            registry,
            downloader,
            http,
        }
    }

    /// Downloads (or copies) `release` into `folder` as a job titled `title`, and ends the job from the outcome.
    pub async fn release(
        &self,
        title: &str,
        subject: Option<&str>,
        release: &Release,
        folder: &std::path::Path,
        verify: bool,
    ) -> Result<Downloaded, String> {
        let op = begin(&self.registry, Kind::Download, title, subject);
        let fetch = Fetch {
            downloader: &self.downloader,
            http: &self.http,
            ctx: op.ctx(),
        };
        let result = spotter_sources::download::download_release(&fetch, release, folder, verify, &op.job).await;
        let cancelled = op.job.is_cancelled() || result.as_ref().err().is_some_and(|e| e == spotter_core::job::CANCELLED);
        op.end_as(result.is_ok(), cancelled, result.as_ref().err().map_or("", |e| e.as_str()));
        result
    }
}

/// The shell's job registry of the app.
pub fn registry<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<Arc<JobRegistry>> {
    use tauri::Manager;
    app.try_state::<paddock_jobs::tauri::Jobs>().map(|j| j.registry().clone())
}

/// Asks every job that still runs to stop (Quit: the engine's work ends at its next cancel check).
pub fn cancel_all(registry: &JobRegistry) {
    for snapshot in registry.snapshots() {
        if !snapshot.state.is_terminal() {
            registry.cancel(snapshot.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_jobs::{EventSink, JobSnapshot};
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorder(Mutex<Vec<JobSnapshot>>);
    impl EventSink for Recorder {
        fn emit(&self, s: &JobSnapshot) {
            self.0.lock().unwrap().push(s.clone());
        }
    }

    fn registry_with_recorder() -> (JobRegistry, Arc<Recorder>) {
        let rec = Arc::new(Recorder::default());
        (
            JobRegistry::with_clock(rec.clone(), Arc::new(paddock_jobs::FakeClock::new()), 0),
            rec,
        )
    }

    #[test]
    fn the_engines_phases_become_the_shells_phases_on_their_own_scales() {
        let (registry, rec) = registry_with_recorder();
        let op = begin(&registry, Kind::Download, "Downloading", Some("Lumen 2.4"));
        op.job.report(Phase::Finding, Unit::Count, 3, None);
        op.job.report(Phase::Downloading, Unit::Bytes, 5, Some(10));
        let last = registry.snapshots().pop().unwrap();
        match last.state {
            JobState::Running { phase: Some(p) } => {
                assert_eq!((p.phase.id.as_str(), p.done, p.phase.total), ("download", 5, Some(10)));
            }
            other => panic!("{other:?}"),
        }
        op.end(true, "");
        assert!(registry.snapshots().pop().unwrap().state == JobState::Done);
        assert!(
            rec.0.lock().unwrap().len() >= 3,
            "queued, running, phases and the end were announced"
        );
    }

    #[test]
    fn the_shells_cancel_stops_the_engines_job_and_ends_it_cancelled() {
        let (registry, _) = registry_with_recorder();
        let op = begin(&registry, Kind::CheckAll, "Checking every watch", None);
        let id = registry.snapshots()[0].id;
        assert!(!op.job.is_cancelled());
        assert!(registry.cancel(id));
        assert!(op.job.is_cancelled(), "the engine sees the shell's cancel");
        assert!(op.job.muted().is_cancelled(), "so does every part of it");
        op.end(false, "Cancelled.");
        assert!(matches!(registry.snapshots()[0].state, JobState::Cancelled { .. }));
    }

    #[test]
    fn a_failure_carries_its_message_and_quit_cancels_whatever_runs() {
        let (registry, _) = registry_with_recorder();
        begin(&registry, Kind::Check, "a", None).end(false, "The share is offline.");
        assert!(matches!(&registry.snapshots()[0].state, JobState::Failed { message } if message == "The share is offline."));
        let running = begin(&registry, Kind::Check, "b", None);
        cancel_all(&registry);
        assert!(running.job.is_cancelled());
    }

    #[test]
    fn the_total_arriving_late_is_passed_on() {
        let (registry, _) = registry_with_recorder();
        let op = begin(&registry, Kind::Download, "d", None);
        let job = op.job.clone().with_interval(std::time::Duration::ZERO);
        job.report(Phase::Downloading, Unit::Bytes, 1, None);
        job.report(Phase::Downloading, Unit::Bytes, 2, Some(100));
        match registry.snapshots()[0].state.clone() {
            JobState::Running { phase: Some(p) } => assert_eq!(p.phase.total, Some(100)),
            other => panic!("{other:?}"),
        }
    }
}
