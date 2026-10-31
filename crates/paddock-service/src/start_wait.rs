//! Waiting for a registered service to come up, judged on what is observed (ADR 0031: patience, not wall clocks), behind
//! the [`StartWatch`] hook of [`Manager`](crate::Manager). The pure parts, so they are tested on every OS: a watch is handed
//! one [`StartObservation`] per poll and answers a [`StartVerdict`]; time never enters it.
//!
//! [`StartWait`] is the first implementation. A slow but alive start is waited for; it fails fast only on a real sign: the
//! job vanished from the OS, it restarts in a loop without staying up, or the shared start judge
//! ([`paddock_proc::StartJudge`]) calls it stuck (spinning, no CPU and no progress, or not observable for long).
//!
//! The judge fits a launchd job: liveness is the job's `state = running` (a job launchd has not started yet is
//! `Paused`, which spends no patience: launchd needed about a minute on the owner's Mac), the handshake is the service
//! answering, and the CPU time is read from the job's pid. It reports no `progress` counter: launchd has no progress
//! channel, and the CPU time is the signal. [`START_CEILING_POLLS`] is a backstop, never the judge.
//!
//! A backend that cannot supply the facts ([`Backend::observe_start`](crate::Backend::observe_start) answers `None`:
//! systemd, Windows) is not watched; its repair keeps the fixed wait of [`Timings::start_wait`](crate::Timings).

use crate::backend::{Job, StartFacts};
use paddock_proc::{
    Cpu, Liveness, Patience, Reading, RunState, StallCause, StartJudge, StartState,
};
use std::time::Duration;

/// How often the service is asked while waiting (one poll is one tick of the judge); the default of
/// [`Timings::start_poll`](crate::Timings).
pub const POLL: Duration = Duration::from_secs(3);
/// The backstop: after this many polls (about ten minutes at [`POLL`]) a start that is still neither answering nor stuck
/// is given up on, so a wait always ends. It counts polls, not seconds, so a paused or starved Mac does not spend it. It
/// is not the judge: a start that is alive and not stuck keeps being waited for until it.
pub const START_CEILING_POLLS: u32 = 200;
/// After this many polls (one minute) alive and not answering, the wait says it is still working.
pub const STILL_WORKING_AFTER_POLLS: u32 = 20;
/// The job must be missing from the OS on this many polls in a row before that counts (launchd can lag after a register).
pub const VANISHED_POLLS: u32 = 3;
/// A job that is not running and was started this many more times than at the first look, the last time with a non-zero
/// exit code, is restarting in a loop.
pub const CRASH_RUNS: u32 = 3;

/// The patience of the start judge, in polls of `tick`.
pub fn patience(tick: Duration) -> Patience {
    Patience {
        spin: Duration::from_secs(120),
        idle: Duration::from_secs(60),
        hang_guard: Duration::from_secs(300),
        note_after: STILL_WORKING_AFTER_POLLS,
        tick,
        cpu_every: 1,
    }
}

/// What one poll saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartObservation {
    /// The service answers.
    pub answers: bool,
    /// What the backend saw of the job.
    pub facts: StartFacts,
}

/// What a poll decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartVerdict {
    /// Keep waiting.
    Waiting,
    /// Keep waiting, and say so ("still working").
    StillWorking(String),
    /// The service answers.
    Up,
    /// The OS refuses the job (spawn failed, exit 78, needs a launch constraint update): the repair registers again. The
    /// text says what was observed.
    Refused(String),
    /// It will not come up; the text says what was observed.
    Failed(String),
}

/// Judges a service that is starting, one observation per poll. [`Manager`](crate::Manager) asks it while a repair waits
/// for the service (and only for a backend that can observe the job); with none the repair waits a fixed time.
pub trait StartWatch: Send {
    /// A new wait begins (each registration of a repair starts one): forget what the last one saw. Nothing by default.
    fn reset(&mut self) {}
    /// Judges one poll. The wait ends on [`StartVerdict::Up`], [`Refused`](StartVerdict::Refused) or
    /// [`Failed`](StartVerdict::Failed), so an implementation must end by itself (see [`START_CEILING_POLLS`]).
    fn poll(&mut self, obs: &StartObservation) -> StartVerdict;
}

/// The recommended [`StartWatch`]: a pure wait over [`paddock_proc::StartJudge`].
pub struct StartWait {
    tick: Duration,
    judge: StartJudge,
    polls: u32,
    vanished: u32,
    first_runs: Option<u32>,
    noted: bool,
    /// The job's process on the last look that had one.
    pid: Option<u32>,
}

impl Default for StartWait {
    fn default() -> Self {
        Self::new()
    }
}

impl StartWait {
    /// A wait polled every [`POLL`], with nothing seen.
    pub fn new() -> Self {
        Self::with_poll(POLL)
    }

    /// A wait polled every `tick` (match [`Timings::start_poll`](crate::Timings)), with nothing seen.
    pub fn with_poll(tick: Duration) -> Self {
        Self {
            tick,
            judge: StartJudge::new(patience(tick), "the background service"),
            polls: 0,
            vanished: 0,
            first_runs: None,
            noted: false,
            pid: None,
        }
    }

    /// Judges one poll.
    pub fn observe(&mut self, o: &StartObservation) -> StartVerdict {
        if o.answers {
            return StartVerdict::Up;
        }
        let f = &o.facts;
        self.polls += 1;
        let seen = describe(f);
        if self.polls > START_CEILING_POLLS {
            return StartVerdict::Failed(format!(
                "the service did not answer after {START_CEILING_POLLS} checks (observed: {seen}); this is the longest the app waits"
            ));
        }
        if f.job == Job::Absent {
            self.vanished += 1;
            if self.vanished >= VANISHED_POLLS {
                return StartVerdict::Failed(format!(
                    "the OS dropped the job while the service was starting (observed: {seen})"
                ));
            }
        } else if f.job != Job::Unknown {
            // A look the OS could not answer (a failed or slow `launchctl print`) is neither gone nor seen.
            self.vanished = 0;
        }
        if f.job == Job::Refused {
            return StartVerdict::Refused(seen);
        }
        let running = f.job == Job::Running;
        if let Some(runs) = f.runs {
            let first = *self.first_runs.get_or_insert(runs);
            if !running
                && f.last_exit.is_some_and(|c| c != 0)
                && runs >= first.saturating_add(CRASH_RUNS)
            {
                return StartVerdict::Failed(format!(
                    "the service keeps exiting and being restarted instead of staying up (observed: {seen})"
                ));
            }
        }
        // The OS started the job again as another process: what the judge counted belongs to the one that is gone. The run
        // count and the poll count are the whole start's and stay.
        if let Some(pid) = f.pid {
            if self.pid.is_some_and(|old| old != pid) {
                self.judge = StartJudge::new(patience(self.tick), "the background service");
            }
            self.pid = Some(pid);
        }
        // A job the OS has not started yet (or between restarts) is `Paused`: it spends no patience.
        // Also a process the OS or an app (App Tamer) has stopped: it goes on when it is resumed, so it is waited for.
        let stopped = running && f.run_state == RunState::Stopped;
        let liveness = if running && !stopped {
            Liveness::Running
        } else {
            Liveness::Paused
        };
        let cpu = match (running, f.cpu) {
            (true, Some(c)) => Cpu::Used(c),
            (true, None) => Cpu::Unavailable,
            (false, _) => Cpu::NotRead,
        };
        let verdict = match self.judge.observe(&Reading {
            liveness,
            ready: false,
            progress: 0,
            cpu,
        }) {
            // The judge is never told `ready` or `Exited`: the answer and the job's end are read from the observation.
            StartState::Starting | StartState::Ready | StartState::Exited(_) => {
                StartVerdict::Waiting
            }
            StartState::Stalled(report) => match report.cause {
                // A job the OS has not started yet is also fed as paused; only a stopped process is worth saying.
                StallCause::Paused if !stopped => StartVerdict::Waiting,
                StallCause::Paused => StartVerdict::StillWorking(self.said(format!(
                    "the service is paused by the system; waiting (observed: {seen})"
                ))),
                // Idle is only a sign of a stuck process when the OS says the process is not stopped; where it cannot say
                // (Windows, another user's process) a flat CPU time is a reason to say so, not to give up.
                StallCause::Idle(_) if f.run_state != RunState::Running => {
                    StartVerdict::StillWorking(self.said(format!(
                        "still working: the service is using no CPU and the system does not say whether it is paused; waiting (observed: {seen})"
                    )))
                }
                cause if cause.is_stuck() => {
                    StartVerdict::Failed(format!("{} (observed: {seen})", report.message))
                }
                _ => StartVerdict::StillWorking(self.note(&seen)),
            },
        };
        // The judge spends no patience on a job that is not running, so it never notes one; the note is counted here.
        if verdict == StartVerdict::Waiting
            && !self.noted
            && self.polls >= STILL_WORKING_AFTER_POLLS
        {
            return StartVerdict::StillWorking(self.note(&seen));
        }
        if let StartVerdict::StillWorking(_) = verdict {
            self.noted = true;
        }
        verdict
    }

    fn note(&mut self, seen: &str) -> String {
        self.said(format!(
            "still working: the service has not answered yet (observed: {seen})"
        ))
    }

    /// A note said: the generic one is not repeated after it.
    fn said(&mut self, text: String) -> String {
        self.noted = true;
        text
    }
}

impl StartWatch for StartWait {
    fn reset(&mut self) {
        *self = Self::with_poll(self.tick);
    }

    fn poll(&mut self, obs: &StartObservation) -> StartVerdict {
        self.observe(obs)
    }
}

fn describe(f: &StartFacts) -> String {
    let mut s = match f.job {
        Job::Absent => "the OS does not know the job",
        Job::Running => "running",
        Job::NotRunning => "not running",
        Job::Refused => "not running, refused by the OS",
        Job::Unknown => "the OS does not say",
    }
    .to_string();
    if let Some(c) = f.last_exit {
        s.push_str(&format!(", last exit code {c}"));
    }
    if let Some(r) = f.runs {
        s.push_str(&format!(", started {r} times"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_proc::RunState;

    fn facts(job: Job) -> StartFacts {
        StartFacts {
            job,
            ..StartFacts::unknown()
        }
    }
    fn obs(facts: StartFacts) -> StartObservation {
        StartObservation {
            answers: false,
            facts,
        }
    }
    fn running(cpu: u64) -> StartObservation {
        obs(StartFacts {
            job: Job::Running,
            pid: Some(7),
            runs: Some(1),
            last_exit: None,
            cpu: Some(Duration::from_secs(cpu)),
            run_state: RunState::Running,
        })
    }
    fn with_state(mut o: StartObservation, state: RunState) -> StartObservation {
        o.facts.run_state = state;
        o
    }
    fn pending() -> StartObservation {
        obs(StartFacts {
            runs: Some(1),
            ..facts(Job::NotRunning)
        })
    }
    fn gone() -> StartObservation {
        obs(facts(Job::Absent))
    }

    #[test]
    fn an_answering_service_is_up_whatever_the_os_says() {
        let mut w = StartWait::new();
        let mut o = pending();
        o.answers = true;
        assert_eq!(w.observe(&o), StartVerdict::Up);
        o.facts = facts(Job::Absent);
        assert_eq!(w.observe(&o), StartVerdict::Up);
    }

    #[test]
    fn a_job_the_os_starts_late_is_waited_for_far_past_the_old_two_minutes() {
        let mut w = StartWait::new();
        // 150 polls (7.5 minutes) with the job known but not started: no patience is spent, nothing fails.
        for _ in 0..150 {
            assert!(matches!(
                w.observe(&pending()),
                StartVerdict::Waiting | StartVerdict::StillWorking(_)
            ));
        }
        assert_eq!(
            w.observe(&StartObservation {
                answers: true,
                ..pending()
            }),
            StartVerdict::Up
        );
    }

    #[test]
    fn a_new_process_of_the_job_starts_the_judge_afresh_but_keeps_the_crash_count() {
        let with_pid = |pid: u32| {
            let mut o = running(5);
            o.facts.pid = Some(pid);
            o
        };
        // 15 idle polls of one process, 15 of the process launchd restarted it as: each is under the idle patience.
        let mut w = StartWait::new();
        for pid in [7, 8] {
            for i in 0..15 {
                let v = w.observe(&with_pid(pid));
                assert!(
                    !matches!(v, StartVerdict::Failed(_)),
                    "pid {pid} poll {i}: {v:?}"
                );
            }
        }
        // The same process for 30 polls is stuck.
        let mut w = StartWait::new();
        let stuck = (0..30).any(|_| matches!(w.observe(&with_pid(7)), StartVerdict::Failed(_)));
        assert!(stuck);
        // A look without a pid (the job between runs) does not count as a new process.
        let mut w = StartWait::new();
        let mut failed = false;
        for i in 0..30 {
            let mut o = with_pid(7);
            if i % 2 == 1 {
                o.facts.pid = None;
            }
            failed |= matches!(w.observe(&o), StartVerdict::Failed(_));
        }
        assert!(failed, "a missing pid is not a restart");
        // The run count survives the restart: the crash loop is still seen.
        let mut w = StartWait::new();
        w.observe(&pending());
        w.observe(&with_pid(7));
        let mut o = pending();
        o.facts.runs = Some(1 + CRASH_RUNS);
        o.facts.last_exit = Some(3);
        o.facts.pid = Some(9);
        assert!(matches!(w.observe(&o), StartVerdict::Failed(_)));
        // The restart itself (pid 9, runs 2) keeps the first look's run count, so a later look three runs on is a loop.
        let mut w = StartWait::new();
        w.observe(&pending());
        w.observe(&with_pid(7));
        let mut restarted = with_pid(9);
        restarted.facts.runs = Some(2);
        assert!(!matches!(w.observe(&restarted), StartVerdict::Failed(_)));
        let mut o = pending();
        o.facts.runs = Some(1 + CRASH_RUNS);
        o.facts.last_exit = Some(3);
        assert!(matches!(w.observe(&o), StartVerdict::Failed(_)));
    }

    #[test]
    fn a_running_job_with_flat_cpu_whose_process_is_stopped_keeps_being_waited_for() {
        let mut w = StartWait::new();
        let mut notes = Vec::new();
        for i in 0..150 {
            match w.observe(&with_state(running(5), RunState::Stopped)) {
                StartVerdict::Failed(m) => panic!("poll {i}: {m}"),
                StartVerdict::StillWorking(m) => notes.push(m),
                _ => {}
            }
        }
        assert!(
            notes
                .iter()
                .any(|m| m.contains("paused by the system") && m.contains("waiting")),
            "{notes:?}"
        );
        // Resumed with the same flat CPU: the idle count goes on from where it was, so it ends as idle soon after.
        let mut ended = None;
        for _ in 0..40 {
            if let StartVerdict::Failed(m) = w.observe(&running(5)) {
                ended = Some(m);
                break;
            }
        }
        assert!(ended.is_some_and(|m| m.contains("observed: running")));
    }

    #[test]
    fn the_same_job_not_stopped_ends_as_idle_and_an_unreadable_run_state_never_does() {
        let mut w = StartWait::new();
        assert!(
            (0..40).any(|_| matches!(w.observe(&running(5)), StartVerdict::Failed(_))),
            "a known-running idle start is stuck"
        );
        for state in [RunState::Unknown, RunState::Exited] {
            let mut w = StartWait::new();
            let mut notes = 0;
            for i in 0..150 {
                match w.observe(&with_state(running(5), state)) {
                    StartVerdict::Failed(m) => panic!("{state:?} poll {i}: {m}"),
                    StartVerdict::StillWorking(m) => {
                        assert!(!m.contains("paused by the system"), "{state:?}: {m}");
                        notes += 1;
                    }
                    _ => {}
                }
            }
            assert!(notes >= 1, "{state:?} says it is still working");
        }
    }

    #[test]
    fn a_pending_job_says_it_is_still_working_once_after_the_note_threshold() {
        let mut w = StartWait::new();
        let mut notes = Vec::new();
        for i in 1..=100 {
            if let StartVerdict::StillWorking(m) = w.observe(&pending()) {
                notes.push((i, m));
            }
        }
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert_eq!(notes[0].0, STILL_WORKING_AFTER_POLLS);
        assert!(notes[0].1.contains("still working") && notes[0].1.contains("not running"));
    }

    #[test]
    fn a_failed_look_never_reads_as_a_gone_job_however_often_it_happens() {
        let mut w = StartWait::new();
        for _ in 0..50 {
            assert!(!matches!(
                w.observe(&obs(facts(Job::Unknown))),
                StartVerdict::Failed(_) | StartVerdict::Refused(_)
            ));
        }
        // It neither adds to the run of missing looks nor ends it.
        let mut w = StartWait::new();
        w.observe(&gone());
        w.observe(&gone());
        w.observe(&obs(facts(Job::Unknown)));
        assert!(matches!(w.observe(&gone()), StartVerdict::Failed(_)));
    }

    #[test]
    fn a_slow_but_alive_start_says_it_is_still_working_once_and_goes_on() {
        let mut w = StartWait::new();
        let mut notes = 0;
        for i in 0..100 {
            // The process keeps using CPU time in small steps but is not spinning past the limit.
            match w.observe(&running(i / 10)) {
                StartVerdict::StillWorking(m) => {
                    notes += 1;
                    assert!(m.contains("still working") && m.contains("running"), "{m}");
                }
                StartVerdict::Waiting => {}
                other => panic!("{i}: {other:?}"),
            }
        }
        assert_eq!(notes, 1, "the note is said once");
    }

    #[test]
    fn a_running_job_that_does_nothing_is_stuck_after_the_idle_patience_with_the_observed_state() {
        let mut w = StartWait::new();
        let mut why = None;
        for _ in 0..40 {
            if let StartVerdict::Failed(m) = w.observe(&running(5)) {
                why = Some(m);
                break;
            }
        }
        let why = why.expect("an idle start is called stuck");
        assert!(why.contains("observed: running"), "{why}");
    }

    #[test]
    fn a_running_job_whose_cpu_time_cannot_be_read_is_bounded_by_the_hang_guard() {
        let mut w = StartWait::new();
        let blind = obs(StartFacts {
            cpu: None,
            ..running(0).facts
        });
        let mut why = None;
        for polls in 1..=START_CEILING_POLLS {
            if let StartVerdict::Failed(m) = w.observe(&blind) {
                // An unreadable CPU time is blind from the first poll (300 s of 3 s polls); a time that was only not
                // read this poll would be forgiven for its first two.
                assert_eq!(polls, 100, "the judge ends it, not the backstop");
                why = Some(m);
                break;
            }
        }
        assert!(
            why.expect("a blind start ends")
                .contains("observed: running")
        );
    }

    #[test]
    fn a_job_the_os_refuses_is_handed_back_for_another_cycle() {
        let mut w = StartWait::new();
        let StartVerdict::Refused(seen) = w.observe(&obs(facts(Job::Refused))) else {
            panic!()
        };
        assert!(seen.contains("refused"), "{seen}");
    }

    #[test]
    fn a_vanished_job_fails_fast_but_not_on_one_lagging_look() {
        let mut w = StartWait::new();
        assert_eq!(w.observe(&gone()), StartVerdict::Waiting);
        assert_eq!(
            w.observe(&pending()),
            StartVerdict::Waiting,
            "seen again: the count restarts"
        );
        assert_eq!(w.observe(&gone()), StartVerdict::Waiting);
        assert_eq!(w.observe(&gone()), StartVerdict::Waiting);
        let StartVerdict::Failed(m) = w.observe(&gone()) else {
            panic!()
        };
        assert!(
            m.contains("dropped the job") && m.contains("does not know the job"),
            "{m}"
        );
    }

    #[test]
    fn an_os_that_does_not_say_is_not_a_vanished_job() {
        let mut w = StartWait::new();
        for _ in 0..VANISHED_POLLS + 2 {
            assert_eq!(w.observe(&obs(facts(Job::Unknown))), StartVerdict::Waiting);
        }
    }

    #[test]
    fn a_job_that_keeps_exiting_is_a_crash_loop() {
        let mut w = StartWait::new();
        let mut o = pending();
        assert_eq!(w.observe(&o), StartVerdict::Waiting);
        o.facts.runs = Some(1 + CRASH_RUNS);
        o.facts.last_exit = Some(3);
        let StartVerdict::Failed(m) = w.observe(&o) else {
            panic!()
        };
        assert!(
            m.contains("last exit code 3") && m.contains("started 4 times"),
            "{m}"
        );
        // One run short of a loop is not one.
        let mut w = StartWait::new();
        w.observe(&pending());
        o.facts.runs = Some(CRASH_RUNS);
        assert_eq!(w.observe(&o), StartVerdict::Waiting);
        // Restarts that exited cleanly, or a job that is up, are not a loop.
        let mut w = StartWait::new();
        w.observe(&pending());
        o.facts.runs = Some(1 + CRASH_RUNS);
        o.facts.last_exit = Some(0);
        assert_eq!(w.observe(&o), StartVerdict::Waiting);
        o.facts.last_exit = Some(3);
        o.facts.job = Job::Running;
        assert!(!matches!(w.observe(&o), StartVerdict::Failed(_)));
        // No exit code yet: not a loop either.
        let mut w = StartWait::new();
        w.observe(&pending());
        o.facts.job = Job::NotRunning;
        o.facts.last_exit = None;
        assert_eq!(w.observe(&o), StartVerdict::Waiting);
    }

    #[test]
    fn the_ceiling_ends_a_wait_that_is_neither_answering_nor_stuck_and_names_what_was_seen() {
        let mut w = StartWait::new();
        let mut verdict = StartVerdict::Waiting;
        let mut polls = 0;
        // Alive with CPU growing slowly forever: never stuck, so only the backstop ends it.
        while !matches!(verdict, StartVerdict::Failed(_)) && polls < START_CEILING_POLLS + 5 {
            polls += 1;
            let mut o = pending();
            o.facts.job = Job::Running;
            o.facts.cpu = Some(Duration::from_millis(u64::from(polls) * 100));
            verdict = w.observe(&o);
        }
        let StartVerdict::Failed(m) = verdict else {
            panic!("{verdict:?}")
        };
        assert_eq!(polls, START_CEILING_POLLS + 1, "{m}");
        assert!(
            m.contains("longest the app waits") && m.contains("observed: running"),
            "{m}"
        );
    }

    #[test]
    fn as_a_watch_it_forgets_the_last_wait_on_reset_and_keeps_its_poll() {
        let mut w = StartWait::with_poll(Duration::from_secs(1));
        for _ in 0..VANISHED_POLLS - 1 {
            assert_eq!(StartWatch::poll(&mut w, &gone()), StartVerdict::Waiting);
        }
        w.reset();
        assert_eq!(w.tick, Duration::from_secs(1));
        // After the reset the run of missing looks starts again.
        assert_eq!(StartWatch::poll(&mut w, &gone()), StartVerdict::Waiting);
        assert_eq!(StartWatch::poll(&mut w, &gone()), StartVerdict::Waiting);
        assert!(matches!(
            StartWatch::poll(&mut w, &gone()),
            StartVerdict::Failed(_)
        ));
    }

    #[test]
    fn a_watch_that_keeps_no_state_needs_no_reset() {
        struct Stateless;
        impl StartWatch for Stateless {
            fn poll(&mut self, _: &StartObservation) -> StartVerdict {
                StartVerdict::Up
            }
        }
        let mut w = Stateless;
        w.reset();
        assert_eq!(w.poll(&pending()), StartVerdict::Up);
    }

    #[test]
    fn the_default_wait_polls_at_the_default_rate() {
        assert_eq!(StartWait::default().tick, POLL);
        assert_eq!(POLL, crate::Timings::default().start_poll);
    }
}
