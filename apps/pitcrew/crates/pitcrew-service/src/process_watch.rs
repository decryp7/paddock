//! What the service knows about one runner's processes: the latest classified tree, the "keep longer" extensions
//! and the stops in progress (ADR 0012).
//!
//! The decisions are `pitcrew_core::process_track`'s; this holds the state between ticks and carries out the
//! operating-system side through a [`Terminator`] and an `alive` check, both passed in so the tests need no real
//! process.

use crate::terminate::{Asked, Terminator};
use paddock_proc::{Ending, ProcessId, StartTime};
use pitcrew_core::history::StopReason;
use pitcrew_core::jobs::FinishedJob;
use pitcrew_core::process_policy::{KeepFor, ProcessPolicy};
use pitcrew_core::process_track::{
    self, check_stop_target, classify, due, entries, export_extensions, extend, import_extensions, leftover_count, next_step,
    prune_extensions, rule_label, same_start, Extensions, JobSpan, KeepRecord, ProcessMeta, Refusal, StopProgress, StopStep, Tracked,
};
use pitcrew_core::protocol::ProcessList;
use pitcrew_core::time::Timestamp;
use std::time::Duration;

/// What a look at a stopping process found (rule 53: the OS names are reused, so "still there" is not "still it").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    /// Nothing runs under the pid (or it is a zombie, or another process now holds it).
    Gone,
    /// A process runs under the pid and its start time matches the one recorded: it is the same process.
    Same,
    /// A process runs under the pid but its start time is unknown (not readable now, or never recorded), so it cannot be
    /// told from a process that took the pid over: it is waited for, never ended by pid.
    Unverified,
}

/// Most stops in progress per runner at once (a cleanup after a big job can have many leftovers).
pub const MAX_STOPPING: usize = 64;

/// A stop that was asked for.
/// Why a stop was not begun.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BeginError {
    /// [`MAX_STOPPING`] stops are in progress already (try again in a moment).
    TooMany,
    /// The process may not be stopped: it has no start time to verify, its pid was handed on, or the system refused. The
    /// text says what to do next.
    Refused(String),
}

impl std::fmt::Display for BeginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BeginError::TooMany => f.write_str("too many stops are in progress; try again in a moment"),
            BeginError::Refused(why) => f.write_str(why),
        }
    }
}

impl BeginError {
    /// The code a request answers with: a refusal is a bad request (the caller names a process that may not be stopped), only
    /// the bound is "busy".
    pub fn code(&self) -> pitcrew_core::protocol::ErrorCode {
        match self {
            BeginError::TooMany => pitcrew_core::protocol::ErrorCode::Busy,
            BeginError::Refused(_) => pitcrew_core::protocol::ErrorCode::BadRequest,
        }
    }
}

/// The identity the service holds for a listed process: its pid and the exact start the OS gave when it was listed.
/// `None` when the OS gave none (it cannot be told from a process that reuses the pid, so nothing acts on it).
pub fn identity_of(meta: &ProcessMeta) -> Option<ProcessId> {
    meta.started_micros.map(|us| ProcessId::new(meta.pid, StartTime::from_micros(us)))
}

#[derive(Debug, Clone)]
struct Stopping {
    id: ProcessId,
    name: String,
    rule: String,
    by: StopReason,
    progress: StopProgress,
    ended: bool,
}

/// A stop that finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stopped {
    /// Program name.
    pub name: String,
    /// Process id.
    pub pid: u32,
    /// How long it had run, in milliseconds.
    pub age_ms: u64,
    /// The rule behind it (`rule … (3 h)`, `default keep time (10 min)`, `you`).
    pub rule: String,
    /// Who decided.
    pub by: StopReason,
    /// It ignored the request and was ended.
    pub forced: bool,
    /// Whether the process is gone is not known: its identity could not be verified, so it was neither seen to stop nor ended.
    pub unconfirmed: bool,
}

/// One runner's processes.
#[derive(Default)]
pub struct ProcessWatch {
    tracked: Vec<Tracked>,
    ext: Extensions,
    stopping: Vec<Stopping>,
    runner_pid: Option<u32>,
    taken_at: Option<Timestamp>,
}

impl ProcessWatch {
    /// Nothing seen yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The runner is not running: forget its processes (stops already asked for still finish).
    pub fn clear(&mut self) {
        self.tracked.clear();
        self.runner_pid = None;
        self.ext.clear();
    }

    /// A new reading of the runner's tree.
    pub fn refresh(&mut self, now: Timestamp, runner_pid: u32, metas: &[ProcessMeta], spans: &[JobSpan], policy: &ProcessPolicy) {
        prune_extensions(&mut self.ext, metas);
        self.tracked = classify(runner_pid, metas, spans, policy, &self.ext);
        self.runner_pid = Some(runner_pid);
        self.taken_at = Some(now);
    }

    /// The runner's processes as last read.
    pub fn tracked(&self) -> &[Tracked] {
        &self.tracked
    }

    /// How many leftovers are alive.
    pub fn leftovers(&self) -> u32 {
        leftover_count(&self.tracked)
    }

    /// The processes that started during `job` and still run.
    pub fn survivors(&self, job: &FinishedJob) -> u32 {
        let metas: Vec<ProcessMeta> = self.tracked.iter().map(|t| t.meta.clone()).collect();
        process_track::survivors(job, &metas, self.runner_pid.unwrap_or(0))
    }

    /// The list for the Processes tab (`None` while the runner is not running).
    pub fn list(&self, idle: bool, policy: &ProcessPolicy) -> Option<ProcessList> {
        let runner_pid = self.runner_pid?;
        let (rows, truncated) = entries(runner_pid, &self.tracked);
        Some(ProcessList {
            runner_pid: Some(runner_pid),
            idle,
            cleanup: policy.cleanup,
            keep: policy.keep,
            entries: rows,
            truncated,
            taken_at: self.taken_at.unwrap_or(Timestamp(0)),
        })
    }

    /// The leftovers due to be stopped now (not those already being stopped).
    pub fn due(&self, now: Timestamp, idle: bool) -> Vec<Tracked> {
        due(&self.tracked, now, idle)
            .into_iter()
            .filter(|t| !self.is_stopping(t.meta.pid))
            .cloned()
            .collect()
    }

    /// Whether a stop of `pid` is under way.
    pub fn is_stopping(&self, pid: u32) -> bool {
        self.stopping.iter().any(|s| s.id.pid() == pid)
    }

    /// Checks that `pid` may be stopped (it is in the runner's tree as last read, and so on).
    pub fn check(&self, pid: u32, started_ms: u64, service_pid: u32, allow_runner: bool) -> Result<&Tracked, Refusal> {
        let runner_pid = self.runner_pid.ok_or(Refusal::NotInTree)?;
        let metas: Vec<ProcessMeta> = self.tracked.iter().map(|t| t.meta.clone()).collect();
        check_stop_target(pid, started_ms, runner_pid, service_pid, allow_runner, &metas)?;
        self.tracked.iter().find(|t| t.meta.pid == pid).ok_or(Refusal::NotInTree)
    }

    /// The "keep longer" extensions, for the state file.
    pub fn keeps(&self) -> Vec<KeepRecord> {
        export_extensions(&self.ext)
    }

    /// Restores extensions saved before a service restart.
    pub fn restore_keeps(&mut self, records: &[KeepRecord]) {
        self.ext = import_extensions(records);
    }

    /// Adds time before a leftover is stopped.
    pub fn keep(&mut self, pid: u32, started_ms: u64, extra: KeepFor) -> Result<(), String> {
        let t = self
            .tracked
            .iter()
            .find(|t| t.meta.pid == pid && t.meta.started_ms.is_some_and(|s| same_start(started_ms, s)))
            .ok_or("that process is not in the runner's list any more; refresh it")?;
        if t.leftover.is_none() {
            return Err("only leftover processes can be kept longer: this one belongs to a running job or to no job".into());
        }
        let started = t.meta.started_ms.unwrap_or(started_ms);
        extend(&mut self.ext, pid, started, extra.duration());
        Ok(())
    }

    /// Asks `target` to stop and tracks it until it is gone. `rule` is the words for the log (`you` for the user).
    pub fn begin(
        &mut self,
        target: &Tracked,
        rule: String,
        by: StopReason,
        now: Timestamp,
        policy: &ProcessPolicy,
        term: &dyn Terminator,
    ) -> Result<(), BeginError> {
        if self.stopping.len() >= MAX_STOPPING {
            return Err(BeginError::TooMany);
        }
        if self.is_stopping(target.meta.pid) {
            return Ok(());
        }
        // Rule 53: a process without a start time has no identity to verify, so it is never signalled.
        let id = identity_of(&target.meta).ok_or_else(|| BeginError::Refused(Refusal::NoStartTime.to_string()))?;
        match term.ask(id).map_err(BeginError::Refused)? {
            Asked::Delivered | Asked::NotTaken => {}
        }
        self.stopping.push(Stopping {
            id,
            name: target.meta.name.clone(),
            rule,
            by,
            progress: StopProgress {
                asked_at: now,
                timeout: Duration::from_secs(u64::from(policy.stop_timeout_secs)),
            },
            ended: false,
        });
        Ok(())
    }

    /// Moves the stops along: finished ones are returned, those that ignored the request are ended. `alive` looks at one
    /// held identity and says what it found (rule 53: never by pid alone).
    pub fn advance(&mut self, now: Timestamp, alive: &mut dyn FnMut(ProcessId) -> Seen, term: &dyn Terminator) -> Vec<Stopped> {
        let mut done = vec![];
        self.stopping.retain_mut(|s| {
            let seen = alive(s.id);
            let is_alive = seen != Seen::Gone;
            let age_ms = now.0.saturating_sub(s.id.started().as_micros() / 1000);
            let stopped = |s: &Stopping, rule: String, forced: bool, unconfirmed: bool| Stopped {
                name: s.name.clone(),
                pid: s.id.pid(),
                age_ms,
                rule,
                by: s.by,
                forced,
                unconfirmed,
            };
            match next_step(&s.progress, is_alive, now) {
                StopStep::Done => {
                    done.push(stopped(s, s.rule.clone(), s.ended, false));
                    false
                }
                StopStep::Wait => true,
                StopStep::Kill if seen == Seen::Unverified => {
                    // The pid may have been handed on: ending it by pid could end another process (rule 53).
                    let rule = format!(
                        "{} (could not confirm that it stopped: the process could not be verified to be the same one)",
                        s.rule
                    );
                    done.push(stopped(s, rule, s.ended, true));
                    false
                }
                StopStep::Kill if !s.ended => {
                    match term.end(s.id) {
                        Ending::Ended | Ending::Refused => {
                            s.ended = true;
                            // Give the kill a moment to take effect before the next check.
                            s.progress.asked_at = now;
                            s.progress.timeout = Duration::from_secs(5);
                            true
                        }
                        // It ended by itself since the look: the next look says so.
                        Ending::Gone => true,
                        // Its pid was handed on between the look and the kill and nothing was ended: not a forced stop.
                        Ending::NotOurs(_) => {
                            let rule = format!("{} (its process id was handed on to another process: nothing was ended)", s.rule);
                            done.push(stopped(s, rule, false, true));
                            false
                        }
                    }
                }
                StopStep::Kill => {
                    // Ended twice and still there (a process in uninterruptible sleep): give up on it.
                    done.push(stopped(s, format!("{} (could not be ended)", s.rule), true, false));
                    false
                }
            }
        });
        done
    }

    /// The label the log uses for a tracked leftover's rule.
    pub fn label(t: &Tracked) -> String {
        rule_label(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitcrew_core::jobs::JobInfo;
    use pitcrew_core::process_track::spans;
    use std::sync::Mutex;

    /// A terminator that records what it was asked and answers as it is told.
    struct Fake {
        asked: Mutex<Vec<ProcessId>>,
        ended: Mutex<Vec<ProcessId>>,
        ask_answer: Result<Asked, String>,
        end_answer: Ending,
    }

    impl Default for Fake {
        fn default() -> Fake {
            Fake {
                asked: Mutex::default(),
                ended: Mutex::default(),
                ask_answer: Ok(Asked::Delivered),
                end_answer: Ending::Ended,
            }
        }
    }

    impl Terminator for Fake {
        fn ask(&self, id: ProcessId) -> Result<Asked, String> {
            self.asked.lock().unwrap().push(id);
            self.ask_answer.clone()
        }
        fn end(&self, id: ProcessId) -> Ending {
            self.ended.lock().unwrap().push(id);
            self.end_answer
        }
    }

    /// The identity of a stand-in listed at `started_s` seconds.
    fn id(pid: u32, started_s: u64) -> ProcessId {
        ProcessId::new(pid, StartTime::from_micros(started_s * 1_000_000))
    }

    fn t(s: u64) -> Timestamp {
        Timestamp(s * 1000)
    }

    fn meta(pid: u32, name: &str, started_s: u64) -> ProcessMeta {
        ProcessMeta {
            pid,
            ppid: Some(1),
            name: name.into(),
            exe: None,
            command: name.into(),
            started_ms: Some(started_s * 1000),
            started_micros: Some(started_s * 1_000_000),
            cpu_permille: 0,
            mem_bytes: 1,
        }
    }

    fn watch() -> (ProcessWatch, ProcessPolicy) {
        let job = FinishedJob {
            info: JobInfo::default(),
            started_at: t(100),
            finished_at: t(200),
            outcome: None,
            peak_cpu_permille: 0,
            peak_mem_bytes: 0,
            leftovers: 0,
        };
        let policy = ProcessPolicy::default();
        let mut w = ProcessWatch::new();
        w.refresh(
            t(300),
            10,
            &[meta(10, "gitea-runner", 1), meta(11, "sleep", 150), meta(12, "adb", 150)],
            &spans(vec![job], None),
            &policy,
        );
        (w, policy)
    }

    #[test]
    fn the_list_counts_leftovers_and_the_runner_comes_with_it() {
        let (w, policy) = watch();
        assert_eq!(w.leftovers(), 2);
        let list = w.list(true, &policy).unwrap();
        assert_eq!(list.runner_pid, Some(10));
        assert!(list.idle && list.cleanup && !list.truncated);
        assert_eq!(list.entries.len(), 3);
        assert_eq!(list.taken_at, t(300));
        assert!(ProcessWatch::new().list(true, &policy).is_none());
        let job = FinishedJob {
            info: JobInfo::default(),
            started_at: t(100),
            finished_at: t(200),
            outcome: None,
            peak_cpu_permille: 0,
            peak_mem_bytes: 0,
            leftovers: 0,
        };
        assert_eq!(w.survivors(&job), 2);
    }

    #[test]
    fn a_due_leftover_is_asked_then_gone() {
        let (mut w, policy) = watch();
        let term = Fake::default();
        assert!(w.due(t(700), true).is_empty());
        let due = w.due(t(800), true);
        assert_eq!(due.len(), 1, "adb is never stopped");
        assert_eq!(due[0].meta.pid, 11);
        w.begin(&due[0], ProcessWatch::label(&due[0]), StopReason::Cleanup, t(800), &policy, &term)
            .unwrap();
        assert!(w.is_stopping(11));
        assert!(w.due(t(801), true).is_empty(), "not asked twice");
        w.begin(&due[0], "again".into(), StopReason::Cleanup, t(801), &policy, &term)
            .unwrap();
        assert_eq!(*term.asked.lock().unwrap(), vec![id(11, 150)]);
        // Still alive: wait. Then gone.
        assert!(w.advance(t(805), &mut |_| Seen::Same, &term).is_empty());
        let done = w.advance(t(806), &mut |_| Seen::Gone, &term);
        assert_eq!(done.len(), 1);
        assert_eq!((done[0].pid, done[0].forced, done[0].by), (11, false, StopReason::Cleanup));
        assert_eq!(done[0].age_ms, 806_000 - 150_000);
        assert_eq!(done[0].rule, "default keep time (10 min)");
        assert!(!w.is_stopping(11));
    }

    #[test]
    fn what_ignores_the_request_is_ended_after_the_timeout_and_a_stuck_one_is_given_up() {
        let (mut w, policy) = watch();
        let term = Fake::default();
        let target = w.check(11, 150_000, 99, false).unwrap().clone();
        w.begin(&target, "you".into(), StopReason::User, t(800), &policy, &term).unwrap();
        assert!(w.advance(t(814), &mut |_| Seen::Same, &term).is_empty());
        assert!(term.ended.lock().unwrap().is_empty());
        assert!(w.advance(t(815), &mut |_| Seen::Same, &term).is_empty());
        assert_eq!(*term.ended.lock().unwrap(), vec![id(11, 150)], "ended after the 15 s timeout");
        let done = w.advance(t(816), &mut |_| Seen::Gone, &term);
        assert!(done[0].forced && done[0].by == StopReason::User);
        // A process that survives even that.
        w.begin(&target, "you".into(), StopReason::User, t(900), &policy, &term).unwrap();
        w.advance(t(915), &mut |_| Seen::Same, &term);
        let done = w.advance(t(921), &mut |_| Seen::Same, &term);
        assert_eq!(done.len(), 1);
        assert!(done[0].rule.contains("could not be ended"));
    }

    #[test]
    fn a_process_that_does_not_take_the_polite_request_is_tracked_and_ended_after_the_timeout() {
        // Windows: a console process without a window refuses `taskkill` without /F. Before, the refusal ended the stop at
        // once and the forced end never ran.
        let (mut w, policy) = watch();
        let term = Fake {
            ask_answer: Ok(Asked::NotTaken),
            ..Default::default()
        };
        let target = w.check(11, 150_000, 99, false).unwrap().clone();
        w.begin(&target, "you".into(), StopReason::User, t(800), &policy, &term).unwrap();
        assert!(w.is_stopping(11));
        assert!(w.advance(t(814), &mut |_| Seen::Same, &term).is_empty());
        assert!(w.advance(t(815), &mut |_| Seen::Same, &term).is_empty());
        assert_eq!(*term.ended.lock().unwrap(), vec![id(11, 150)], "ended once the timeout passed");
        let done = w.advance(t(816), &mut |_| Seen::Gone, &term);
        assert!(done[0].forced);
    }

    #[test]
    fn a_kill_that_finds_the_pid_handed_on_is_not_reported_as_forced() {
        let (mut w, policy) = watch();
        let term = Fake {
            end_answer: Ending::NotOurs(StartTime::from_micros(9)),
            ..Default::default()
        };
        let target = w.check(11, 150_000, 99, false).unwrap().clone();
        w.begin(&target, "you".into(), StopReason::User, t(800), &policy, &term).unwrap();
        let done = w.advance(t(815), &mut |_| Seen::Same, &term);
        assert_eq!(done.len(), 1, "{done:?}");
        assert!(!done[0].forced, "nothing was ended: {done:?}");
        assert!(done[0].unconfirmed && done[0].rule.contains("handed on"), "{done:?}");
        assert!(!w.is_stopping(11));
    }

    #[test]
    fn a_kill_that_finds_it_already_gone_waits_for_the_next_look() {
        let (mut w, policy) = watch();
        let term = Fake {
            end_answer: Ending::Gone,
            ..Default::default()
        };
        let target = w.check(11, 150_000, 99, false).unwrap().clone();
        w.begin(&target, "you".into(), StopReason::User, t(800), &policy, &term).unwrap();
        assert!(w.advance(t(815), &mut |_| Seen::Same, &term).is_empty());
        let done = w.advance(t(816), &mut |_| Seen::Gone, &term);
        assert!(!done[0].forced, "it ended by itself: {done:?}");
    }

    #[test]
    fn a_process_whose_identity_cannot_be_verified_is_never_ended_by_its_pid() {
        let (mut w, policy) = watch();
        let term = Fake::default();
        let target = w.check(11, 150_000, 99, false).unwrap().clone();
        w.begin(&target, "you".into(), StopReason::User, t(800), &policy, &term).unwrap();
        assert!(w.advance(t(814), &mut |_| Seen::Unverified, &term).is_empty());
        // The timeout has passed: a pid that reports start time 0 may be a recycled one.
        let done = w.advance(t(815), &mut |_| Seen::Unverified, &term);
        assert!(term.ended.lock().unwrap().is_empty(), "never ended by a bare pid");
        let reported: Vec<_> = done
            .iter()
            .filter(|d| d.rule.contains("could not confirm that it stopped"))
            .collect();
        assert_eq!(reported.len(), 1, "reported, not silently dropped: {done:?}");
        assert!(
            reported[0].unconfirmed && !reported[0].forced,
            "not claimed as a stop or a kill: {done:?}"
        );
        assert!(!w.is_stopping(11));
    }

    #[test]
    fn a_process_without_a_start_time_is_never_signalled() {
        let (mut w, policy) = watch();
        let term = Fake::default();
        let mut target = w.check(11, 150_000, 99, false).unwrap().clone();
        target.meta.started_ms = None;
        target.meta.started_micros = None;
        let err = w
            .begin(&target, "you".into(), StopReason::User, t(800), &policy, &term)
            .unwrap_err();
        assert!(
            matches!(&err, BeginError::Refused(why) if why.contains("cannot be told apart")),
            "{err}"
        );
        assert_eq!(err.code(), pitcrew_core::protocol::ErrorCode::BadRequest);
        assert!(term.asked.lock().unwrap().is_empty() && !w.is_stopping(11));
    }

    #[test]
    fn refusals_and_errors() {
        let (mut w, policy) = watch();
        assert_eq!(w.check(500, 150_000, 99, false).unwrap_err(), Refusal::NotInTree);
        assert_eq!(w.check(10, 1_000, 99, false).unwrap_err(), Refusal::IsRunner);
        assert_eq!(w.check(10, 1_000, 99, true).unwrap().meta.pid, 10);
        assert_eq!(w.check(1, 150_000, 99, true).unwrap_err(), Refusal::SystemProcess);
        assert_eq!(ProcessWatch::new().check(11, 150_000, 99, false).unwrap_err(), Refusal::NotInTree);
        let target = w.check(11, 150_000, 99, false).unwrap().clone();
        let deny = Fake {
            ask_answer: Err("denied".into()),
            ..Default::default()
        };
        let denied = w.begin(&target, "x".into(), StopReason::User, t(1), &policy, &deny).unwrap_err();
        assert_eq!(denied, BeginError::Refused("denied".into()));
        assert_eq!(
            denied.code(),
            pitcrew_core::protocol::ErrorCode::BadRequest,
            "a refusal is not \"busy\""
        );
        assert!(!w.is_stopping(11));
        // The number of stops in progress is bounded.
        let term = Fake::default();
        for pid in 1000..1000 + MAX_STOPPING as u32 {
            let mut m = target.clone();
            m.meta.pid = pid;
            w.begin(&m, "x".into(), StopReason::Cleanup, t(1), &policy, &term).unwrap();
        }
        let full = w.begin(&target, "x".into(), StopReason::User, t(1), &policy, &term).unwrap_err();
        assert_eq!(full, BeginError::TooMany);
        assert_eq!(full.code(), pitcrew_core::protocol::ErrorCode::Busy);
    }

    #[test]
    fn keeping_longer_moves_the_stop_time_and_only_for_leftovers() {
        let (mut w, policy) = watch();
        w.keep(11, 150_000, KeepFor::hours(1)).unwrap();
        w.refresh(
            t(301),
            10,
            &w.tracked().iter().map(|t| t.meta.clone()).collect::<Vec<_>>(),
            &[],
            &policy,
        );
        // Without job spans nothing is a leftover any more: keeping is refused.
        assert!(w.keep(11, 150_000, KeepFor::hours(1)).unwrap_err().contains("only leftover"));
        assert!(w.keep(999, 1, KeepFor::hours(1)).unwrap_err().contains("not in the runner's list"));
        let (mut w, policy) = watch();
        w.keep(11, 150_000, KeepFor::hours(1)).unwrap();
        let metas: Vec<ProcessMeta> = w.tracked().iter().map(|t| t.meta.clone()).collect();
        let job = FinishedJob {
            info: JobInfo::default(),
            started_at: t(100),
            finished_at: t(200),
            outcome: None,
            peak_cpu_permille: 0,
            peak_mem_bytes: 0,
            leftovers: 0,
        };
        w.refresh(t(400), 10, &metas, &spans(vec![job], None), &policy);
        assert!(w.due(t(800), true).is_empty(), "an hour was added");
        assert_eq!(w.due(t(200 + 600 + 3600), true).len(), 1);
        w.clear();
        assert!(w.list(true, &policy).is_none() && w.leftovers() == 0);
    }
}
