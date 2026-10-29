//! Runs the fetch work as `paddock-jobs` jobs on a small pool: the command thread only waits, every transfer and unpack
//! runs on a worker, progress reaches standard error as phase-labelled lines (ER-17, ER-18), and a cancel (Ctrl+C) stops
//! the work in every phase (ER-19: nothing here ever times a running job out).

use super::error::FetchError;
use paddock_cli::{Progress, format_size};
use paddock_download::{Downloader, Transport};
use paddock_jobs::{EventSink, JobContext, JobError, JobId, JobPool, JobRegistry, JobSnapshot, JobState, Unit, Workers};
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// The largest number of archives fetched at once (ER-20: network work, a few at a time; the downloader's own per-host cap of
/// 4 connections holds across all of them).
pub const MAX_JOBS: usize = 8;
/// The default: three archives at once. The pinned archives sit on one or two hosts, each download already uses up to four
/// connections, and the unpack is disk-bound, so more only competes for the same link and disk.
pub const DEFAULT_JOBS: usize = 3;

/// Runs jobs for one command.
pub struct Runner {
    registry: Arc<JobRegistry>,
    pool: JobPool,
    downloader: Arc<Downloader>,
    jobs: usize,
    cancelled: Arc<AtomicBool>,
}

impl Runner {
    /// A runner over `transport` that reports to `sink`; at most `jobs` (1 to [`MAX_JOBS`]) jobs run at once.
    pub fn new(transport: Arc<dyn Transport>, sink: Arc<dyn EventSink>, jobs: usize) -> Runner {
        let jobs = jobs.clamp(1, MAX_JOBS);
        Runner {
            registry: Arc::new(JobRegistry::new(sink)),
            pool: JobPool::new(Workers::new(jobs)),
            downloader: Arc::new(Downloader::new(transport)),
            jobs,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// A runner on the real network (the operating system's proxy and certificate store) writing progress to standard error.
    pub fn http(jobs: usize) -> Result<Runner, FetchError> {
        let downloader = Downloader::http(paddock_download::HttpConfig::default())
            .map_err(|e| FetchError::failed(format!("The network could not be set up: {e}")))?;
        let sink = Arc::new(LineSink::new(std::io::stderr(), true));
        let runner = Runner::new(downloader.transport().clone(), sink, jobs);
        Ok(runner)
    }

    /// The downloader the jobs use.
    pub fn downloader(&self) -> &Arc<Downloader> {
        &self.downloader
    }

    /// How many jobs run at once.
    pub fn jobs(&self) -> usize {
        self.jobs
    }

    /// Starts a job and returns at once.
    pub fn spawn<T: Send + 'static>(
        &self,
        kind: &str,
        title: &str,
        subject: Option<&str>,
        body: impl FnOnce(&JobContext) -> Result<T, JobError> + Send + 'static,
    ) -> Result<Pending<T>, FetchError> {
        if self.cancelled.load(Ordering::SeqCst) {
            return Err(FetchError::Cancelled);
        }
        let ctx = self.registry.register(kind, title, subject);
        let id = ctx.id();
        let handle = self
            .pool
            .spawn_job(ctx, body)
            .map_err(|_| FetchError::failed("The job pool is closed."))?;
        Ok(Pending { id, handle })
    }

    /// Runs one job and waits for it.
    pub fn run<T: Send + 'static>(
        &self,
        kind: &str,
        title: &str,
        subject: Option<&str>,
        body: impl FnOnce(&JobContext) -> Result<T, JobError> + Send + 'static,
    ) -> Result<T, FetchError> {
        self.spawn(kind, title, subject, body)?.wait()
    }

    /// Asks every job to stop and refuses new ones from now on. Safe to call from a signal-handling thread.
    pub fn cancel_all(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        for s in self.registry.snapshots() {
            self.registry.cancel(s.id);
        }
    }

    /// What Ctrl+C does for this runner.
    pub fn interrupt(&self) -> Arc<Interrupt> {
        Arc::new(Interrupt {
            registry: self.registry.clone(),
            cancelled: self.cancelled.clone(),
            hits: AtomicUsize::new(0),
        })
    }
}

/// Ctrl+C: the first press cancels every job and refuses new ones, a second press ends the process with 130.
pub struct Interrupt {
    registry: Arc<JobRegistry>,
    cancelled: Arc<AtomicBool>,
    hits: AtomicUsize,
}

impl Interrupt {
    /// One press. `exit` ends the process (injected so a test can watch it).
    pub fn press(&self, exit: &dyn Fn(i32)) {
        if self.hits.fetch_add(1, Ordering::SeqCst) == 0 {
            self.cancelled.store(true, Ordering::SeqCst);
            for s in self.registry.snapshots() {
                self.registry.cancel(s.id);
            }
        } else {
            exit(130);
        }
    }
}

/// Removes a throwaway download: the file, its `.part` and the resume record.
pub fn discard(request: &paddock_download::Request) {
    for p in [request.dest().to_path_buf(), request.part_path(), request.resume_path()] {
        let _ = std::fs::remove_file(p);
    }
}

/// A started job.
pub struct Pending<T> {
    id: JobId,
    handle: paddock_jobs::JobHandle<T>,
}

impl<T> Pending<T> {
    /// The job's id.
    pub fn id(&self) -> JobId {
        self.id
    }

    /// Waits for the job and returns what it returned.
    pub fn wait(self) -> Result<T, FetchError> {
        self.handle.join().map_err(FetchError::from)
    }
}

/// Phase lines on a writer ("Downloading 12.3 MB of 195 MB (chromium)"), throttled and de-duplicated by `Progress`.
pub struct LineSink<W: Write + Send> {
    start: Instant,
    progress: Mutex<Progress<W>>,
}

impl<W: Write + Send> LineSink<W> {
    /// Lines to `out`; `enabled` is false for a silent run.
    pub fn new(out: W, enabled: bool) -> LineSink<W> {
        LineSink {
            start: Instant::now(),
            progress: Mutex::new(Progress::new(out, enabled)),
        }
    }

    /// Gives the writer back (tests read what was written).
    pub fn into_inner(self) -> W {
        self.progress.into_inner().unwrap_or_else(|p| p.into_inner()).into_inner()
    }
}

impl<W: Write + Send> EventSink for LineSink<W> {
    fn emit(&self, snapshot: &JobSnapshot) {
        let mut progress = self.progress.lock().unwrap_or_else(|p| p.into_inner());
        match describe(snapshot) {
            Line::Progress(text) => {
                progress.update(self.start.elapsed(), &text);
            }
            Line::Note(text) => progress.note(&text),
            Line::None => {}
        }
    }
}

/// What a snapshot shows.
#[derive(Debug, PartialEq, Eq)]
pub enum Line {
    /// A progress line, throttled.
    Progress(String),
    /// A line that is always shown.
    Note(String),
    /// Nothing to show.
    None,
}

/// The line for a snapshot: the phase label, how far it is on its own scale (a growing count when the total is unknown, never
/// a static 0), and the subject.
pub fn describe(s: &JobSnapshot) -> Line {
    let subject = s.subject.as_deref().map(|x| format!(" ({x})")).unwrap_or_default();
    match &s.state {
        JobState::Queued => Line::None,
        JobState::Running { phase: None } => Line::Progress(format!("Starting{subject}")),
        JobState::Running { phase: Some(p) } => {
            let count = |n: u64| match p.phase.unit {
                Unit::Bytes => format_size(n),
                Unit::Items => n.to_string(),
            };
            let how = match p.phase.total {
                Some(t) => format!("{} of {}", count(p.done), count(t)),
                None => format!("{} so far", count(p.done)),
            };
            Line::Progress(format!("{} {how}{subject}", p.phase.label))
        }
        JobState::Done => Line::Note(format!("Finished: {}{subject}", s.title)),
        JobState::Failed { .. } => Line::None,
        JobState::Cancelled { .. } => Line::Note(format!("Cancelled: {}{subject}", s.title)),
    }
}

/// Installs the Ctrl+C handler of `runner`: the first press cancels, the second exits with 130. Installed once per process; a
/// failed installation is ignored, as a missing handler only means Ctrl+C ends the process the plain way.
pub fn cancel_on_ctrl_c(runner: &Runner) {
    let interrupt = runner.interrupt();
    let _ = ctrlc::set_handler(move || interrupt.press(&|code| std::process::exit(code)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_jobs::{CancelReason, Phase, PhaseProgress};

    fn snap(state: JobState) -> JobSnapshot {
        JobSnapshot {
            id: JobId(1),
            kind: paddock_jobs::JobKind::new("fetch"),
            title: "Fetching".into(),
            subject: Some("chromium".into()),
            state,
            seq: 1,
            elapsed_ms: 0,
            eta_ms: None,
            cancellable: true,
            not_cancellable_reason: None,
        }
    }

    fn running(phase: Phase, done: u64) -> JobSnapshot {
        snap(JobState::Running {
            phase: Some(PhaseProgress { phase, done }),
        })
    }

    #[test]
    fn lines_name_the_phase_the_scale_and_the_subject() {
        let bytes = describe(&running(Phase::bytes("download", "Downloading", Some(2048)), 1024));
        assert_eq!(bytes, Line::Progress("Downloading 1.0 KB of 2.0 KB (chromium)".into()));
        let unknown = describe(&running(Phase::bytes("download", "Downloading", None), 3));
        assert_eq!(unknown, Line::Progress("Downloading 3 B so far (chromium)".into()));
        let items = describe(&running(Phase::items("verify", "Verifying", 4), 2));
        assert_eq!(items, Line::Progress("Verifying 2 of 4 (chromium)".into()));
        assert_eq!(
            describe(&snap(JobState::Running { phase: None })),
            Line::Progress("Starting (chromium)".into())
        );
        assert_eq!(describe(&snap(JobState::Queued)), Line::None);
        assert_eq!(describe(&snap(JobState::Done)), Line::Note("Finished: Fetching (chromium)".into()));
        assert_eq!(describe(&snap(JobState::Failed { message: "x".into() })), Line::None);
        let c = describe(&snap(JobState::Cancelled {
            reason: CancelReason::User,
        }));
        assert_eq!(c, Line::Note("Cancelled: Fetching (chromium)".into()));
        let mut bare = snap(JobState::Done);
        bare.subject = None;
        assert_eq!(describe(&bare), Line::Note("Finished: Fetching".into()));
    }

    #[test]
    fn jobs_are_bounded_run_in_order_of_the_pool_and_report_failures() {
        let net = super::super::testing::FakeNet::new();
        let r = super::super::testing::runner(&net, 99);
        assert_eq!(r.jobs(), MAX_JOBS);
        assert_eq!(super::super::testing::runner(&net, 0).jobs(), 1);
        let pending = r.spawn("t", "Title", None, |_| Ok(7)).unwrap();
        let id = pending.id();
        assert_eq!(pending.wait().unwrap(), 7);
        assert_eq!(r.registry.snapshots().iter().filter(|s| s.id == id).count(), 1);
        let err = r
            .run::<()>("t", "Title", None, |_| Err(JobError::Failed("no disk".into())))
            .unwrap_err();
        assert_eq!(err, FetchError::failed("no disk."));
        r.cancel_all();
        assert!(r.downloader().config().connections >= 1);
    }

    #[test]
    fn a_queued_job_cancelled_before_it_starts_never_runs() {
        let net = super::super::testing::FakeNet::new();
        let r = super::super::testing::runner(&net, 1);
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let hold = r.pool.hold();
        let flag = ran.clone();
        let pending = r
            .spawn("t", "Title", None, move |_| {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            })
            .unwrap();
        r.cancel_all();
        drop(hold);
        assert_eq!(pending.wait().unwrap_err(), FetchError::Cancelled);
        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn the_first_press_cancels_and_refuses_new_jobs_and_the_second_exits_130() {
        let net = super::super::testing::FakeNet::new();
        let r = super::super::testing::runner(&net, 1);
        let interrupt = r.interrupt();
        let exited = std::sync::Mutex::new(Vec::new());
        let exit = |code: i32| exited.lock().unwrap().push(code);
        interrupt.press(&exit);
        assert!(exited.lock().unwrap().is_empty(), "the first press only cancels");
        assert_eq!(r.spawn("t", "Title", None, |_| Ok(())).err(), Some(FetchError::Cancelled));
        interrupt.press(&exit);
        interrupt.press(&exit);
        assert_eq!(*exited.lock().unwrap(), [130, 130]);
    }

    #[test]
    fn the_real_network_runner_is_built_without_connecting() {
        let r = Runner::http(2).unwrap();
        assert_eq!(r.jobs(), 2);
    }

    #[test]
    fn the_sink_writes_lines_and_notes_in_order() {
        let sink = LineSink::new(Vec::new(), true);
        sink.emit(&running(Phase::items("connect", "Connecting", 1), 0));
        sink.emit(&snap(JobState::Queued));
        sink.emit(&snap(JobState::Done));
        let text = String::from_utf8(sink.into_inner()).unwrap();
        assert_eq!(text, "Connecting 0 of 1 (chromium)\nFinished: Fetching (chromium)\n");
    }
}
