//! Long-running work: named phases, progress on a scale of their own, throttled events and cooperative cancel.
//!
//! A [`Job`] is handed to every operation that can take noticeable time (downloads, installs, folder scans,
//! check-all). The operation reports each phase with [`Job::report`] (the first report of a phase always goes out,
//! the rest at most ten times a second, [`Job::report_done`] is never dropped) and polls [`Job::is_cancelled`] between
//! chunks, directories and files. A phase whose total is not known reports `total: None`: the UI shows an
//! indeterminate bar and the growing count.
//!
//! The job is the one owner of its progress: within a phase `done` never goes back, however many threads report.
//! A report below what was already shown is dropped (an end report is raised to it), and the sink sees reports in
//! the order the job accepted them, so parallel workers that read a shared count and then report it cannot make the
//! bar tick backwards (Paddock rule 11).
//!
//! This module is deliberately small and self-contained so it can be replaced by the shared `paddock-jobs` crate.

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The message an operation ends with when the user cancelled it.
pub const CANCELLED: &str = "Cancelled.";

/// The shortest time between two throttled progress events (at most ten a second).
pub const MIN_INTERVAL: Duration = Duration::from_millis(100);

/// A flag the user (or a timeout) raises to stop an operation; clones share it.
#[derive(Clone, Default)]
pub struct Cancel {
    flag: Arc<AtomicBool>,
    /// Another owner of the stop: the app's job registry, which the user's Cancel reaches.
    source: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
}

impl std::fmt::Debug for Cancel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cancel").field("cancelled", &self.is_cancelled()).finish()
    }
}

impl Cancel {
    /// A fresh, not yet cancelled flag.
    pub fn new() -> Cancel {
        Cancel::default()
    }

    /// A flag that is also raised when `source` says so: the stop of a job another owner (the app shell's job
    /// registry) cancels. `source` is read on every poll, so it must be cheap.
    pub fn linked(source: impl Fn() -> bool + Send + Sync + 'static) -> Cancel {
        Cancel {
            flag: Arc::default(),
            source: Some(Arc::new(source)),
        }
    }

    /// Asks the operation to stop.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Relaxed);
    }

    /// Whether a stop was asked for.
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Relaxed) || self.source.as_ref().is_some_and(|s| s())
    }
}

/// What an operation is doing now. Every phase has its own scale (see [`Unit`]); a bar never restarts under a label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// Looking for the publisher's checksum file.
    FetchingChecksum,
    /// Reading the part of a download that is already on disk, to carry its checksum on.
    CheckingExisting,
    /// Receiving a file from the network.
    Downloading,
    /// Copying a file from a drive or share.
    Copying,
    /// Extracting an archive.
    Unpacking,
    /// Starting what was installed, to see that it works.
    SelfTest,
    /// Listing a folder: the count of entries seen so far grows.
    Finding,
    /// Waiting for new files to stop changing.
    Settling,
    /// Checking watches: `done` of `total`.
    Checking,
}

impl Phase {
    /// The words the UI shows.
    pub fn label(self) -> &'static str {
        match self {
            Phase::FetchingChecksum => "Fetching the checksum",
            Phase::CheckingExisting => "Checking the part already downloaded",
            Phase::Downloading => "Downloading",
            Phase::Copying => "Copying",
            Phase::Unpacking => "Unpacking",
            Phase::SelfTest => "Testing",
            Phase::Finding => "Finding files",
            Phase::Settling => "Waiting for files to settle",
            Phase::Checking => "Checking",
        }
    }
}

/// What `done` and `total` count in a phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Unit {
    /// Bytes.
    Bytes,
    /// Items (entries, files, watches, seconds).
    Count,
}

/// One progress report: a phase and where it stands on its own scale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// The phase.
    pub phase: Phase,
    /// The phase's words.
    pub label: &'static str,
    /// What the numbers count.
    pub unit: Unit,
    /// How far the phase is.
    pub done: u64,
    /// How far it goes, when known; `None` is an indeterminate phase with a growing `done`.
    pub total: Option<u64>,
}

type Sink = Arc<dyn Fn(&Progress) + Send + Sync>;

#[derive(Default)]
struct Gate {
    last: Option<Instant>,
    phase: Option<Phase>,
    /// The highest `done` shown in `phase`.
    done: u64,
}

/// The handle an operation reports to and polls: progress sink plus cancel flag. Clones share both.
#[derive(Clone)]
pub struct Job {
    cancel: Cancel,
    sink: Option<Sink>,
    gate: Arc<Mutex<Gate>>,
    interval: Duration,
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Job").field("cancelled", &self.cancel.is_cancelled()).finish()
    }
}

impl Job {
    /// A job nobody watches and nobody cancels (what plain, non-interactive callers pass).
    pub fn none() -> Job {
        Job {
            cancel: Cancel::new(),
            sink: None,
            gate: Arc::default(),
            interval: MIN_INTERVAL,
        }
    }

    /// A job reporting to `sink` and stopped by `cancel`.
    pub fn new(cancel: Cancel, sink: impl Fn(&Progress) + Send + Sync + 'static) -> Job {
        Job {
            cancel,
            sink: Some(Arc::new(sink)),
            ..Job::none()
        }
    }

    /// A job that stops with this one but reports nowhere: for the parts of a bigger job whose own progress would
    /// only fight over the one display (the checks inside "check everything").
    pub fn muted(&self) -> Job {
        Job {
            cancel: self.cancel.clone(),
            sink: None,
            gate: Arc::default(),
            interval: self.interval,
        }
    }

    /// The same job with another shortest interval between throttled reports (tests use zero).
    #[must_use]
    pub fn with_interval(mut self, interval: Duration) -> Job {
        self.interval = interval;
        self
    }

    /// The cancel flag.
    pub fn cancel_flag(&self) -> &Cancel {
        &self.cancel
    }

    /// Whether the user asked to stop.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Reports progress, throttled: the first report of a phase goes out, later ones at most every interval.
    pub fn report(&self, phase: Phase, unit: Unit, done: u64, total: Option<u64>) {
        self.emit(phase, unit, done, total, false);
    }

    /// Reports the end state of a phase; never dropped by the throttle.
    pub fn report_done(&self, phase: Phase, unit: Unit, done: u64, total: Option<u64>) {
        self.emit(phase, unit, done, total, true);
    }

    /// The sink is called with the gate held, so the order it sees is the order the gate accepted (a sink must not
    /// report to the same job).
    fn emit(&self, phase: Phase, unit: Unit, done: u64, total: Option<u64>, force: bool) {
        let Some(sink) = &self.sink else { return };
        let mut gate = self.gate.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        let fresh = gate.phase != Some(phase);
        let (mut done, mut total) = (done, total);
        if !fresh && done < gate.done {
            if !force {
                // Read before a higher count that already went out: showing it would move the bar back.
                return;
            }
            done = gate.done;
            total = total.map(|t| t.max(done));
        }
        let due = gate.last.is_none_or(|t| now.duration_since(t) >= self.interval);
        if !(force || fresh || due) {
            return;
        }
        gate.phase = Some(phase);
        gate.last = Some(now);
        gate.done = done;
        sink(&Progress {
            phase,
            label: phase.label(),
            unit,
            done,
            total,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collecting(interval: Duration) -> (Job, Cancel, Arc<Mutex<Vec<Progress>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let cancel = Cancel::new();
        let sink = seen.clone();
        let job = Job::new(cancel.clone(), move |p| sink.lock().unwrap().push(p.clone())).with_interval(interval);
        (job, cancel, seen)
    }

    #[test]
    fn a_linked_cancel_follows_its_source_and_its_own_flag() {
        let raised = Arc::new(AtomicBool::new(false));
        let source = raised.clone();
        let c = Cancel::linked(move || source.load(Ordering::Relaxed));
        let d = c.clone();
        assert!(!d.is_cancelled());
        raised.store(true, Ordering::Relaxed);
        assert!(c.is_cancelled() && d.is_cancelled(), "the registry's cancel reaches every clone");
        let own = Cancel::linked(|| false);
        own.cancel();
        assert!(own.is_cancelled(), "the flag still works on its own");
    }

    #[test]
    fn cancel_is_shared_between_clones() {
        let c = Cancel::new();
        let d = c.clone();
        assert!(!d.is_cancelled());
        c.cancel();
        assert!(d.is_cancelled());
    }

    #[test]
    fn a_job_without_a_sink_ignores_reports_and_is_never_cancelled() {
        let job = Job::none();
        job.report(Phase::Downloading, Unit::Bytes, 1, None);
        assert!(!job.is_cancelled());
        assert!(format!("{job:?}").contains("Job"));
    }

    #[test]
    fn reports_are_throttled_but_phase_changes_and_the_end_always_go_out() {
        let (job, _, seen) = collecting(Duration::from_secs(3600));
        for i in 0..1000 {
            job.report(Phase::Finding, Unit::Count, i, None);
        }
        assert_eq!(seen.lock().unwrap().len(), 1, "one per interval");
        job.report(Phase::Settling, Unit::Count, 0, Some(5));
        job.report_done(Phase::Settling, Unit::Count, 5, Some(5));
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[1].label, "Waiting for files to settle");
        assert_eq!(seen[2].done, 5);
    }

    /// Regression (Windows CI, `the_found_count_is_live_from_the_start_and_only_grows`): parallel folder walkers each
    /// read the shared count and then report it, so a walker that read 100 can report after one that read 200. The
    /// job is the one owner of its progress: within a phase a lower count never goes out after a higher one.
    #[test]
    fn a_count_read_before_a_higher_one_but_reported_after_it_is_never_shown() {
        let (job, _, seen) = collecting(Duration::ZERO);
        // Walker A reads 100, walker B reads 200, B reports first, then A.
        job.report(Phase::Finding, Unit::Count, 0, None);
        job.report(Phase::Finding, Unit::Count, 200, None);
        job.report(Phase::Finding, Unit::Count, 100, None);
        job.report(Phase::Finding, Unit::Count, 300, None);
        let done: Vec<u64> = seen.lock().unwrap().iter().map(|p| p.done).collect();
        assert_eq!(done, [0, 200, 300]);
    }

    #[test]
    fn the_end_of_a_phase_always_goes_out_and_never_below_what_was_shown() {
        let (job, _, seen) = collecting(Duration::from_secs(3600));
        job.report(Phase::Finding, Unit::Count, 500, None);
        job.report_done(Phase::Finding, Unit::Count, 400, Some(400));
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(
            (seen[1].done, seen[1].total),
            (500, Some(500)),
            "the end is clamped, with its total"
        );
    }

    #[test]
    fn every_phase_starts_its_own_scale() {
        let (job, _, seen) = collecting(Duration::ZERO);
        job.report(Phase::Downloading, Unit::Bytes, 9000, None);
        job.report(Phase::Unpacking, Unit::Bytes, 10, Some(100));
        job.report(Phase::Downloading, Unit::Bytes, 5, None);
        let done: Vec<u64> = seen.lock().unwrap().iter().map(|p| p.done).collect();
        assert_eq!(done, [9000, 10, 5]);
    }

    /// Many threads counting into one atomic and reporting what they read: the shown counts only grow, because the
    /// order the sink sees is the order the job accepted them in.
    #[test]
    fn concurrent_reporters_never_make_the_count_go_back() {
        use std::sync::atomic::AtomicU64;
        let (job, _, seen) = collecting(Duration::ZERO);
        let count = AtomicU64::new(0);
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| {
                    for _ in 0..2000 {
                        let n = count.fetch_add(1, Ordering::Relaxed) + 1;
                        job.report(Phase::Finding, Unit::Count, n, None);
                    }
                });
            }
        });
        let seen = seen.lock().unwrap();
        assert!(seen.windows(2).all(|w| w[0].done <= w[1].done), "the count went back");
        assert_eq!(seen.last().map(|p| p.done), Some(16_000));
    }

    #[test]
    fn unthrottled_jobs_report_every_step_and_cancel_is_visible() {
        let (job, cancel, seen) = collecting(Duration::ZERO);
        for i in 0..5 {
            job.report(Phase::Downloading, Unit::Bytes, i, Some(5));
        }
        assert_eq!(seen.lock().unwrap().len(), 5);
        cancel.cancel();
        assert!(job.is_cancelled() && job.cancel_flag().is_cancelled());
    }

    #[test]
    fn progress_serialises_with_camel_case_names() {
        let p = Progress {
            phase: Phase::CheckingExisting,
            label: Phase::CheckingExisting.label(),
            unit: Unit::Bytes,
            done: 1,
            total: None,
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(
            json.contains(r#""phase":"checkingExisting""#) && json.contains(r#""total":null"#),
            "{json}"
        );
    }

    #[test]
    fn a_muted_job_stops_with_its_parent_but_stays_silent() {
        let (job, cancel, seen) = collecting(Duration::ZERO);
        let quiet = job.muted();
        quiet.report(Phase::Finding, Unit::Count, 1, None);
        assert!(seen.lock().unwrap().is_empty());
        cancel.cancel();
        assert!(quiet.is_cancelled());
    }

    #[test]
    fn every_phase_has_a_label() {
        for p in [
            Phase::FetchingChecksum,
            Phase::CheckingExisting,
            Phase::Downloading,
            Phase::Copying,
            Phase::Unpacking,
            Phase::SelfTest,
            Phase::Finding,
            Phase::Settling,
            Phase::Checking,
        ] {
            assert!(!p.label().is_empty());
        }
    }
}
