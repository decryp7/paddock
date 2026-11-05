//! Replacing a registration (rule 24): unregister → evict (`launchctl bootout`) → wait until the OS has dropped the job
//! (bounded) → register → check that the OS accepts it → wait for the service to answer (bounded). launchd keeps the old
//! build's launch constraint while the job lives, so registering over a live job does nothing (R03); the cycle repeats
//! a bounded number of times, waiting longer each time. Every step is said in words ([`Step`], phase-labelled progress
//! for the log and the UI); waits go through the backend's port, so tests run them on a fake clock.

use crate::backend::{Backend, Job};
use crate::port::RegisterError;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The waits of a repair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timings {
    /// How many full cycles follow the first while the OS still refuses the new registration.
    pub max_cycles: u32,
    /// The pause after registering before the OS's report is read.
    pub settle: Duration,
    /// How long the OS gets to drop the removed job before registering anyway.
    pub gone_wait: Duration,
    /// How often the OS is asked whether the job is gone.
    pub gone_poll: Duration,
    /// How long an accepted registration gets to answer (launchd needed about a minute on the owner's Mac).
    pub start_wait: Duration,
    /// How often the service is asked while waiting.
    pub start_poll: Duration,
    /// The extra pause before registering again, per retry.
    pub backoff: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            max_cycles: 3,
            settle: Duration::from_secs(3),
            gone_wait: Duration::from_secs(20),
            gone_poll: Duration::from_secs(1),
            start_wait: Duration::from_secs(120),
            start_poll: Duration::from_secs(3),
            backoff: Duration::from_secs(2),
        }
    }
}

impl Timings {
    /// The same waits, but a registration the OS accepted is not waited on for the service to answer: for a caller that
    /// must return at once (the Settings switch) and watches the service by other means.
    #[must_use]
    pub fn without_start_wait(self) -> Timings {
        Timings {
            start_wait: Duration::ZERO,
            ..self
        }
    }

    /// The longest a repair can wait in total (the bound the tests check).
    pub fn worst_case(&self) -> Duration {
        let cycles = self.max_cycles + 1;
        let mut total = self.start_wait + self.start_poll;
        for c in 0..cycles {
            total += self.gone_wait + self.gone_poll + self.settle + self.backoff * c;
        }
        total
    }
}

/// A phase of the repair, for the log and the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// A cycle starts (`0` is the first; later ones are retries).
    Cycle(u32),
    /// The old registration is removed.
    Removing,
    /// The OS is waited for to drop the old job.
    WaitingForRemoval,
    /// The OS still held the job after the bounded wait; registering anyway.
    RemovalTimedOut,
    /// Registering.
    Registering,
    /// The OS accepted the registration; waiting for the service.
    WaitingForStart,
    /// The service answers.
    Answers,
    /// `register` failed (nothing was registered); the repair goes on while cycles are left.
    RegisterFailed(String),
    /// A step did not succeed and the repair goes on.
    Note(String),
}

impl std::fmt::Display for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Step::Cycle(0) => f.write_str("Re-registering the background service"),
            Step::Cycle(n) => write!(f, "The OS still refuses the registration; retry {n}"),
            Step::Removing => f.write_str("Removing the old registration"),
            Step::WaitingForRemoval => f.write_str("Waiting for the OS to drop the old job"),
            Step::RemovalTimedOut => {
                f.write_str("The OS still holds the old job; registering anyway")
            }
            Step::Registering => f.write_str("Registering"),
            Step::WaitingForStart => {
                f.write_str("Registered; waiting for the service to start (this can take a minute)")
            }
            Step::Answers => f.write_str("The service answers again"),
            Step::RegisterFailed(why) => write!(f, "Registering failed: {why}"),
            Step::Note(text) => f.write_str(text),
        }
    }
}

/// How a repair ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepairOutcome {
    /// Registered and the service answers.
    Recovered,
    /// Registered and accepted; the service did not answer within the wait (it may still come).
    Registered,
    /// macOS needs the user's approval in Login Items.
    NeedsApproval,
    /// Administrator rights are needed.
    NeedsElevation,
    /// The user declined the OS's prompt.
    Declined,
    /// It cannot be registered from here.
    Unavailable(String),
    /// Cancelled before anything was removed (the old registration is untouched).
    Cancelled,
    /// It did not work; the text says why and what to do.
    Failed(String),
}

impl RepairOutcome {
    /// Whether the registration is in place afterwards.
    pub fn registered(&self) -> bool {
        matches!(self, RepairOutcome::Recovered | RepairOutcome::Registered)
    }
}

/// What to do next after a failure.
pub const NEXT_STEPS: &str = "Press Repair again, or restart the computer. On a Mac, if the app is listed in System Settings > General > Login Items, switch it off and on there. The app log has each step.";

fn stop_for(e: RegisterError) -> Option<RepairOutcome> {
    match e {
        RegisterError::NeedsApproval => Some(RepairOutcome::NeedsApproval),
        RegisterError::NeedsElevation => Some(RepairOutcome::NeedsElevation),
        RegisterError::Declined => Some(RepairOutcome::Declined),
        RegisterError::Unavailable(why) => Some(RepairOutcome::Unavailable(why)),
        RegisterError::Other(_) | RegisterError::NotAsWritten(_) => None,
    }
}

/// What the executor does next (the OS call or the wait the [`Stepper`] asks for).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// `unregister`.
    Unregister,
    /// `evict` (`launchctl bootout`).
    Evict,
    /// Ask whether the OS dropped the job, polling until [`Timings::gone_wait`].
    WaitGone,
    /// Pause (the backoff, or the poll while waiting for the service).
    Pause(Duration),
    /// `register`.
    Register,
    /// Pause [`Timings::settle`], then look at the job (and ask the service when `ask`: a failed registration needs it).
    Settle {
        /// Ask the service too.
        ask: bool,
    },
    /// Look at the job and ask the service.
    Look,
    /// The repair is over.
    Done(RepairOutcome),
}

/// What the OS (or the clock) answered to the last [`Action`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OsResult {
    /// `unregister` answered.
    Unregistered(Result<(), String>),
    /// `evict` answered.
    Evicted(Result<(), String>),
    /// The wait for the job to go ended; `timed_out`: the OS still held it.
    Gone {
        /// The OS still held the job after the bounded wait.
        timed_out: bool,
    },
    /// A pause ended.
    Paused,
    /// `register` was accepted.
    RegisterOk,
    /// `register` failed.
    RegisterErr(RegisterError),
    /// The look after the settle pause.
    Settled {
        /// What the OS says about the job.
        job: Job,
        /// The service answers.
        answers: bool,
        /// The caller asked to cancel.
        cancelled: bool,
    },
    /// A look during the wait for the service.
    Looked {
        /// The service answers now.
        answers: bool,
        /// What the OS says about the job now (it may be refused now).
        job: Job,
        /// Real time since the wait began.
        elapsed: Duration,
        /// The caller asked to cancel.
        cancelled: bool,
    },
}

/// The next action and what to say before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Next {
    /// Steps for the log and the UI, in order.
    pub say: Vec<Step>,
    /// What to do.
    pub action: Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Removing,
    Evicting,
    Gone,
    BeforeRegister,
    Registering,
    Settling,
    Waiting,
    Over,
}

/// The decisions of a replace as a pure state machine: given what the OS answered, what to do next (ADR 0056). The
/// executor ([`replace_bounded`]) only performs the actions; retries, the wait and the failure text are decided here, so
/// they are tested without an OS.
#[derive(Debug, Clone)]
pub struct Stepper {
    t: Timings,
    cycles: u32,
    cycle: u32,
    stage: Stage,
    last_error: Option<String>,
    last_job: Option<Job>,
    polled: Duration,
}

impl Stepper {
    /// A replace of at most `allowed` cycles (each registers once).
    pub fn new(t: &Timings, allowed: u32) -> Self {
        Self {
            t: *t,
            cycles: allowed.min(t.max_cycles + 1),
            cycle: 0,
            stage: Stage::Removing,
            last_error: None,
            last_job: None,
            polled: Duration::ZERO,
        }
    }

    /// The first action: the removal of the old registration.
    pub fn start(&mut self) -> Next {
        self.stage = Stage::Removing;
        Next {
            say: vec![Step::Cycle(self.cycle), Step::Removing],
            action: Action::Unregister,
        }
    }

    /// The first action when nothing is registered and no job is held: register, removing nothing.
    pub fn start_register_only(&mut self) -> Next {
        self.stage = Stage::Registering;
        Next {
            say: vec![Step::Cycle(self.cycle), Step::Registering],
            action: Action::Register,
        }
    }

    /// Where a replace that registered in one step goes: straight to waiting for the service.
    pub fn registered_in_one_step(&mut self) -> Next {
        self.stage = Stage::Waiting;
        Next {
            say: vec![Step::Cycle(0), Step::WaitingForStart],
            action: Action::Look,
        }
    }

    fn done(&mut self, mut say: Vec<Step>, outcome: RepairOutcome) -> Next {
        self.stage = Stage::Over;
        say.shrink_to_fit();
        Next {
            say,
            action: Action::Done(outcome),
        }
    }

    fn next_cycle(&mut self, cancelled: bool, mut say: Vec<Step>) -> Next {
        self.cycle += 1;
        // The previous cycle registered (possibly refused); nothing is left half-removed.
        if cancelled {
            return self.done(say, RepairOutcome::Cancelled);
        }
        if self.cycle >= self.cycles {
            let why = match &self.last_error {
                Some(e) => format!(
                    "Registering failed {} times ({e}). {NEXT_STEPS}",
                    self.cycles
                ),
                None => format!(
                    "The OS still refuses the service after {} full re-registrations (observed: {}). {NEXT_STEPS}",
                    self.cycles,
                    self.last_job.map_or("nothing".to_string(), Job::describe)
                ),
            };
            say.push(Step::Note(why.clone()));
            return self.done(say, RepairOutcome::Failed(why));
        }
        say.push(Step::Cycle(self.cycle));
        say.push(Step::Removing);
        self.stage = Stage::Removing;
        Next {
            say,
            action: Action::Unregister,
        }
    }

    /// The decision after `result`.
    pub fn next(&mut self, result: OsResult) -> Next {
        let go = |say: Vec<Step>, action| Next { say, action };
        match (self.stage, result) {
            (Stage::Removing, OsResult::Unregistered(r)) => {
                let say = match r {
                    Err(e) => vec![Step::Note(format!(
                        "Unregister did not succeed (going on): {e}"
                    ))],
                    Ok(()) => vec![],
                };
                self.stage = Stage::Evicting;
                go(say, Action::Evict)
            }
            (Stage::Evicting, OsResult::Evicted(r)) => {
                let mut say = match r {
                    Err(e) => vec![Step::Note(format!(
                        "Removing the job did not succeed (going on): {e}"
                    ))],
                    Ok(()) => vec![],
                };
                say.push(Step::WaitingForRemoval);
                self.stage = Stage::Gone;
                go(say, Action::WaitGone)
            }
            (Stage::Gone, OsResult::Gone { timed_out }) => {
                let say = if timed_out {
                    vec![Step::RemovalTimedOut]
                } else {
                    vec![]
                };
                if self.cycle > 0 {
                    self.stage = Stage::BeforeRegister;
                    go(say, Action::Pause(self.t.backoff * self.cycle))
                } else {
                    self.stage = Stage::Registering;
                    let mut say = say;
                    say.push(Step::Registering);
                    go(say, Action::Register)
                }
            }
            (Stage::BeforeRegister, OsResult::Paused) => {
                self.stage = Stage::Registering;
                go(vec![Step::Registering], Action::Register)
            }
            (Stage::Registering, OsResult::RegisterOk) => {
                self.last_error = None;
                self.stage = Stage::Settling;
                go(vec![], Action::Settle { ask: false })
            }
            (Stage::Registering, OsResult::RegisterErr(e)) => {
                if let Some(stop) = stop_for(e.clone()) {
                    return self.done(vec![Step::Note(format!("Registering stopped: {e}"))], stop);
                }
                self.last_error = Some(e.to_string());
                self.stage = Stage::Settling;
                go(vec![Step::RegisterFailed(e.to_string())], Action::Settle { ask: true })
            }
            (
                Stage::Settling,
                OsResult::Settled {
                    job,
                    answers,
                    cancelled,
                },
            ) => {
                self.last_job = Some(job);
                if job == Job::Refused
                    || (self.last_error.is_some() && (job == Job::Absent || !answers))
                {
                    return self.next_cycle(cancelled, vec![]);
                }
                self.stage = Stage::Waiting;
                go(vec![Step::WaitingForStart], Action::Look)
            }
            (
                Stage::Waiting,
                OsResult::Looked {
                    answers,
                    job,
                    elapsed,
                    cancelled,
                },
            ) => {
                if answers {
                    return self.done(vec![Step::Answers], RepairOutcome::Recovered);
                }
                self.last_job = Some(job);
                // The OS now refuses what it accepted a moment ago: another cycle, not a longer wait.
                if job == Job::Refused {
                    return self.next_cycle(cancelled, vec![Step::Note("The OS now refuses the registration.".into())]);
                }
                // The longer of the polls' sum (the fake clock) and the real time (a slow OS call counts too).
                let waited = self.polled.max(elapsed);
                if waited >= self.t.start_wait || cancelled {
                    return self.done(
                        vec![Step::Note(format!(
                            "The service is registered but did not answer within {} seconds; it may still start.",
                            waited.as_secs()
                        ))],
                        RepairOutcome::Registered,
                    );
                }
                self.polled += self.t.start_poll;
                go(vec![], Action::Pause(self.t.start_poll))
            }
            (Stage::Waiting, OsResult::Paused) => go(vec![], Action::Look),
            (_, other) => {
                let why = format!("internal: {other:?} in {:?}", self.stage);
                self.done(vec![Step::Note(why.clone())], RepairOutcome::Failed(why))
            }
        }
    }
}

/// Replaces the registration (see the module documentation). `answers` asks the service itself; `cancel` is honoured
/// before anything is removed and during the final wait, never between removing and registering (a cancel there would
/// leave nothing registered).
pub fn replace(
    b: &mut dyn Backend,
    answers: &mut dyn FnMut() -> bool,
    cancel: &AtomicBool,
    t: &Timings,
    say: &mut dyn FnMut(Step),
) -> RepairOutcome {
    let abort = AtomicBool::new(false);
    let bounds = Bounds {
        cancel,
        abort: &abort,
        allowed: t.max_cycles + 1,
        register_only: false,
    };
    replace_bounded(b, answers, &bounds, t, say)
}

/// How far a replace may go.
pub struct Bounds<'a> {
    /// The caller asked to cancel.
    pub cancel: &'a AtomicBool,
    /// Set by the step reporter when a step could not be recorded: no OS call that changes anything follows (no OS call
    /// without a durable write).
    pub abort: &'a AtomicBool,
    /// At most this many cycles (each registers once): an unattended replace gets what is left of the version's attempts,
    /// so a refused job costs at most [`crate::plan::MAX_ATTEMPTS`] registrations across all starts.
    pub allowed: u32,
    /// Nothing is registered and no job is held: register without removing anything first.
    pub register_only: bool,
}

/// [`replace`] within `bounds`: the executor of a [`Stepper`], which performs each action and feeds back what the OS answered.
pub fn replace_bounded(
    b: &mut dyn Backend,
    answers: &mut dyn FnMut() -> bool,
    bounds: &Bounds<'_>,
    t: &Timings,
    say: &mut dyn FnMut(Step),
) -> RepairOutcome {
    let cancel = bounds.cancel;
    if cancel.load(Ordering::Relaxed) {
        return RepairOutcome::Cancelled;
    }
    let mut stepper = Stepper::new(t, bounds.allowed);
    let mut next = if let Some(result) = b.replace_in_one_step() {
        match result {
            Ok(_) => stepper.registered_in_one_step(),
            Err(e) => {
                say(Step::Cycle(0));
                return stop_for(e.clone())
                    .unwrap_or_else(|| RepairOutcome::Failed(format!("{e}. {NEXT_STEPS}")));
            }
        }
    } else if bounds.register_only {
        stepper.start_register_only()
    } else {
        stepper.start()
    };
    let mut waiting_since: Option<std::time::Instant> = None;
    loop {
        for step in next.say.drain(..) {
            if step == Step::WaitingForStart {
                waiting_since = Some(std::time::Instant::now());
            }
            say(step);
        }
        if bounds.abort.load(Ordering::Relaxed)
            && matches!(next.action, Action::Unregister | Action::Evict | Action::Register)
        {
            return RepairOutcome::Failed(
                "The service state could not be written, so nothing more was changed. Check that the data folder is writable, then try again."
                    .to_string(),
            );
        }
        let result = match next.action {
            Action::Done(outcome) => return outcome,
            Action::Unregister => OsResult::Unregistered(b.unregister().map(drop).map_err(|e| e.to_string())),
            Action::Evict => OsResult::Evicted(b.evict()),
            Action::WaitGone => {
                let began = std::time::Instant::now();
                let mut polled = Duration::ZERO;
                let mut timed_out = false;
                while !b.job_gone() {
                    if polled.max(began.elapsed()) >= t.gone_wait {
                        timed_out = true;
                        break;
                    }
                    b.pause(t.gone_poll);
                    polled += t.gone_poll;
                }
                OsResult::Gone { timed_out }
            }
            Action::Pause(how_long) => {
                b.pause(how_long);
                OsResult::Paused
            }
            Action::Register => match b.register() {
                Ok(_) => OsResult::RegisterOk,
                Err(e) => OsResult::RegisterErr(e),
            },
            Action::Settle { ask } => {
                b.pause(t.settle);
                let job = b.inspect().job;
                OsResult::Settled {
                    job,
                    answers: ask && answers(),
                    cancelled: cancel.load(Ordering::Relaxed),
                }
            }
            Action::Look => {
                let answered = answers();
                let job = if answered { Job::Running } else { b.inspect().job };
                OsResult::Looked {
                    answers: answered,
                    job,
                    elapsed: waiting_since.map_or(Duration::ZERO, |s| s.elapsed()),
                    cancelled: cancel.load(Ordering::Relaxed),
                }
            }
        };
        next = stepper.next(result);
    }
}

#[cfg(test)]
mod tests {
    // ---- the stepper: every decision of a replace without an OS (ADR 0056)

    fn through_removal(s: &mut Stepper) -> Next {
        s.next(OsResult::Unregistered(Ok(())));
        s.next(OsResult::Evicted(Ok(())))
    }

    fn settled(job: Job, answers: bool) -> OsResult {
        OsResult::Settled {
            job,
            answers,
            cancelled: false,
        }
    }

    fn looked(answers: bool, job: Job, secs: u64) -> OsResult {
        OsResult::Looked {
            answers,
            job,
            elapsed: Duration::from_secs(secs),
            cancelled: false,
        }
    }

    #[test]
    fn a_replace_that_works_goes_remove_wait_register_settle_look_and_says_each_phase() {
        let t = Timings::default();
        let mut s = Stepper::new(&t, 4);
        let first = s.start();
        assert_eq!(first.action, Action::Unregister);
        assert_eq!(first.say, [Step::Cycle(0), Step::Removing]);
        assert_eq!(s.next(OsResult::Unregistered(Ok(()))).action, Action::Evict);
        let wait = s.next(OsResult::Evicted(Ok(())));
        assert_eq!((wait.action, wait.say), (Action::WaitGone, vec![Step::WaitingForRemoval]));
        let reg = s.next(OsResult::Gone { timed_out: false });
        assert_eq!((reg.action, reg.say), (Action::Register, vec![Step::Registering]));
        assert_eq!(s.next(OsResult::RegisterOk).action, Action::Settle { ask: false });
        let look = s.next(settled(Job::NotRunning, false));
        assert_eq!((look.action, look.say), (Action::Look, vec![Step::WaitingForStart]));
        let up = s.next(looked(true, Job::Running, 1));
        assert_eq!(up.action, Action::Done(RepairOutcome::Recovered));
        assert_eq!(up.say, [Step::Answers]);
    }

    #[test]
    fn a_failed_unregister_or_evict_is_said_and_the_replace_goes_on() {
        let mut s = Stepper::new(&Timings::default(), 4);
        s.start();
        let evict = s.next(OsResult::Unregistered(Err("busy".into())));
        assert_eq!(evict.action, Action::Evict);
        assert!(matches!(&evict.say[0], Step::Note(n) if n.contains("Unregister did not succeed") && n.contains("busy")));
        let wait = s.next(OsResult::Evicted(Err("no".into())));
        assert!(matches!(&wait.say[0], Step::Note(n) if n.contains("Removing the job did not succeed")));
        assert_eq!(wait.action, Action::WaitGone);
        let timed_out = s.next(OsResult::Gone { timed_out: true });
        assert_eq!(timed_out.say[0], Step::RemovalTimedOut);
        assert_eq!(timed_out.action, Action::Register, "registers anyway");
    }

    #[test]
    fn a_refusal_starts_the_next_cycle_with_a_backoff_until_the_cycles_are_used_up() {
        let t = Timings::default();
        let mut s = Stepper::new(&t, 2);
        s.start();
        through_removal(&mut s);
        s.next(OsResult::Gone { timed_out: false });
        s.next(OsResult::RegisterOk);
        let again = s.next(settled(Job::Refused, false));
        assert_eq!(again.action, Action::Unregister);
        assert_eq!(again.say, [Step::Cycle(1), Step::Removing]);
        through_removal(&mut s);
        let backoff = s.next(OsResult::Gone { timed_out: false });
        assert_eq!(backoff.action, Action::Pause(t.backoff), "waits longer each cycle");
        assert_eq!(s.next(OsResult::Paused).action, Action::Register);
        s.next(OsResult::RegisterOk);
        let over = s.next(settled(Job::Refused, false));
        let Action::Done(RepairOutcome::Failed(why)) = over.action else {
            panic!("{over:?}")
        };
        assert!(why.contains("after 2 full re-registrations") && why.contains("refuses to start it"), "{why}");
    }

    #[test]
    fn a_register_that_failed_is_said_retried_and_ends_with_its_reason() {
        let mut s = Stepper::new(&Timings::default(), 1);
        s.start();
        through_removal(&mut s);
        s.next(OsResult::Gone { timed_out: false });
        let failed = s.next(OsResult::RegisterErr(RegisterError::Other("denied".into())));
        assert_eq!(failed.action, Action::Settle { ask: true });
        assert_eq!(failed.say, [Step::RegisterFailed("denied".into())]);
        // Nothing was registered (the job is gone): the cycle failed, and with no cycle left the repair says why.
        let over = s.next(settled(Job::Absent, false));
        let Action::Done(RepairOutcome::Failed(why)) = over.action else {
            panic!("{over:?}")
        };
        assert!(why.contains("Registering failed 1 times (denied)"), "{why}");
    }

    #[test]
    fn what_retrying_cannot_help_ends_the_replace_with_its_own_outcome() {
        for (error, outcome) in [
            (RegisterError::NeedsApproval, RepairOutcome::NeedsApproval),
            (RegisterError::NeedsElevation, RepairOutcome::NeedsElevation),
            (RegisterError::Declined, RepairOutcome::Declined),
            (RegisterError::Unavailable("w".into()), RepairOutcome::Unavailable("w".into())),
        ] {
            let mut s = Stepper::new(&Timings::default(), 4);
            s.start();
            through_removal(&mut s);
            s.next(OsResult::Gone { timed_out: false });
            assert_eq!(s.next(OsResult::RegisterErr(error)).action, Action::Done(outcome));
        }
    }

    #[test]
    fn the_wait_for_the_service_is_bounded_cancellable_and_notices_a_refusal() {
        let t = Timings::default();
        let to_wait = || {
            let mut s = Stepper::new(&t, 4);
            s.start();
            through_removal(&mut s);
            s.next(OsResult::Gone { timed_out: false });
            s.next(OsResult::RegisterOk);
            s.next(settled(Job::NotRunning, false));
            s
        };
        // It polls, then gives up with the seconds it waited.
        let mut s = to_wait();
        assert_eq!(s.next(looked(false, Job::NotRunning, 0)).action, Action::Pause(t.start_poll));
        assert_eq!(s.next(OsResult::Paused).action, Action::Look);
        let late = s.next(looked(false, Job::NotRunning, t.start_wait.as_secs()));
        assert_eq!(late.action, Action::Done(RepairOutcome::Registered));
        assert!(matches!(&late.say[0], Step::Note(n) if n.contains("did not answer within 120 seconds")), "{:?}", late.say);
        // Real time counts as much as the polls: a slow OS call cannot stretch the wait.
        let mut s = to_wait();
        let slow = s.next(looked(false, Job::NotRunning, 500));
        assert_eq!(slow.action, Action::Done(RepairOutcome::Registered));
        // A cancel ends the wait (the registration stays).
        let mut s = to_wait();
        let cancelled = s.next(OsResult::Looked {
            answers: false,
            job: Job::NotRunning,
            elapsed: Duration::ZERO,
            cancelled: true,
        });
        assert_eq!(cancelled.action, Action::Done(RepairOutcome::Registered));
        // A refusal seen during the wait is another cycle, not a longer wait.
        let mut s = to_wait();
        assert_eq!(s.next(looked(false, Job::Refused, 3)).action, Action::Unregister);
        // A cancel at the cycle boundary leaves nothing half removed.
        let mut s = to_wait();
        let over = s.next(OsResult::Looked {
            answers: false,
            job: Job::Refused,
            elapsed: Duration::ZERO,
            cancelled: true,
        });
        assert_eq!(over.action, Action::Done(RepairOutcome::Cancelled));
    }

    #[test]
    fn a_step_out_of_order_is_a_failure_not_a_panic() {
        let mut s = Stepper::new(&Timings::default(), 1);
        s.start();
        let odd = s.next(OsResult::Paused);
        assert!(matches!(odd.action, Action::Done(RepairOutcome::Failed(_))));
    }

    use super::*;
    use crate::backend::{Capabilities, Inspection, Mechanism};

    /// A scripted backend: the job is refused for `refused_for` checks, lingers for `lingers` gone-checks after each
    /// eviction, and the service answers after `answers_after` asks.
    #[derive(Default)]
    struct Script {
        calls: Vec<&'static str>,
        refused_for: u32,
        lingers: u32,
        lingering: u32,
        register: Vec<Result<(), RegisterError>>,
        one_step: Option<Result<(), RegisterError>>,
        unregister_fails: bool,
        evict_fails: bool,
        paused: Duration,
        job_after_failed_register: Option<Job>,
        failed_last: bool,
    }

    impl Backend for Script {
        fn mechanism(&self) -> Mechanism {
            Mechanism::LaunchdBundled
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                automatic: true,
                pinned_identity: true,
                login_items: true,
            }
        }
        fn inspect(&mut self) -> Inspection {
            self.calls.push("inspect");
            let job = if self.failed_last && self.job_after_failed_register.is_some() {
                self.job_after_failed_register.unwrap_or(Job::Absent)
            } else if self.refused_for > 0 {
                self.refused_for -= 1;
                Job::Refused
            } else {
                Job::Running
            };
            Inspection {
                job,
                ..Inspection::absent()
            }
        }
        fn register(&mut self) -> Result<Inspection, RegisterError> {
            self.calls.push("register");
            let r = if self.register.is_empty() {
                Ok(())
            } else {
                self.register.remove(0)
            };
            self.failed_last = r.is_err();
            r.map(|()| Inspection::absent())
        }
        fn unregister(&mut self) -> Result<Inspection, crate::backend::UnregisterError> {
            self.calls.push("unregister");
            if self.unregister_fails {
                Err(crate::backend::UnregisterError::Failed(
                    "not registered".into(),
                ))
            } else {
                Ok(Inspection::absent())
            }
        }
        fn evict(&mut self) -> Result<(), String> {
            self.calls.push("bootout");
            self.lingering = self.lingers;
            if self.evict_fails {
                Err("timed out".into())
            } else {
                Ok(())
            }
        }
        fn job_gone(&mut self) -> bool {
            if self.lingering > 0 {
                self.lingering -= 1;
                false
            } else {
                true
            }
        }
        fn pause(&mut self, d: Duration) {
            self.paused += d;
        }
        fn replace_in_one_step(&mut self) -> Option<Result<Inspection, RegisterError>> {
            self.one_step
                .take()
                .map(|r| r.map(|()| Inspection::absent()))
        }
    }

    fn run(s: &mut Script, answers_after: Option<u32>) -> (RepairOutcome, Vec<String>) {
        let mut asked = 0;
        let mut answers = || {
            asked += 1;
            answers_after.is_some_and(|n| asked > n)
        };
        let mut said = Vec::new();
        let out = replace(
            s,
            &mut answers,
            &AtomicBool::new(false),
            &Timings::default(),
            &mut |st| said.push(st.to_string()),
        );
        (out, said)
    }

    const CYCLE: [&str; 4] = ["unregister", "bootout", "register", "inspect"];

    #[test]
    fn the_old_job_is_removed_and_gone_before_registering() {
        let mut s = Script {
            lingers: 3,
            ..Script::default()
        };
        let (out, said) = run(&mut s, Some(0));
        assert_eq!(out, RepairOutcome::Recovered);
        assert_eq!(s.calls, CYCLE);
        assert_eq!(s.paused, Duration::from_secs(3) + Timings::default().settle);
        assert_eq!(
            said.first().map(String::as_str),
            Some("Re-registering the background service")
        );
        assert!(said.iter().any(|m| m.contains("answers again")));
    }

    #[test]
    fn a_bootout_that_hangs_and_a_job_that_never_goes_are_waited_for_a_bounded_time() {
        let mut s = Script {
            lingers: u32::MAX,
            evict_fails: true,
            ..Script::default()
        };
        let (out, said) = run(&mut s, Some(0));
        assert_eq!(out, RepairOutcome::Recovered);
        let t = Timings::default();
        assert_eq!(s.paused, t.gone_wait + t.settle);
        assert!(
            said.iter()
                .any(|m| m.contains("timed out") && m.contains("going on"))
        );
        assert!(said.iter().any(|m| m.contains("still holds the old job")));
    }

    #[test]
    fn a_stale_registration_is_retried_with_a_full_cycle_and_a_growing_pause() {
        let mut s = Script {
            refused_for: 2,
            ..Script::default()
        };
        let (out, said) = run(&mut s, Some(0));
        assert_eq!(out, RepairOutcome::Recovered);
        assert_eq!(s.calls, [CYCLE, CYCLE, CYCLE].concat());
        assert!(
            said.iter().any(|m| m.contains("retry 1"))
                && said.iter().any(|m| m.contains("retry 2"))
        );
        let t = Timings::default();
        assert_eq!(s.paused, t.settle * 3 + t.backoff * (1 + 2));
    }

    #[test]
    fn a_registration_that_stays_refused_fails_after_the_bounded_retries() {
        let mut s = Script {
            refused_for: u32::MAX,
            unregister_fails: true,
            ..Script::default()
        };
        let (out, said) = run(&mut s, Some(0));
        let RepairOutcome::Failed(why) = out else {
            panic!("{out:?}")
        };
        assert!(
            why.contains("still refuses") && why.contains("4 full") && why.contains("Repair again"),
            "{why}"
        );
        let t = Timings::default();
        assert_eq!(
            s.calls.iter().filter(|c| **c == "register").count(),
            t.max_cycles as usize + 1
        );
        assert!(
            said.iter()
                .any(|m| m.contains("Unregister did not succeed"))
        );
        assert!(s.paused <= t.worst_case());
    }

    #[test]
    fn the_service_gets_a_bounded_time_to_answer() {
        let t = Timings::default();
        let mut s = Script::default();
        assert_eq!(run(&mut s, Some(20)).0, RepairOutcome::Recovered);
        assert_eq!(s.paused, t.settle + t.start_poll * 20);
        let mut s = Script::default();
        let (out, said) = run(&mut s, None);
        assert_eq!(out, RepairOutcome::Registered);
        assert!(out.registered() && !RepairOutcome::Cancelled.registered());
        assert!(
            said.iter()
                .any(|m| m.contains("did not answer within 120 seconds"))
        );
        assert!(s.paused <= t.settle + t.start_wait + t.start_poll);
    }

    #[test]
    fn approval_elevation_decline_and_unavailable_stop_at_once_other_errors_are_retried() {
        for (e, want) in [
            (RegisterError::NeedsApproval, RepairOutcome::NeedsApproval),
            (RegisterError::NeedsElevation, RepairOutcome::NeedsElevation),
            (RegisterError::Declined, RepairOutcome::Declined),
            (
                RegisterError::Unavailable("no bus".into()),
                RepairOutcome::Unavailable("no bus".into()),
            ),
        ] {
            let mut s = Script {
                register: vec![Err(e)],
                ..Script::default()
            };
            assert_eq!(run(&mut s, Some(0)).0, want);
            assert_eq!(s.calls, ["unregister", "bootout", "register"]);
        }
        // Register refused (busy) leaves no job: retried, then works.
        let mut s = Script {
            register: vec![Err(RegisterError::Other("busy".into()))],
            job_after_failed_register: Some(Job::Absent),
            ..Script::default()
        };
        let (out, said) = run(&mut s, Some(0));
        assert_eq!(out, RepairOutcome::Recovered);
        assert!(said.iter().any(|m| m.contains("Registering failed: busy")));
        assert_eq!(s.calls.iter().filter(|c| **c == "register").count(), 2);
        // Register always refused: fails with the OS's words.
        let mut s = Script {
            register: vec![Err(RegisterError::Other("busy".into())); 9],
            job_after_failed_register: Some(Job::Absent),
            ..Script::default()
        };
        let RepairOutcome::Failed(why) = run(&mut s, Some(0)).0 else {
            panic!()
        };
        assert!(why.contains("Registering failed 4 times (busy)"), "{why}");
    }

    #[test]
    fn a_cancel_before_the_start_changes_nothing_and_a_cancel_later_never_leaves_it_unregistered() {
        let mut s = Script::default();
        let out = replace(
            &mut s,
            &mut || true,
            &AtomicBool::new(true),
            &Timings::default(),
            &mut |_| {},
        );
        assert_eq!(out, RepairOutcome::Cancelled);
        assert!(s.calls.is_empty());
        // Cancelled while refused: the cycle that ran registered again before stopping.
        let mut s = Script {
            refused_for: u32::MAX,
            ..Script::default()
        };
        let cancel = AtomicBool::new(false);
        let mut said = 0;
        let out = replace(
            &mut s,
            &mut || false,
            &cancel,
            &Timings::default(),
            &mut |st| {
                if st == Step::Registering {
                    said += 1;
                    cancel.store(true, Ordering::Relaxed);
                }
            },
        );
        assert_eq!(out, RepairOutcome::Cancelled);
        assert_eq!(
            s.calls, CYCLE,
            "the last call before stopping is a registration"
        );
        // Cancelled while waiting for the answer: the registration stays.
        let mut s = Script::default();
        let cancel = AtomicBool::new(false);
        let out = replace(
            &mut s,
            &mut || false,
            &cancel,
            &Timings::default(),
            &mut |st| {
                if st == Step::WaitingForStart {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
        );
        assert_eq!(out, RepairOutcome::Registered);
        assert_eq!(said, 1);
    }

    #[test]
    fn a_one_step_replacement_is_used_when_the_backend_has_one() {
        let mut s = Script {
            one_step: Some(Ok(())),
            ..Script::default()
        };
        assert_eq!(run(&mut s, Some(0)).0, RepairOutcome::Recovered);
        assert!(
            s.calls.is_empty(),
            "no separate unregister and register: one prompt"
        );
        assert_eq!(s.mechanism(), Mechanism::LaunchdBundled);
        assert!(s.capabilities().pinned_identity);
        let mut s = Script {
            one_step: Some(Err(RegisterError::Declined)),
            ..Script::default()
        };
        assert_eq!(run(&mut s, Some(0)).0, RepairOutcome::Declined);
        let mut s = Script {
            one_step: Some(Err(RegisterError::Other("exit code 1".into()))),
            ..Script::default()
        };
        let RepairOutcome::Failed(why) = run(&mut s, Some(0)).0 else {
            panic!()
        };
        assert!(why.contains("exit code 1"));
    }

    #[test]
    fn every_step_has_words_and_the_worst_case_is_bounded() {
        for s in [
            Step::Cycle(0),
            Step::Cycle(2),
            Step::Removing,
            Step::WaitingForRemoval,
            Step::RemovalTimedOut,
            Step::Registering,
            Step::WaitingForStart,
            Step::Answers,
            Step::Note("x".into()),
        ] {
            assert!(!s.to_string().is_empty());
        }
        assert!(Timings::default().worst_case() < Duration::from_secs(300));
    }
}
