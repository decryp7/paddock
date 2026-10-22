//! What the service knows about one runner's processes: the latest classified tree, the "keep longer" extensions
//! and the stops in progress (ADR 0012).
//!
//! The decisions are `pitcrew_core::process_track`'s; this holds the state between ticks and carries out the
//! operating-system side through a [`Terminator`] and an `alive` check, both passed in so the tests need no real
//! process.

use crate::terminate::Terminator;
use pitcrew_core::history::StopReason;
use pitcrew_core::jobs::FinishedJob;
use pitcrew_core::process_policy::{KeepFor, ProcessPolicy};
use pitcrew_core::process_track::{
    self, check_stop_target, classify, due, entries, extend, leftover_count, next_step, prune_extensions, rule_label, Extensions, JobSpan,
    ProcessMeta, Refusal, StopProgress, StopStep, Tracked,
};
use pitcrew_core::protocol::ProcessList;
use pitcrew_core::time::Timestamp;
use std::time::Duration;

/// Most stops in progress per runner at once (a cleanup after a big job can have many leftovers).
pub const MAX_STOPPING: usize = 64;

/// A stop that was asked for.
#[derive(Debug, Clone)]
struct Stopping {
    pid: u32,
    started_ms: Option<u64>,
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
        self.stopping.iter().any(|s| s.pid == pid)
    }

    /// Checks that `pid` may be stopped (it is in the runner's tree as last read, and so on).
    pub fn check(&self, pid: u32, started_ms: Option<u64>, service_pid: u32, allow_runner: bool) -> Result<&Tracked, Refusal> {
        let runner_pid = self.runner_pid.ok_or(Refusal::NotInTree)?;
        let metas: Vec<ProcessMeta> = self.tracked.iter().map(|t| t.meta.clone()).collect();
        check_stop_target(pid, started_ms, runner_pid, service_pid, allow_runner, &metas)?;
        self.tracked.iter().find(|t| t.meta.pid == pid).ok_or(Refusal::NotInTree)
    }

    /// Adds time before a leftover is stopped.
    pub fn keep(&mut self, pid: u32, started_ms: u64, extra: KeepFor) -> Result<(), String> {
        let t = self
            .tracked
            .iter()
            .find(|t| t.meta.pid == pid && t.meta.started_ms.is_some_and(|s| s.abs_diff(started_ms) <= 1500))
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
    ) -> Result<(), String> {
        if self.stopping.len() >= MAX_STOPPING {
            return Err("too many stops are in progress; try again in a moment".into());
        }
        if self.is_stopping(target.meta.pid) {
            return Ok(());
        }
        term.ask(target.meta.pid)?;
        self.stopping.push(Stopping {
            pid: target.meta.pid,
            started_ms: target.meta.started_ms,
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

    /// Moves the stops along: finished ones are returned, those that ignored the request are ended.
    pub fn advance(&mut self, now: Timestamp, alive: &mut dyn FnMut(u32, Option<u64>) -> bool, term: &dyn Terminator) -> Vec<Stopped> {
        let mut done = vec![];
        self.stopping.retain_mut(|s| {
            let is_alive = alive(s.pid, s.started_ms);
            match next_step(&s.progress, is_alive, now) {
                StopStep::Done => {
                    done.push(Stopped {
                        name: s.name.clone(),
                        pid: s.pid,
                        age_ms: s.started_ms.map_or(0, |st| now.0.saturating_sub(st)),
                        rule: s.rule.clone(),
                        by: s.by,
                        forced: s.ended,
                    });
                    false
                }
                StopStep::Wait => true,
                StopStep::Kill => {
                    if !s.ended {
                        term.end(s.pid);
                        s.ended = true;
                        // Give the kill a moment to take effect before the next check.
                        s.progress.asked_at = now;
                        s.progress.timeout = Duration::from_secs(5);
                    } else {
                        // Ended twice and still there (a process in uninterruptible sleep): give up on it.
                        done.push(Stopped {
                            name: s.name.clone(),
                            pid: s.pid,
                            age_ms: s.started_ms.map_or(0, |st| now.0.saturating_sub(st)),
                            rule: format!("{} (could not be ended)", s.rule),
                            by: s.by,
                            forced: true,
                        });
                        return false;
                    }
                    true
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

    #[derive(Default)]
    struct Fake {
        asked: Mutex<Vec<u32>>,
        ended: Mutex<Vec<u32>>,
        refuse: bool,
    }

    impl Terminator for Fake {
        fn ask(&self, pid: u32) -> Result<(), String> {
            if self.refuse {
                return Err("denied".into());
            }
            self.asked.lock().unwrap().push(pid);
            Ok(())
        }
        fn end(&self, pid: u32) {
            self.ended.lock().unwrap().push(pid);
        }
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
        assert_eq!(*term.asked.lock().unwrap(), vec![11]);
        // Still alive: wait. Then gone.
        assert!(w.advance(t(805), &mut |_, _| true, &term).is_empty());
        let done = w.advance(t(806), &mut |_, _| false, &term);
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
        let target = w.check(11, None, 99, false).unwrap().clone();
        w.begin(&target, "you".into(), StopReason::User, t(800), &policy, &term).unwrap();
        assert!(w.advance(t(814), &mut |_, _| true, &term).is_empty());
        assert!(term.ended.lock().unwrap().is_empty());
        assert!(w.advance(t(815), &mut |_, _| true, &term).is_empty());
        assert_eq!(*term.ended.lock().unwrap(), vec![11], "ended after the 15 s timeout");
        let done = w.advance(t(816), &mut |_, _| false, &term);
        assert!(done[0].forced && done[0].by == StopReason::User);
        // A process that survives even that.
        w.begin(&target, "you".into(), StopReason::User, t(900), &policy, &term).unwrap();
        w.advance(t(915), &mut |_, _| true, &term);
        let done = w.advance(t(921), &mut |_, _| true, &term);
        assert_eq!(done.len(), 1);
        assert!(done[0].rule.contains("could not be ended"));
    }

    #[test]
    fn refusals_and_errors() {
        let (mut w, policy) = watch();
        assert_eq!(w.check(500, None, 99, false).unwrap_err(), Refusal::NotInTree);
        assert_eq!(w.check(10, None, 99, false).unwrap_err(), Refusal::IsRunner);
        assert_eq!(w.check(10, None, 99, true).unwrap().meta.pid, 10);
        assert_eq!(w.check(1, None, 99, true).unwrap_err(), Refusal::SystemProcess);
        assert_eq!(ProcessWatch::new().check(11, None, 99, false).unwrap_err(), Refusal::NotInTree);
        let target = w.check(11, None, 99, false).unwrap().clone();
        let deny = Fake {
            refuse: true,
            ..Default::default()
        };
        assert_eq!(
            w.begin(&target, "x".into(), StopReason::User, t(1), &policy, &deny).unwrap_err(),
            "denied"
        );
        assert!(!w.is_stopping(11));
        // The number of stops in progress is bounded.
        let term = Fake::default();
        for pid in 1000..1000 + MAX_STOPPING as u32 {
            let mut m = target.clone();
            m.meta.pid = pid;
            w.begin(&m, "x".into(), StopReason::Cleanup, t(1), &policy, &term).unwrap();
        }
        assert!(w
            .begin(&target, "x".into(), StopReason::User, t(1), &policy, &term)
            .unwrap_err()
            .contains("too many"));
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
