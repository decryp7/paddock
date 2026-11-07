//! Replacing a registration (rule 24): unregister → evict (`launchctl bootout`) → wait until the OS has dropped the job
//! (bounded) → register → check that the OS accepts it → wait for the service to answer (bounded). launchd keeps the old
//! build's launch constraint while the job lives, so registering over a live job does nothing (R03); the cycle repeats
//! a bounded number of times, waiting longer each time. Every step is said in words ([`Step`], phase-labelled progress
//! for the log and the UI); waits go through the backend's port, so tests run them on a fake clock.

use crate::backend::{Backend, Job};
use crate::port::RegisterError;
use crate::start_wait::{StartObservation, StartVerdict, StartWatch};
use paddock_health::Clock;
use paddock_proc::{ProcessId, StartTime};
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
    /// The state machine's own backstop for a **judged** wait ([`StartWatch`]): after this many looks it ends, whatever the
    /// watch says. A job that was seen running ends `Registered` ("it may still start"), one that never ran ends `Failed`. It is
    /// counted in looks, not seconds. The default is a few looks past [`crate::start_wait::START_CEILING_POLLS`], so the
    /// watch's own, friendlier message comes first; a model shrinks both.
    pub judged_ceiling_polls: u32,
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
            judged_ceiling_polls: crate::start_wait::START_CEILING_POLLS + 5,
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
        // The elevated step may still finish: no second prompt, and the words say it was not stopped.
        RegisterError::StillRunning(why) => Some(RepairOutcome::Unavailable(format!(
            "The elevated step is still running and was not stopped; it may still finish ({why})."
        ))),
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
        /// The process that answers, when the backend can name it (rule 53); `None` when it cannot.
        process: Option<ProcessId>,
        /// What the [`StartWatch`] made of this look (ADR 0056, amendment "the wait is judged"); `None`: no watch, or the
        /// backend cannot observe the job, so the wait is bounded by [`Timings::start_wait`].
        verdict: Option<StartVerdict>,
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
    /// The last register call failed and the OS did not take the write (`NotAsWritten` is the one failure that may have landed).
    register_lost: bool,
    last_job: Option<Job>,
    polled: Duration,
    /// Looks of a judged wait so far (for [`Timings::judged_ceiling_polls`]).
    judged_polls: u32,
    /// The job was seen running, apart from an old build's that answers, on some look of this wait.
    saw_running: bool,
    /// When the last register began (the executor's clock); a process that started before it is an old build's.
    registered_at: Option<StartTime>,
    /// The answering process of the previous look; a new one settles only when two looks name it.
    seen: Option<ProcessId>,
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
            register_lost: false,
            last_job: None,
            polled: Duration::ZERO,
            judged_polls: 0,
            saw_running: false,
            registered_at: None,
            seen: None,
        }
    }

    /// The executor tells when it began a register, so a later look can tell a new process from a lingering one.
    pub fn registered_at(&mut self, at: StartTime) {
        self.registered_at = Some(at);
        self.seen = None;
    }

    /// Whether the answer comes from the process this register started (rule 53): one that started before the register is
    /// an old build's job still running, and a new one must be named by two looks in a row (a crash loop shows `Running` once).
    /// A backend that cannot name the process leaves the answer alone (the executor logs that the check could not run).
    fn new_process_answers(&mut self, process: Option<ProcessId>) -> bool {
        let (Some(p), Some(at)) = (process, self.registered_at) else {
            return true;
        };
        if p.started() < at {
            self.seen = None;
            return false;
        }
        let same = self.seen == Some(p);
        self.seen = Some(p);
        same
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
                self.register_lost = false;
                self.stage = Stage::Settling;
                go(vec![], Action::Settle { ask: false })
            }
            (Stage::Registering, OsResult::RegisterErr(e)) => {
                if let Some(stop) = stop_for(e.clone()) {
                    return self.done(vec![Step::Note(format!("Registering stopped: {e}"))], stop);
                }
                self.last_error = Some(e.to_string());
                // The old job that still answers must not make a registration that failed look settled.
                self.register_lost = !matches!(e, RegisterError::NotAsWritten(_));
                self.stage = Stage::Settling;
                go(
                    vec![Step::RegisterFailed(e.to_string())],
                    Action::Settle { ask: true },
                )
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
                    || self.register_lost
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
                    process,
                    verdict,
                },
            ) => {
                if answers && self.new_process_answers(process) {
                    return self.done(vec![Step::Answers], RepairOutcome::Recovered);
                }
                self.last_job = Some(job);
                if job == Job::Running && !answers {
                    self.saw_running = true;
                }
                // The OS now refuses what it accepted a moment ago: another cycle, not a longer wait.
                if job == Job::Refused {
                    return self.next_cycle(
                        cancelled,
                        vec![Step::Note("The OS now refuses the registration.".into())],
                    );
                }
                // A caller that must return at once ([`Timings::without_start_wait`]) is not made to wait by a judge.
                let verdict = verdict.filter(|_| !self.t.start_wait.is_zero());
                // A cancel ends the wait before a verdict that ends it for the judge does (ADR 0056 amendment).
                let verdict = match verdict {
                    Some(StartVerdict::Failed(_) | StartVerdict::GaveUp(_)) if cancelled => {
                        Some(StartVerdict::Waiting)
                    }
                    other => other,
                };
                let mut say = vec![];
                let judged = verdict.is_some();
                match verdict {
                    // A watch is told `answers: false` only (the answer is established above, with the process's identity), so
                    // `Up` cannot come from one; if a watch says it anyway it is one more poll, never a settled service.
                    None | Some(StartVerdict::Waiting | StartVerdict::Up) => {}
                    Some(StartVerdict::StillWorking(note)) => say.push(Step::Note(note)),
                    // The OS refuses the job it accepted: another cycle, like a refusal seen at once.
                    Some(StartVerdict::Refused(seen)) => {
                        return self.next_cycle(
                            cancelled,
                            vec![Step::Note(format!(
                                "The OS refuses the registration ({seen}); registering again."
                            ))],
                        );
                    }
                    Some(StartVerdict::Failed(why)) => {
                        let why = format!(
                            "The service was registered but did not start: {why}. {NEXT_STEPS}"
                        );
                        return self
                            .done(vec![Step::Note(why.clone())], RepairOutcome::Failed(why));
                    }
                    // The judge stops waiting for a job that ran and is not stuck: not a failure, so no advice to repair again
                    // (ER-19: a running job is never abandoned as broken).
                    Some(StartVerdict::GaveUp(why)) => {
                        return self.done(
                            vec![Step::Note(format!("The service is registered; {why}."))],
                            RepairOutcome::Registered,
                        );
                    }
                }
                // The longer of the polls' sum (the fake clock) and the real time (a slow OS call counts too). A judged wait
                // ends by the judge's own verdict, never by this bound; a cancel ends either.
                let waited = self.polled.max(elapsed);
                if (!judged && waited >= self.t.start_wait) || cancelled {
                    return self.done(
                        vec![Step::Note(format!(
                            "The service is registered but did not answer within {} seconds; it may still start.",
                            waited.as_secs()
                        ))],
                        RepairOutcome::Registered,
                    );
                }
                if judged {
                    // The backstop of a judged wait, whatever the watch says: counted in looks, never in seconds.
                    self.judged_polls += 1;
                    if self.judged_polls >= self.t.judged_ceiling_polls {
                        let n = self.judged_polls;
                        return if self.saw_running {
                            self.done(
                                vec![Step::Note(format!(
                                    "The service is registered and still starting after {n} checks; it may still start."
                                ))],
                                RepairOutcome::Registered,
                            )
                        } else {
                            let why = format!(
                                "The service was registered but never started after {n} checks (observed: {}). {NEXT_STEPS}",
                                job.describe()
                            );
                            self.done(vec![Step::Note(why.clone())], RepairOutcome::Failed(why))
                        };
                    }
                }
                self.polled += self.t.start_poll;
                go(say, Action::Pause(self.t.start_poll))
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
        clock: &paddock_health::SystemClock,
    };
    replace_bounded(b, answers, &bounds, t, None, &mut || {}, &mut || true, say)
}

/// The wall clock as a process start time (microseconds since the Unix epoch).
pub fn wall_clock() -> StartTime {
    start_time_of(std::time::SystemTime::now())
}

/// A calendar time as a process start time (microseconds since the Unix epoch).
pub fn start_time_of(t: std::time::SystemTime) -> StartTime {
    let t = t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    StartTime::from_micros(u64::try_from(t.as_micros()).unwrap_or(u64::MAX))
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
    /// The clock of the waits (how long the OS has been given) and of `registered_at`, whose calendar reading is stamped in
    /// the units of a process start time ([`start_time_of`]). A model passes one that stands still, so no system clock is
    /// read in its hot loop.
    pub clock: &'a dyn Clock,
}

/// [`replace`] within `bounds`: the executor of a [`Stepper`], which performs each action and feeds back what the OS answered.
/// `watch` judges the wait for the service (ADR 0056 amendment); `before_wait` and `after_wait` are the lock hooks of the wait.
// The three hooks are separate closures of different callers; bundling them in a struct would only rename the arguments.
#[allow(clippy::too_many_arguments)]
pub fn replace_bounded<'w>(
    b: &mut dyn Backend,
    answers: &mut dyn FnMut() -> bool,
    bounds: &Bounds<'_>,
    t: &Timings,
    mut watch: Option<&mut (dyn StartWatch + 'w)>,
    before_wait: &mut dyn FnMut(),
    after_wait: &mut dyn FnMut() -> bool,
    say: &mut dyn FnMut(Step),
) -> RepairOutcome {
    let cancel = bounds.cancel;
    if cancel.load(Ordering::Relaxed) {
        return RepairOutcome::Cancelled;
    }
    let mut stepper = Stepper::new(t, bounds.allowed);
    let at = start_time_of(bounds.clock.wall());
    let mut next = if let Some(result) = b.replace_in_one_step() {
        stepper.registered_at(at);
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
    let mut waiting_since: Option<Duration> = None;
    // The caller let go of what it holds for the wait (`before_wait`) and has not taken it back since.
    let mut let_go = false;
    loop {
        // A later cycle (the OS refused what it had accepted) changes the OS again: whatever was let go of for the wait is taken
        // back first, and the caller says whether the record still shows this replace (ADR 0056 section 3: the lock is taken again
        // to act). Before the steps are said, which write the record.
        if let_go
            && matches!(
                next.action,
                Action::Unregister | Action::Evict | Action::Register
            )
        {
            let_go = false;
            if !after_wait() {
                return RepairOutcome::Cancelled;
            }
        }
        for step in next.say.drain(..) {
            if step == Step::WaitingForStart {
                waiting_since = Some(bounds.clock.monotonic());
                // Each registration is a new wait: the watch forgets what the last one saw.
                if let Some(w) = watch.as_deref_mut() {
                    w.reset();
                }
                // The wait is long and changes nothing: the caller lets go of what it holds for it.
                before_wait();
                let_go = true;
            }
            say(step);
        }
        if bounds.abort.load(Ordering::Relaxed)
            && matches!(
                next.action,
                Action::Unregister | Action::Evict | Action::Register
            )
        {
            return RepairOutcome::Failed(
                "The service state could not be written, so nothing more was changed. Check that the data folder is writable, then try again."
                    .to_string(),
            );
        }
        let result = match next.action {
            Action::Done(outcome) => return outcome,
            Action::Unregister => {
                OsResult::Unregistered(b.unregister().map(drop).map_err(|e| e.to_string()))
            }
            Action::Evict => OsResult::Evicted(b.evict()),
            Action::WaitGone => {
                let began = bounds.clock.monotonic();
                let mut polled = Duration::ZERO;
                let mut timed_out = false;
                while !b.job_gone() {
                    if polled.max(bounds.clock.monotonic().saturating_sub(began)) >= t.gone_wait {
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
            Action::Register => {
                stepper.registered_at(start_time_of(bounds.clock.wall()));
                match b.register() {
                    Ok(_) => OsResult::RegisterOk,
                    Err(e) => OsResult::RegisterErr(e),
                }
            }
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
                // One look at the OS per poll: when the watch can see the job, those facts are the look (the job's state
                // comes from them) and `inspect` is not asked again (ER-20). A caller that must return at once is not judged.
                // The look at which the service answers is observed too: until the answering process is confirmed (two looks name
                // it, rule 53) the wait goes on, and it must stay a judged one, not fall back to the fixed bound.
                let facts = if !t.start_wait.is_zero() && watch.is_some() {
                    b.observe_start()
                } else {
                    None
                };
                let verdict = match (watch.as_deref_mut(), facts) {
                    (Some(w), Some(facts)) => {
                        let verdict = w.poll(&StartObservation {
                            answers: false,
                            facts,
                        });
                        log::info!(verdict:? = verdict; "service repair: the start watch judged the look");
                        Some(verdict)
                    }
                    _ => None,
                };
                let job = if answered {
                    Job::Running
                } else if let Some(f) = facts {
                    f.job
                } else {
                    b.inspect().job
                };
                let process = if answered { b.job_process() } else { None };
                log::info!(answers = answered, pid:? = process.map(|p| p.pid()); "service repair: look at the service (the pid is absent when the backend cannot name it)");
                OsResult::Looked {
                    answers: answered,
                    process,
                    verdict,
                    job,
                    elapsed: waiting_since.map_or(Duration::ZERO, |s| {
                        bounds.clock.monotonic().saturating_sub(s)
                    }),
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
            process: None,
            verdict: None,
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
        assert_eq!(
            (wait.action, wait.say),
            (Action::WaitGone, vec![Step::WaitingForRemoval])
        );
        let reg = s.next(OsResult::Gone { timed_out: false });
        assert_eq!(
            (reg.action, reg.say),
            (Action::Register, vec![Step::Registering])
        );
        assert_eq!(
            s.next(OsResult::RegisterOk).action,
            Action::Settle { ask: false }
        );
        let look = s.next(settled(Job::NotRunning, false));
        assert_eq!(
            (look.action, look.say),
            (Action::Look, vec![Step::WaitingForStart])
        );
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
        assert!(
            matches!(&evict.say[0], Step::Note(n) if n.contains("Unregister did not succeed") && n.contains("busy"))
        );
        let wait = s.next(OsResult::Evicted(Err("no".into())));
        assert!(
            matches!(&wait.say[0], Step::Note(n) if n.contains("Removing the job did not succeed"))
        );
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
        assert_eq!(
            backoff.action,
            Action::Pause(t.backoff),
            "waits longer each cycle"
        );
        assert_eq!(s.next(OsResult::Paused).action, Action::Register);
        s.next(OsResult::RegisterOk);
        let over = s.next(settled(Job::Refused, false));
        let Action::Done(RepairOutcome::Failed(why)) = over.action else {
            panic!("{over:?}")
        };
        assert!(
            why.contains("after 2 full re-registrations") && why.contains("refuses to start it"),
            "{why}"
        );
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
    fn a_failed_register_is_never_settled_by_the_old_job_that_still_answers_but_a_write_read_back_wrong_may_be()
     {
        let to_settle = |error: RegisterError| {
            let mut s = Stepper::new(&Timings::default(), 4);
            s.start();
            through_removal(&mut s);
            s.next(OsResult::Gone { timed_out: false });
            s.next(OsResult::RegisterErr(error));
            s
        };
        let lost =
            to_settle(RegisterError::Other("denied".into())).next(settled(Job::Running, true));
        assert_eq!(
            lost.action,
            Action::Unregister,
            "the old job answers, the new registration is not there"
        );
        let maybe = to_settle(RegisterError::NotAsWritten(Inspection::absent()))
            .next(settled(Job::Running, true));
        assert_eq!(
            maybe.action,
            Action::Look,
            "the write may have landed: look for the service"
        );
    }

    #[test]
    fn what_retrying_cannot_help_ends_the_replace_with_its_own_outcome() {
        for (error, outcome) in [
            (RegisterError::NeedsApproval, RepairOutcome::NeedsApproval),
            (RegisterError::NeedsElevation, RepairOutcome::NeedsElevation),
            (RegisterError::Declined, RepairOutcome::Declined),
            (
                RegisterError::Unavailable("w".into()),
                RepairOutcome::Unavailable("w".into()),
            ),
        ] {
            let mut s = Stepper::new(&Timings::default(), 4);
            s.start();
            through_removal(&mut s);
            s.next(OsResult::Gone { timed_out: false });
            assert_eq!(
                s.next(OsResult::RegisterErr(error)).action,
                Action::Done(outcome)
            );
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
        assert_eq!(
            s.next(looked(false, Job::NotRunning, 0)).action,
            Action::Pause(t.start_poll)
        );
        assert_eq!(s.next(OsResult::Paused).action, Action::Look);
        let late = s.next(looked(false, Job::NotRunning, t.start_wait.as_secs()));
        assert_eq!(late.action, Action::Done(RepairOutcome::Registered));
        assert!(
            matches!(&late.say[0], Step::Note(n) if n.contains("did not answer within 120 seconds")),
            "{:?}",
            late.say
        );
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
            process: None,
            verdict: None,
        });
        assert_eq!(cancelled.action, Action::Done(RepairOutcome::Registered));
        // A refusal seen during the wait is another cycle, not a longer wait.
        let mut s = to_wait();
        assert_eq!(
            s.next(looked(false, Job::Refused, 3)).action,
            Action::Unregister
        );
        // A cancel at the cycle boundary leaves nothing half removed.
        let mut s = to_wait();
        let over = s.next(OsResult::Looked {
            answers: false,
            job: Job::Refused,
            elapsed: Duration::ZERO,
            cancelled: true,
            process: None,
            verdict: None,
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
        /// The first this many checks find the job running before the refusal begins (the OS refuses it during the wait).
        quiet_for: u32,
        lingers: u32,
        lingering: u32,
        register: Vec<Result<(), RegisterError>>,
        one_step: Option<Result<(), RegisterError>>,
        unregister_fails: bool,
        evict_fails: bool,
        paused: Duration,
        job_after_failed_register: Option<Job>,
        failed_last: bool,
        /// The process the job runs, as `job_process` names it.
        process: Option<ProcessId>,
        /// What `observe_start` shows, one per look (the last one repeats); `None`: the backend cannot observe.
        facts: Option<Vec<crate::backend::StartFacts>>,
        looks: usize,
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
            } else if self.quiet_for > 0 {
                self.quiet_for -= 1;
                Job::Running
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
        fn job_process(&mut self) -> Option<ProcessId> {
            self.process
        }
        fn observe_start(&mut self) -> Option<crate::backend::StartFacts> {
            self.calls.push("observe");
            let facts = self.facts.as_ref()?;
            let at = self.looks.min(facts.len() - 1);
            self.looks += 1;
            Some(facts[at])
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
            // An elevated step that has not ended stops the repair with its words: another try would ask the user again.
            (
                RegisterError::StillRunning(
                    "the program was still running after 15 minutes".into(),
                ),
                RepairOutcome::Unavailable(
                    "The elevated step is still running and was not stopped; it may still finish (the program was still running after 15 minutes).".into(),
                ),
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

    // ---- which process answers (rule 53): an old build's job that lingers never settles a new version ----
    const REGISTERED: u64 = 100;
    fn proc(pid: u32, started: u64) -> ProcessId {
        ProcessId::new(pid, StartTime::from_micros(started))
    }
    fn answered_by(process: Option<ProcessId>) -> OsResult {
        OsResult::Looked {
            answers: true,
            job: Job::Running,
            elapsed: Duration::ZERO,
            cancelled: false,
            process,
            verdict: None,
        }
    }
    fn waiting_after_register() -> Stepper {
        let mut s = Stepper::new(&Timings::default(), 4);
        s.start_register_only();
        s.registered_at(StartTime::from_micros(REGISTERED));
        s.next(OsResult::RegisterOk);
        s.next(settled(Job::NotRunning, false));
        s
    }

    #[test]
    fn an_old_job_that_stays_running_does_not_settle_the_new_version() {
        let mut s = waiting_after_register();
        for _ in 0..3 {
            let n = s.next(answered_by(Some(proc(7, REGISTERED - 50))));
            assert!(!matches!(n.action, Action::Done(_)), "{n:?}");
        }
    }

    #[test]
    fn a_crash_loop_that_reads_running_once_does_not_settle() {
        let mut s = waiting_after_register();
        for pid in 20..24 {
            let n = s.next(answered_by(Some(proc(pid, REGISTERED + u64::from(pid)))));
            assert!(
                !matches!(n.action, Action::Done(RepairOutcome::Recovered)),
                "pid {pid}: {n:?}"
            );
        }
    }

    #[test]
    fn one_new_process_seen_on_two_looks_settles() {
        let mut s = waiting_after_register();
        let new = proc(30, REGISTERED + 5);
        assert_eq!(
            s.next(answered_by(Some(new))).action,
            Action::Pause(Timings::default().start_poll)
        );
        let n = s.next(answered_by(Some(new)));
        assert_eq!(n.action, Action::Done(RepairOutcome::Recovered));
    }

    #[test]
    fn a_process_that_started_at_the_register_moment_is_new() {
        let mut s = waiting_after_register();
        let p = proc(31, REGISTERED);
        s.next(answered_by(Some(p)));
        assert_eq!(
            s.next(answered_by(Some(p))).action,
            Action::Done(RepairOutcome::Recovered)
        );
    }

    #[test]
    fn a_backend_that_cannot_name_the_process_settles_on_the_answer_as_before() {
        let mut s = waiting_after_register();
        let n = s.next(answered_by(None));
        assert_eq!(n.action, Action::Done(RepairOutcome::Recovered));
    }

    #[test]
    fn the_executor_ends_registered_when_only_the_old_job_answers() {
        let old = proc(7, REGISTERED - 50);
        let mut s = Script {
            process: Some(old),
            ..Script::default()
        };
        let (cancel, abort) = (AtomicBool::new(false), AtomicBool::new(false));
        let bounds = Bounds {
            cancel: &cancel,
            abort: &abort,
            allowed: 4,
            register_only: false,
            clock: &paddock_health::FakeClock::at_wall(
                std::time::UNIX_EPOCH + Duration::from_micros(REGISTERED),
            ),
        };
        let out = replace_bounded(
            &mut s,
            &mut || true,
            &bounds,
            &Timings::default(),
            None,
            &mut || {},
            &mut || true,
            &mut |_| {},
        );
        assert_eq!(out, RepairOutcome::Registered);
    }

    #[test]
    fn the_executor_settles_on_a_process_that_started_after_the_register() {
        let new = proc(8, REGISTERED + 5);
        let mut s = Script {
            process: Some(new),
            ..Script::default()
        };
        let (cancel, abort) = (AtomicBool::new(false), AtomicBool::new(false));
        let bounds = Bounds {
            cancel: &cancel,
            abort: &abort,
            allowed: 4,
            register_only: false,
            clock: &paddock_health::FakeClock::at_wall(
                std::time::UNIX_EPOCH + Duration::from_micros(REGISTERED),
            ),
        };
        let out = replace_bounded(
            &mut s,
            &mut || true,
            &bounds,
            &Timings::default(),
            None,
            &mut || {},
            &mut || true,
            &mut |_| {},
        );
        assert_eq!(out, RepairOutcome::Recovered);
    }

    #[test]
    fn what_a_wait_let_go_of_is_taken_again_once_before_the_cycle_that_follows_it_and_not_before_the_first()
     {
        let events = std::cell::RefCell::new(Vec::<String>::new());
        let mut s = Script {
            quiet_for: 1,
            refused_for: 1,
            ..Script::default()
        };
        let (cancel, abort) = (AtomicBool::new(false), AtomicBool::new(false));
        let bounds = Bounds {
            cancel: &cancel,
            abort: &abort,
            allowed: 4,
            register_only: false,
            clock: &paddock_health::FakeClock::at_wall(
                std::time::UNIX_EPOCH + Duration::from_micros(1),
            ),
        };
        let mut asked = 0;
        let out = replace_bounded(
            &mut s,
            &mut || {
                asked += 1;
                asked > 1
            },
            &bounds,
            &Timings::default(),
            None,
            &mut || events.borrow_mut().push("release".into()),
            &mut || {
                events.borrow_mut().push("retake".into());
                true
            },
            &mut |st| events.borrow_mut().push(format!("say {st}")),
        );
        assert_eq!(out, RepairOutcome::Recovered);
        let log = events.borrow();
        let release = log
            .iter()
            .position(|e| e == "release")
            .expect("the wait let go");
        assert_eq!(log.iter().filter(|e| *e == "retake").count(), 1, "{log:?}");
        let retake = log.iter().position(|e| e == "retake").unwrap();
        assert!(
            release < retake,
            "taken again before it was let go: {log:?}"
        );
        assert!(log[retake + 1].starts_with("say "), "{log:?}");
        assert_eq!(
            s.calls.iter().filter(|c| **c == "register").count(),
            2,
            "the refused job was registered again"
        );
    }

    #[test]
    fn a_retake_that_says_no_ends_the_replace_before_any_os_call_of_the_next_cycle() {
        let mut s = Script {
            quiet_for: 1,
            refused_for: 1,
            ..Script::default()
        };
        let (cancel, abort) = (AtomicBool::new(false), AtomicBool::new(false));
        let bounds = Bounds {
            cancel: &cancel,
            abort: &abort,
            allowed: 4,
            register_only: false,
            clock: &paddock_health::FakeClock::at_wall(
                std::time::UNIX_EPOCH + Duration::from_micros(1),
            ),
        };
        let mut said = Vec::new();
        let out = replace_bounded(
            &mut s,
            &mut || false,
            &bounds,
            &Timings::default(),
            None,
            &mut || {},
            &mut || false,
            &mut |st| said.push(st.to_string()),
        );
        assert_eq!(out, RepairOutcome::Cancelled);
        assert_eq!(
            s.calls.iter().filter(|c| **c == "unregister").count(),
            1,
            "the next cycle removed again: {:?}",
            s.calls
        );
        assert!(
            said.iter().all(|x| !x.contains("retry 1")),
            "the next cycle was announced (it writes the record): {said:?}"
        );
    }

    #[test]
    fn after_a_register_read_back_wrong_a_silent_or_absent_job_starts_the_next_cycle_and_a_live_one_is_waited_for()
     {
        let after = |job, answers| {
            let mut s = Stepper::new(&Timings::default(), 4);
            s.start();
            through_removal(&mut s);
            s.next(OsResult::Gone { timed_out: false });
            s.next(OsResult::RegisterErr(RegisterError::NotAsWritten(
                Inspection::absent(),
            )));
            s.next(settled(job, answers)).action
        };
        assert_eq!(
            after(Job::Absent, true),
            Action::Unregister,
            "no job at all, whatever answers"
        );
        assert_eq!(
            after(Job::NotRunning, false),
            Action::Unregister,
            "a job that is silent"
        );
        assert_eq!(after(Job::Running, true), Action::Look);
        assert_eq!(
            after(Job::NotRunning, true),
            Action::Look,
            "something answers"
        );
    }

    #[test]
    fn a_cancel_ends_the_wait_at_once_and_the_time_running_out_ends_it_without_one() {
        let t = Timings::default();
        let waiting = || {
            let mut s = Stepper::new(&t, 4);
            s.start();
            through_removal(&mut s);
            s.next(OsResult::Gone { timed_out: false });
            s.next(OsResult::RegisterOk);
            s.next(settled(Job::NotRunning, false));
            s
        };
        let look = |secs, cancelled| OsResult::Looked {
            answers: false,
            job: Job::NotRunning,
            elapsed: Duration::from_secs(secs),
            cancelled,
            process: None,
            verdict: None,
        };
        let cancelled = waiting().next(look(0, true));
        assert_eq!(
            cancelled.action,
            Action::Done(RepairOutcome::Registered),
            "cancelled early"
        );
        let ran_out = waiting().next(look(t.start_wait.as_secs() + 1, false));
        assert_eq!(
            ran_out.action,
            Action::Done(RepairOutcome::Registered),
            "time ran out"
        );
        assert!(
            matches!(waiting().next(look(0, false)).action, Action::Pause(_)),
            "neither: keep waiting"
        );
    }

    // ---- the judged wait (ADR 0056 amendment: the wait for the service is judged, not clocked)

    fn looked_judged(job: Job, secs: u64, verdict: Option<StartVerdict>) -> OsResult {
        OsResult::Looked {
            answers: false,
            job,
            elapsed: Duration::from_secs(secs),
            cancelled: false,
            process: None,
            verdict,
        }
    }

    fn waiting_stepper() -> Stepper {
        let mut s = Stepper::new(&Timings::default(), 4);
        s.start_register_only();
        s.next(OsResult::RegisterOk);
        s.next(settled(Job::NotRunning, false));
        s
    }

    #[test]
    fn a_judged_wait_goes_on_past_start_wait_and_only_the_judge_ends_it() {
        let t = Timings::default();
        let mut s = waiting_stepper();
        // Far past the fixed limit, the judge says waiting: the wait goes on (without a verdict it would have ended).
        for secs in [t.start_wait.as_secs(), 5 * t.start_wait.as_secs()] {
            let n = s.next(looked_judged(
                Job::Running,
                secs,
                Some(StartVerdict::Waiting),
            ));
            assert_eq!(n.action, Action::Pause(t.start_poll), "at {secs} s");
            assert!(n.say.is_empty());
            assert_eq!(s.next(OsResult::Paused).action, Action::Look);
        }
        let note = s.next(looked_judged(
            Job::Running,
            600,
            Some(StartVerdict::StillWorking("still working".into())),
        ));
        assert_eq!(note.action, Action::Pause(t.start_poll));
        assert_eq!(note.say, [Step::Note("still working".into())]);
        // A cancel still ends it, with the registration in place.
        let cancelled = s.next(OsResult::Looked {
            answers: false,
            job: Job::Running,
            elapsed: Duration::ZERO,
            cancelled: true,
            process: None,
            verdict: Some(StartVerdict::Waiting),
        });
        assert_eq!(cancelled.action, Action::Done(RepairOutcome::Registered));
        // An unjudged look (no watch, or a backend that cannot observe) keeps the fixed bound.
        let mut s = waiting_stepper();
        let late = s.next(looked_judged(Job::Running, t.start_wait.as_secs(), None));
        assert_eq!(late.action, Action::Done(RepairOutcome::Registered));
    }

    #[test]
    fn a_failed_verdict_ends_the_repair_with_what_was_seen_and_what_to_do() {
        let mut s = waiting_stepper();
        let n = s.next(looked_judged(
            Job::NotRunning,
            9,
            Some(StartVerdict::Failed(
                "the job keeps exiting (observed: x)".into(),
            )),
        ));
        let Action::Done(RepairOutcome::Failed(why)) = &n.action else {
            panic!("{n:?}")
        };
        assert!(
            why.contains("did not start")
                && why.contains("observed: x")
                && why.contains("Repair again"),
            "{why}"
        );
        assert_eq!(n.say, [Step::Note(why.clone())]);
        assert!(!RepairOutcome::Failed(why.clone()).registered());
    }

    #[test]
    fn a_refused_verdict_is_another_cycle_and_the_cycles_stay_bounded() {
        let mut s = waiting_stepper();
        let n = s.next(looked_judged(
            Job::NotRunning,
            9,
            Some(StartVerdict::Refused("refused by the OS".into())),
        ));
        assert_eq!(n.action, Action::Unregister);
        assert!(
            matches!(&n.say[0], Step::Note(t) if t.contains("refuses the registration") && t.contains("refused by the OS")),
            "{:?}",
            n.say
        );
        assert!(n.say.contains(&Step::Cycle(1)));
        // Refused on every cycle ends in the bounded failure, not a loop.
        let mut s = Stepper::new(&Timings::default(), 2);
        s.start_register_only();
        s.next(OsResult::RegisterOk);
        s.next(settled(Job::NotRunning, false));
        let refused = || looked_judged(Job::NotRunning, 9, Some(StartVerdict::Refused("x".into())));
        assert_eq!(s.next(refused()).action, Action::Unregister);
        through_removal(&mut s);
        s.next(OsResult::Gone { timed_out: false });
        s.next(OsResult::Paused);
        s.next(OsResult::RegisterOk);
        s.next(settled(Job::NotRunning, false));
        let end = s.next(refused());
        assert!(
            matches!(&end.action, Action::Done(RepairOutcome::Failed(w)) if w.contains("still refuses")),
            "{end:?}"
        );
    }

    #[test]
    fn a_caller_that_must_return_at_once_is_not_made_to_wait_by_a_judge() {
        let t = Timings::default().without_start_wait();
        let mut s = Stepper::new(&t, 4);
        s.start_register_only();
        s.next(OsResult::RegisterOk);
        s.next(settled(Job::NotRunning, false));
        let n = s.next(looked_judged(Job::Running, 0, Some(StartVerdict::Waiting)));
        assert_eq!(n.action, Action::Done(RepairOutcome::Registered));
        // The executor does not even look at the job then.
        let mut b = Script {
            facts: Some(up()),
            ..Script::default()
        };
        let mut w = ScriptedWatch::default();
        let abort = AtomicBool::new(false);
        let cancel = AtomicBool::new(false);
        let bounds = Bounds {
            cancel: &cancel,
            abort: &abort,
            allowed: 4,
            register_only: false,
            clock: &paddock_health::SystemClock,
        };
        let out = replace_bounded(
            &mut b,
            &mut || false,
            &bounds,
            &t,
            Some(&mut w),
            &mut || {},
            &mut || true,
            &mut |_| {},
        );
        assert_eq!(out, RepairOutcome::Registered);
        assert!(w.polled.is_empty() && !b.calls.contains(&"observe"));
    }

    #[test]
    fn a_gave_up_verdict_ends_registered_and_does_not_advise_to_repair_again() {
        let mut s = waiting_stepper();
        let n = s.next(looked_judged(
            Job::Running,
            600,
            Some(StartVerdict::GaveUp(
                "still starting after 200 checks".into(),
            )),
        ));
        assert_eq!(n.action, Action::Done(RepairOutcome::Registered));
        let Step::Note(text) = &n.say[0] else {
            panic!("{:?}", n.say)
        };
        assert!(
            text.contains("still starting") && !text.contains("Repair"),
            "{text}"
        );
    }

    #[test]
    fn the_judged_wait_has_a_backstop_in_looks_whatever_the_watch_says() {
        let t = Timings {
            judged_ceiling_polls: 3,
            ..Timings::default()
        };
        let waiting = |job| {
            let mut s = Stepper::new(&t, 4);
            s.start_register_only();
            s.next(OsResult::RegisterOk);
            s.next(settled(Job::NotRunning, false));
            let _ = job;
            s
        };
        // A job that was seen running ends Registered: still starting, no advice to repair.
        let mut s = waiting(Job::Running);
        for secs in [0, 3] {
            assert!(matches!(
                s.next(looked_judged(
                    Job::Running,
                    secs,
                    Some(StartVerdict::Waiting)
                ))
                .action,
                Action::Pause(_)
            ));
            s.next(OsResult::Paused);
        }
        let end = s.next(looked_judged(Job::Running, 9, Some(StartVerdict::Waiting)));
        assert_eq!(
            end.action,
            Action::Done(RepairOutcome::Registered),
            "ER-19: a running job is not failed"
        );
        assert!(
            matches!(&end.say[0], Step::Note(n) if n.contains("still starting after 3 checks") && !n.contains("Repair")),
            "{:?}",
            end.say
        );
        // One that never ran ends Failed: it did not start.
        let mut s = waiting(Job::NotRunning);
        for secs in [0, 3] {
            s.next(looked_judged(
                Job::NotRunning,
                secs,
                Some(StartVerdict::Waiting),
            ));
            s.next(OsResult::Paused);
        }
        let end = s.next(looked_judged(
            Job::NotRunning,
            9,
            Some(StartVerdict::Waiting),
        ));
        let Action::Done(RepairOutcome::Failed(why)) = &end.action else {
            panic!("{end:?}")
        };
        assert!(
            why.contains("never started after 3 checks") && why.contains("Repair again"),
            "{why}"
        );
    }

    #[test]
    fn a_cancel_comes_before_a_verdict_that_ends_the_wait_for_the_judge() {
        for verdict in [
            StartVerdict::Failed("x".into()),
            StartVerdict::GaveUp("y".into()),
        ] {
            let mut s = waiting_stepper();
            let n = s.next(OsResult::Looked {
                answers: false,
                job: Job::Running,
                elapsed: Duration::ZERO,
                cancelled: true,
                process: None,
                verdict: Some(verdict),
            });
            assert_eq!(n.action, Action::Done(RepairOutcome::Registered));
            assert!(
                matches!(&n.say[0], Step::Note(t) if !t.contains("did not start")),
                "the user stopped waiting: {:?}",
                n.say
            );
        }
    }

    #[test]
    fn the_look_at_which_an_unconfirmed_process_answers_stays_a_judged_wait() {
        // A service that answers is settled only when two looks name the same new process (rule 53). Between the two looks the
        // wait is still the judge's: with no verdict it would fall back to the fixed bound and end `Registered` at once after a
        // long start, one look short of settling.
        let t = Timings::default();
        let mut s = waiting_after_register();
        let first = |verdict| OsResult::Looked {
            answers: true,
            job: Job::Running,
            elapsed: t.start_wait * 3,
            cancelled: false,
            process: Some(proc(5, REGISTERED + 10)),
            verdict,
        };
        let n = s.next(first(Some(StartVerdict::Waiting)));
        assert_eq!(
            n.action,
            Action::Pause(t.start_poll),
            "judged: the next look confirms"
        );
        assert_eq!(s.next(OsResult::Paused).action, Action::Look);
        let second = s.next(first(Some(StartVerdict::Waiting)));
        assert_eq!(second.action, Action::Done(RepairOutcome::Recovered));
        // Unjudged, the same look ends at the fixed bound.
        let mut s = waiting_after_register();
        assert_eq!(
            s.next(first(None)).action,
            Action::Done(RepairOutcome::Registered)
        );
    }

    #[test]
    fn a_judged_wait_looks_at_the_os_once_per_poll() {
        let mut s = Script {
            facts: Some(up()),
            ..Script::default()
        };
        let mut w = ScriptedWatch::default();
        let (out, _) = run_watched(&mut s, &mut w, Some(5), &AtomicBool::new(false));
        assert_eq!(out, RepairOutcome::Recovered);
        // `inspect` is the cycle's own (after the register, before the wait); the waiting looks are `observe_start`'s alone.
        let inspects = s.calls.iter().filter(|c| **c == "inspect").count();
        let observes = s.calls.iter().filter(|c| **c == "observe").count();
        assert_eq!((inspects, observes), (1, 6), "{:?}", s.calls);
    }

    #[test]
    fn an_up_verdict_from_a_watch_never_settles_the_service() {
        let mut s = waiting_stepper();
        let n = s.next(looked_judged(Job::Running, 3, Some(StartVerdict::Up)));
        assert_eq!(n.action, Action::Pause(Timings::default().start_poll));
    }

    /// A watch that follows a script and records what it was asked.
    #[derive(Default)]
    struct ScriptedWatch {
        verdicts: Vec<StartVerdict>,
        polled: Vec<StartObservation>,
        resets: u32,
    }

    impl StartWatch for ScriptedWatch {
        fn reset(&mut self) {
            self.resets += 1;
        }
        fn poll(&mut self, obs: &StartObservation) -> StartVerdict {
            self.polled.push(*obs);
            if self.verdicts.is_empty() {
                StartVerdict::Waiting
            } else {
                self.verdicts.remove(0)
            }
        }
    }

    fn up() -> Vec<crate::backend::StartFacts> {
        vec![crate::backend::StartFacts {
            job: Job::Running,
            pid: Some(ProcessId::new(7, StartTime::from_micros(1))),
            runs: Some(1),
            ..crate::backend::StartFacts::unknown()
        }]
    }

    fn run_watched(
        s: &mut Script,
        w: &mut ScriptedWatch,
        answers_after: Option<u32>,
        cancel: &AtomicBool,
    ) -> (RepairOutcome, Vec<String>) {
        let mut asked = 0;
        let mut answers = || {
            asked += 1;
            answers_after.is_some_and(|n| asked > n)
        };
        let mut said = Vec::new();
        let abort = AtomicBool::new(false);
        let bounds = Bounds {
            cancel,
            abort: &abort,
            allowed: 4,
            register_only: false,
            clock: &paddock_health::SystemClock,
        };
        let out = replace_bounded(
            s,
            &mut answers,
            &bounds,
            &Timings::default(),
            Some(w),
            &mut || {},
            &mut || true,
            &mut |st| said.push(st.to_string()),
        );
        (out, said)
    }

    #[test]
    fn without_a_watch_the_fixed_wait_stays_even_for_a_backend_that_can_observe() {
        let t = Timings::default();
        let mut s = Script {
            facts: Some(up()),
            ..Script::default()
        };
        let (out, said) = run(&mut s, None);
        assert_eq!(out, RepairOutcome::Registered);
        assert!(said.iter().any(|m| m.contains("within 120 seconds")));
        assert!(
            !s.calls.contains(&"observe"),
            "no watch: the job is not observed"
        );
        assert_eq!(s.paused, t.settle + t.start_wait);
    }

    #[test]
    fn a_watch_on_a_backend_that_cannot_observe_keeps_the_fixed_wait() {
        let mut s = Script::default();
        let mut w = ScriptedWatch::default();
        let (out, said) = run_watched(&mut s, &mut w, None, &AtomicBool::new(false));
        assert_eq!(out, RepairOutcome::Registered);
        assert!(said.iter().any(|m| m.contains("within 120 seconds")));
        assert!(
            w.polled.is_empty(),
            "nothing to judge: the watch is not asked"
        );
        assert_eq!(w.resets, 1);
    }

    #[test]
    fn a_watch_decides_how_long_the_wait_lasts_past_the_old_fixed_limit() {
        let t = Timings::default();
        let mut s = Script {
            facts: Some(up()),
            ..Script::default()
        };
        let mut w = ScriptedWatch::default();
        // The service answers on the 60th ask: three minutes, far past the fixed 120 seconds.
        let (out, said) = run_watched(&mut s, &mut w, Some(59), &AtomicBool::new(false));
        assert_eq!(out, RepairOutcome::Recovered);
        assert_eq!(
            w.polled.len(),
            60,
            "each look is judged, the one the service answers at included"
        );
        assert!(s.paused > t.start_wait, "{:?}", s.paused);
        assert!(w.polled.iter().all(
            |o| !o.answers && o.facts.pid == Some(ProcessId::new(7, StartTime::from_micros(1)))
        ));
        assert!(said.iter().any(|m| m.contains("answers again")));
    }

    #[test]
    fn a_watch_says_it_is_still_working_and_a_failed_verdict_ends_the_wait() {
        let mut s = Script {
            facts: Some(up()),
            ..Script::default()
        };
        let mut w = ScriptedWatch {
            verdicts: vec![
                StartVerdict::StillWorking("still working: slow disk".into()),
                StartVerdict::Failed("the job keeps exiting (observed: x)".into()),
            ],
            ..ScriptedWatch::default()
        };
        let (out, said) = run_watched(&mut s, &mut w, None, &AtomicBool::new(false));
        let RepairOutcome::Failed(why) = out else {
            panic!("{out:?}")
        };
        assert!(
            why.contains("did not start") && why.contains("Repair again"),
            "{why}"
        );
        assert!(said.iter().any(|m| m == "still working: slow disk"));
        assert!(said.contains(&why));
    }

    #[test]
    fn a_job_the_watch_finds_refused_returns_to_the_repair_cycle_and_each_registration_resets_the_watch()
     {
        let mut s = Script {
            facts: Some(up()),
            ..Script::default()
        };
        let mut w = ScriptedWatch {
            verdicts: vec![StartVerdict::Refused("refused by the OS".into())],
            ..ScriptedWatch::default()
        };
        let (out, said) = run_watched(&mut s, &mut w, Some(3), &AtomicBool::new(false));
        assert_eq!(out, RepairOutcome::Recovered);
        assert_eq!(
            s.calls.iter().filter(|c| **c == "register").count(),
            2,
            "a second cycle registered again"
        );
        assert!(said.iter().any(|m| m.contains("retry 1")));
        assert!(said.iter().any(|m| m.contains("refuses the registration")));
        assert_eq!(w.resets, 2, "each registration is a new wait");
    }

    #[test]
    fn a_cancel_stops_a_watched_wait_and_the_registration_stays() {
        let mut s = Script {
            facts: Some(up()),
            ..Script::default()
        };
        let mut w = ScriptedWatch::default();
        let cancel = AtomicBool::new(true);
        // Cancelled before anything: nothing is registered.
        let (out, _) = run_watched(&mut s, &mut w, None, &cancel);
        assert_eq!(out, RepairOutcome::Cancelled);
        assert!(w.polled.is_empty());
    }
}
