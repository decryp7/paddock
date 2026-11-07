//! The one object an app holds per background service: [`Manager`]. It looks (status), keeps the registration right at
//! every start (rule 24: [`Manager::on_start`]), repairs and installs on request and removes. Every method blocks on
//! bounded OS calls, so the app calls them from a background thread (rule 10) and reads the progress from
//! [`Manager::tracker`] meanwhile.

mod resume;

use crate::backend::{Backend, Inspection, Mechanism};
use crate::error::ServiceError;
use crate::plan::{Facts, Hold, MAX_ATTEMPTS, Plan, Reason, Refusal, plan};
use crate::record::{Phase, RecordFile, Recorded, State};
use crate::repair::{Bounds, RepairOutcome, Step, Timings, replace_bounded};
use crate::start_wait::StartWatch;
use crate::status::{Seen, ServiceStatus, compose};
use crate::tracker::RepairTracker;
use paddock_health::{Clock, SystemClock};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// What [`Manager::on_start`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Started {
    /// Nothing to do.
    Left(Hold),
    /// The registration was current; the version is remembered.
    Marked,
    /// The registration was replaced.
    Repaired(Reason, RepairOutcome),
    /// A replacement needs the user (an OS prompt): Repair is offered.
    Offered(Reason),
    /// Another registration or repair is running.
    Busy,
    /// An uninstall that was begun is finished.
    Removed,
}

pub use resume::{LIGHT_LOOKS, LIGHT_POLL};

/// What a replace run came to (the lock was already held).
enum Ran {
    Outcome(RepairOutcome),
    Busy,
    /// The record could not be written, so nothing was changed.
    Unrecorded(String),
}

/// The background service of an app.
pub struct Manager {
    app: String,
    backend: Box<dyn Backend>,
    record: RecordFile,
    version: String,
    program: Option<String>,
    timings: Timings,
    tracker: Arc<RepairTracker>,
    answers: Box<dyn FnMut() -> bool + Send>,
    /// Judges the wait for the service after a registration ([`Manager::with_start_watch`]).
    start_watch: Option<Box<dyn StartWatch>>,
    cover: Box<dyn FnMut() -> bool + Send>,
    wanted: Box<dyn FnMut() -> Option<bool> + Send>,
    /// The app said what its switch is ([`Manager::with_app_state`]).
    app_state: bool,
    first_run_done: Box<dyn FnMut() -> bool + Send>,
    location_ok: Box<dyn FnMut() -> bool + Send>,
    /// The process the last look at a running job named (rule R2: a job settles the service only when two looks name the same one).
    seen_job: Option<paddock_proc::ProcessId>,
    /// The time source of the waits, the repair's notice and the register stamp (the real one unless a test or a model passes
    /// another: a clock read is a system call, and on some Windows hosts a slow one).
    clock: Arc<dyn Clock>,
    /// What the last fresh look concluded for rule R2.
    job_confirmed: bool,
}

impl Manager {
    /// The service of the app called `app` (for messages), registered through `backend`, remembered in `data_dir` under
    /// `label`; `version` is this app's version (`env!("CARGO_PKG_VERSION")`), `answers` asks **the service itself**
    /// whether it is up (its own health check, bounded; never the app's own agent, which is [`Manager::with_cover`]).
    pub fn new(
        app: &str,
        backend: Box<dyn Backend>,
        data_dir: &Path,
        label: &str,
        version: &str,
        answers: impl FnMut() -> bool + Send + 'static,
    ) -> Self {
        Self {
            app: app.to_string(),
            backend,
            record: RecordFile::new(data_dir, label),
            version: version.to_string(),
            program: None,
            timings: Timings::default(),
            tracker: Arc::new(RepairTracker::new()),
            answers: Box::new(answers),
            start_watch: None,
            cover: Box::new(|| false),
            wanted: Box::new(|| Some(true)),
            app_state: false,
            first_run_done: Box::new(|| false),
            location_ok: Box::new(|| true),
            seen_job: None,
            clock: Arc::new(SystemClock),
            job_confirmed: true,
        }
    }

    /// Another record (the memory store of the model test).
    #[must_use]
    pub fn with_record(mut self, record: RecordFile) -> Self {
        self.record = record;
        self
    }

    /// Judges the wait for the service after a registration by what the OS shows of the job instead of a fixed time
    /// ([`Timings::start_wait`]): a slow start is waited for, a job that vanished, restarts in a loop or is stuck fails
    /// early with what was observed, a refused one goes round another cycle. [`StartWait`](crate::StartWait) is the
    /// recommended watch. It only applies to a backend that can observe the job ([`Backend::observe_start`]; launchd);
    /// the others, and a manager without a watch, keep the fixed wait.
    #[must_use]
    pub fn with_start_watch(mut self, watch: Box<dyn StartWatch>) -> Self {
        self.start_watch = Some(watch);
        self
    }

    /// Asks whether the app's own agent holds the data folder: it keeps a refused job from being replaced while the app
    /// measures, and never settles a registration (only the service's own answer does).
    #[must_use]
    pub fn with_cover(mut self, cover: impl FnMut() -> bool + Send + 'static) -> Self {
        self.cover = Box::new(cover);
        self
    }

    /// The app's own decisions as inputs to the plan (ADR 0056): the user's switch (`None`: unreadable, so nothing is
    /// registered unattended), whether the first run is done, and whether the app runs from a place it can register from.
    /// Without them the switch counts as on, the first run as not done and the location as good.
    #[must_use]
    pub fn with_app_state(
        mut self,
        wanted: impl FnMut() -> Option<bool> + Send + 'static,
        first_run_done: impl FnMut() -> bool + Send + 'static,
        location_ok: impl FnMut() -> bool + Send + 'static,
    ) -> Self {
        self.wanted = Box::new(wanted);
        self.app_state = true;
        self.first_run_done = Box::new(first_run_done);
        self.location_ok = Box::new(location_ok);
        self
    }

    /// The time source this manager (and so its tracker's readings) uses.
    pub fn clock(&self) -> Arc<dyn Clock> {
        self.clock.clone()
    }

    /// Another time source (a test's fake clock, or the crash model's, which never reads the system's). The repair tracker
    /// reads the same one through the times this manager gives it.
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Other waits (tests, or a service that takes longer to start).
    #[must_use]
    pub fn with_timings(mut self, timings: Timings) -> Self {
        self.timings = timings;
        self
    }

    /// The program recorded with the version (diagnostics).
    #[must_use]
    pub fn with_program(mut self, program: &str) -> Self {
        self.program = Some(program.to_string());
        self
    }

    /// Shares the progress with the commands that report it.
    #[must_use]
    pub fn with_tracker(mut self, tracker: Arc<RepairTracker>) -> Self {
        self.tracker = tracker;
        self
    }

    /// The repair's progress (read it from any thread).
    pub fn tracker(&self) -> Arc<RepairTracker> {
        self.tracker.clone()
    }

    /// How it is registered.
    pub fn mechanism(&self) -> Mechanism {
        self.backend.mechanism()
    }

    /// What the state file remembers.
    pub fn recorded(&self) -> Recorded {
        self.record.peek().0
    }

    fn decide(&mut self, waited: bool, resuming: bool) -> (Inspection, bool, Plan) {
        self.decide_with(waited, resuming, true)
    }

    /// [`Manager::decide`]; `fresh` is false for the read under the lock right after a decision: it is the same look, so it does
    /// not count as the second one of rule R2.
    fn decide_with(
        &mut self,
        waited: bool,
        resuming: bool,
        fresh: bool,
    ) -> (Inspection, bool, Plan) {
        let answers = (self.answers)();
        let covered = (self.cover)();
        let (recorded, record_damaged) = self.record.peek();
        // The job that runs must be the one this attempt's register call started (not the old build's that outlived the removal).
        let job_after_register = self.job_after_register(recorded.registered_at);
        self.backend.know_copy(
            self.program.as_deref(),
            recorded.registered_program.as_deref(),
        );
        let inspection = self.backend.inspect();
        if fresh {
            self.job_confirmed =
                answers || inspection.job != crate::backend::Job::Running || self.running_twice();
        }
        let job_confirmed = self.job_confirmed;
        let p = plan(&Facts {
            inspection: &inspection,
            answers,
            covered,
            recorded_version: recorded.registered_version.as_deref(),
            state: &recorded.state,
            current_version: &self.version,
            caps: self.backend.capabilities(),
            waited,
            resuming,
            // An app that does not say what its switch is has no word on an unfinished removal: it is finished.
            wanted: if self.app_state || !matches!(recorded.state, State::Removing) {
                (self.wanted)()
            } else {
                None
            },
            first_run_done: (self.first_run_done)(),
            location_ok: (self.location_ok)(),
            record_damaged,
            job_after_register,
            job_confirmed,
        });
        (inspection, answers, p)
    }

    /// Whether the running job started at or after the attempt's register call (`None`: no register time, or the OS cannot say).
    pub(crate) fn job_after_register(&mut self, registered_at: Option<u64>) -> Option<bool> {
        let at = registered_at?;
        self.backend
            .job_process()
            .map(|p| p.started().as_micros() >= at)
    }

    /// Rule R2: a job that runs while the service does not answer settles nothing on one look. This look names the process; it
    /// counts only when the last look named the same one (pid and start time, rule 53). A backend that cannot name the process
    /// leaves the look alone, as the repair's own wait does.
    pub(crate) fn running_twice(&mut self) -> bool {
        let Some(process) = self.backend.job_process() else {
            return true;
        };
        let same = self.seen_job == Some(process);
        self.seen_job = Some(process);
        same
    }

    /// How the service is doing, for the UI and [`ServiceStatus::health`].
    pub fn status(&mut self) -> ServiceStatus {
        let (inspection, answers, p) = self.decide(false, false);
        compose(Seen {
            mechanism: self.backend.mechanism(),
            caps: self.backend.capabilities(),
            inspection: &inspection,
            answers,
            plan: &p,
            repair: self.tracker.current(self.clock.monotonic()),
            app: &self.app,
        })
    }

    /// What [`Manager::on_start`] would do now, without doing it.
    pub fn check(&mut self) -> Plan {
        self.decide(false, false).2
    }

    /// Whether this app is waiting for the service to come up, or is about to register it: the plan for a start is `Await`
    /// or a replace that is under way. It is false when nothing is going to happen (the user's switch is off, approval is
    /// pending, the location is refused, the attempts are used up), so the app starts an agent of its own then. The
    /// link neither starts nor retires an agent of its own while this is true.
    pub fn pending(&mut self) -> bool {
        // Past the attempts the wait ends in a hold, so nothing is coming: not pending. A replace that the start will make is
        // pending already, so the link does not start an agent of its own in the seconds before the start repair runs.
        let rec = self.record.peek().0;
        let paid_tail = matches!(
            rec.phase_of(&self.version),
            Some(Phase::Registering | Phase::RemovingOld { paid: true, .. })
        );
        let used_up = rec.attempts_of(&self.version) >= MAX_ATTEMPTS;
        match self.decide(false, false).2 {
            Plan::Await(_) => !used_up,
            // A replace that cannot be recorded does not start (no OS call without a durable write): nothing is coming. The
            // paid finish of an interrupted replace comes past the cap.
            Plan::Replace(_) => (!used_up || paid_tail) && !self.record.unwritable(),
            _ => false,
        }
    }

    /// The proof that the service is this version's and this copy's (ADR 0056 rule B): it answers or runs, its job started at
    /// or after this attempt's register call, and the registration points at this copy.
    pub(crate) fn proves_ours(&mut self, i: &Inspection, answers: bool) -> bool {
        let at = self.record.peek().0.registered_at;
        (answers || (i.job == crate::backend::Job::Running && self.running_twice())) && self.job_after_register(at) != Some(false)
            && i.points_here != Some(false)
            // On a mechanism that can tell, the registration is this version's definition.
            && i.definition_current != Some(false)
    }

    /// Settles with a proof (clears the attempts); without one (a current definition that does not answer) remembers the version
    /// only, so the attempts are not given back (ADR 0056 rule A).
    fn mark(&mut self, i: &Inspection, answers: bool) {
        if self.proves_ours(i, answers) {
            return self.remember();
        }
        if let Err(e) = self.record.mark(&self.version, self.program.as_deref()) {
            log::warn!(error:% = e; "the registered service version could not be remembered");
        }
    }

    fn remember(&self) {
        if let Err(e) = self.record.save(&self.version, self.program.as_deref()) {
            log::warn!(error:% = e; "the registered service version could not be remembered");
        }
    }

    fn write_state(&self, state: State) -> Result<(), ServiceError> {
        self.record.set_state(state)
    }

    /// Replaces the registration; the caller holds the lock. The record is written before each OS step (phase
    /// `RemovingOld`, `Registering`, `Awaiting`), so a crash or a quit at any point leaves what is safe next. An unattended
    /// run gets what is left of this version's attempts; a requested one (the user pressed it) runs past the cap.
    fn run_replace_locked(
        &mut self,
        lock: &mut Option<crate::record::RepairLock>,
        reason: Reason,
        requested: bool,
        cancel: &AtomicBool,
        register_only_once: bool,
    ) -> Ran {
        let before = self.record.load();
        let attempts0 = before.attempts_of(&self.version);
        // Nothing registered and no job held: there is nothing to remove, so the first step is the registration.
        let (nothing_to_remove, had_registration) = {
            let i = self.backend.inspect();
            let had = i.registration != crate::backend::Registration::Absent;
            (
                !had && matches!(
                    i.job,
                    crate::backend::Job::Absent | crate::backend::Job::Unknown
                ),
                had,
            )
        };
        // Only this version's own replace that was begun (a crash after the removal, before or during the register call) is a
        // free finish: its register call was already counted or never made. Another version's attempts are not this one's.
        // The next register of an interrupted replace is paid for once (ADR 0056 rule A): by the request that began it, by the
        // attempt counted when `Registering` was written, or (nothing left to remove) by the crash-after-removal finish.
        let paid_tail = matches!(
            before.phase_of(&self.version),
            Some(Phase::Registering | Phase::RemovingOld { paid: true, .. })
        );
        let finishing = !requested
            && (paid_tail
                || (nothing_to_remove
                    && matches!(
                        before.phase_of(&self.version),
                        Some(Phase::RemovingOld { .. })
                    )));
        // Where registering over what is there replaces it in one step (no pinned build identity: systemd, the Windows Run value), the
        // new registration is made first and the old one is not removed, so a register that fails leaves the old service working.
        let register_first = !self.backend.capabilities().pinned_identity
            && !nothing_to_remove
            && !register_only_once
            && reason == Reason::VersionChanged
            && (self.answers)();
        let register_only_once = register_only_once || register_first;
        let allowed = if register_only_once {
            1
        } else {
            allowed_registers(requested, self.timings.max_cycles, attempts0, finishing)
        };
        if allowed == 0 {
            return Ran::Outcome(RepairOutcome::Failed(format!(
                "The service was registered {attempts0} times and did not start; it is not registered again by itself."
            )));
        }
        // What is replaced is this version's own when it saw the service come up or registered it and waits for it.
        let ours = before.registered_version.as_deref() == Some(self.version.as_str())
            || matches!(
                before.phase_of(&self.version),
                Some(Phase::Registering | Phase::Awaiting | Phase::RemovingOld { ours: true, .. })
            );
        let removing = Phase::RemovingOld {
            ours,
            paid: requested || finishing,
        };
        let first = State::Attempting {
            version: self.version.clone(),
            attempts: attempts0,
            phase: if finishing || nothing_to_remove || register_only_once {
                Phase::Registering
            } else {
                removing
            },
        };
        // No OS call without a durable write, not even for what the user asked: a service the record does not know about would be
        // replaced again at the next start.
        if let Err(e) = self
            .record
            .set_state_and_program(first, self.program.as_deref())
        {
            return Ran::Unrecorded(e.to_string());
        }
        if !self.tracker.start(self.clock.monotonic()) {
            return Ran::Busy;
        }
        log::info!(reason = reason.describe(), mechanism:? = self.backend.mechanism(), attempts = attempts0, allowed = allowed; "replacing the background service's registration");
        let tracker = self.tracker.clone();
        let clock = self.clock.clone();
        let record = &self.record;
        let version = self.version.clone();
        let answers = &mut self.answers;
        let mut attempts = attempts0;
        let mut free_register = finishing;
        let mut register_failed = false;
        let abort = AtomicBool::new(false);
        let bounds = Bounds {
            cancel,
            abort: &abort,
            allowed,
            register_only: nothing_to_remove || register_only_once,
            clock: clock.as_ref(),
        };
        // The wait for the service can take 120 s: the lock is let go for it, so a user's event (a removal, a repair) never waits.
        // A later cycle takes it back before it changes anything, and only when the record still shows this wait.
        let held = std::cell::RefCell::new(lock.take());
        let wait_lock = std::cell::Cell::new(WaitLock::Held);
        let removed = std::cell::Cell::new(false);
        let mut release = || {
            *held.borrow_mut() = None;
            wait_lock.set(WaitLock::LetGo);
        };
        let mut retake = || {
            let mut lock = held.borrow_mut();
            *lock = record.lock();
            let ours = lock.is_some()
                && matches!(record.load().state, State::Attempting { version: ref v, phase: Phase::Awaiting, .. } if *v == version);
            if ours {
                wait_lock.set(WaitLock::Held);
            } else {
                log::info!(
                    "the record or the lock changed during the wait (another request ran); this replace stops before it changes anything"
                );
                *lock = None;
                wait_lock.set(WaitLock::Lost);
            }
            ours
        };
        let outcome = replace_bounded(
            self.backend.as_mut(),
            &mut || answers(),
            &bounds,
            &self.timings,
            self.start_watch.as_deref_mut(),
            &mut release,
            &mut retake,
            &mut |step| {
                log::info!(step:% = step; "service repair");
                tracker.step(&step);
                let phase = match step {
                    Step::Removing => {
                        removed.set(true);
                        Some(removing)
                    }
                    Step::Registering => {
                        if let Err(e) = record.set_registered_at(Some(
                            crate::repair::start_time_of(clock.wall()).as_micros(),
                        )) {
                            log::warn!(error:% = e; "the time of the register call could not be recorded");
                        }
                        if free_register {
                            free_register = false;
                        } else {
                            attempts += 1;
                        }
                        Some(Phase::Registering)
                    }
                    Step::WaitingForStart => Some(Phase::Awaiting),
                    Step::RegisterFailed(_) => {
                        register_failed = true;
                        Some(Phase::RegisterFailed(crate::record::FailKind::Other))
                    }
                    _ => None,
                };
                if let Some(phase) = phase
                    && let Err(e) = record.set_state(State::Attempting {
                        version: version.clone(),
                        attempts,
                        phase,
                    })
                {
                    log::warn!(error:% = e; "the service repair's step could not be recorded");
                    // No OS call that changes anything without a durable write.
                    abort.store(true, Ordering::Relaxed);
                }
            },
        );
        self.tracker.finish(&outcome, self.clock.monotonic());
        log::info!(outcome:? = outcome; "service repair finished");
        // After a wait without the lock, what the record says now may be somebody else's: only a record that still shows this
        // wait is settled here.
        let still_ours = match wait_lock.get() {
            WaitLock::Held => true,
            WaitLock::Lost => false,
            // The replace ended inside a wait: only a record that still shows it is settled.
            WaitLock::LetGo => {
                *held.borrow_mut() = self.record.lock();
                matches!(self.record.load().state, State::Attempting { version: ref v, phase: Phase::Awaiting, .. } if *v == self.version)
            }
        };
        *lock = held.into_inner();
        let settled = if !still_ours {
            log::info!(
                "the record changed during the wait (another request ran); this replace leaves it as it is"
            );
            Ok(())
        } else {
            match &outcome {
                RepairOutcome::Recovered => {
                    self.remember();
                    Ok(())
                }
                // Accepted and waiting: the state already says so, and the next start gives it time instead of replacing it.
                RepairOutcome::Registered => {
                    let tried = self.record.load().attempts_of(&self.version);
                    log::warn!(
                        attempts = tried;
                        "the service was registered but did not answer; the next start gives it time and looks again"
                    );
                    Ok(())
                }
                // Nothing came up: not trusted as accepted, so the next start replaces (until the attempts are used).
                RepairOutcome::Failed(_) => self.write_state(State::Attempting {
                    version: self.version.clone(),
                    attempts,
                    // Not an interrupted removal (that one finishes for free): the attempts were used and failed.
                    phase: Phase::RegisterFailed(crate::record::FailKind::Other),
                }),
                // Nothing was counted: back to what it was.
                // A register that was made and then cancelled stays counted (the registration may be there): waiting for it.
                RepairOutcome::Cancelled if attempts > attempts0 => {
                    self.write_state(State::Attempting {
                        version: self.version.clone(),
                        attempts,
                        phase: Phase::Awaiting,
                    })
                }
                RepairOutcome::Cancelled => self.write_state(before.state.clone()),
                // The removal happened and nothing was registered (approval, unavailable, declined): the record keeps saying a removal
                // is unfinished, never less than the OS shows, so the next start finishes it (its register is paid for, so past the cap too).
                _ if had_registration
                    && removed.get()
                    && self.backend.inspect().registration
                        == crate::backend::Registration::Absent =>
                {
                    // A state of the user or the OS is not an attempt (N1): the count is the one before this register.
                    self.write_state(State::Attempting {
                        version: self.version.clone(),
                        attempts: attempts0,
                        phase: removing,
                    })
                }
                // The replace removed what an attempt of another version made: that attempt is over, so it is not restored (a
                // downgrade would otherwise take it for its own and settle a registration it did not make).
                _ => self.write_state(match &before.state {
                    State::Attempting { version, .. } if *version != self.version => State::Idle,
                    other => other.clone(),
                }),
            }
        };
        if let Err(e) = settled {
            log::warn!(error:% = e; "the outcome of the service repair could not be recorded");
        }
        Ran::Outcome(outcome)
    }

    /// The lock, or `None` after saying why.
    fn locked(&self) -> Option<crate::record::RepairLock> {
        let lock = self.record.lock();
        if lock.is_none() {
            log::info!("another registration or repair holds the service lock");
        }
        lock
    }

    /// Keeps the registration right at every start (rule 24): decides through [`plan`] on what the OS and the record
    /// show (read again under the lock before anything changes), gives a registration this version already made the time
    /// a start takes, replaces a stale one, and holds the service after [`MAX_ATTEMPTS`] register calls that never came up.
    /// Never installs unasked, never touches what the user switched off, never prompts. Call it on a background thread
    /// soon after start (and again later: the service may only fail when it restarts itself onto the new binary).
    pub fn on_start(&mut self, cancel: &AtomicBool) -> Started {
        self.start_with(cancel, false)
    }

    pub(crate) fn start_with(&mut self, cancel: &AtomicBool, resuming: bool) -> Started {
        let mut waited = false;
        for _ in 0..3 {
            let (inspection, answers, mut p) = self.decide(waited, resuming);
            let mut lock = None;
            if matches!(p, Plan::Replace(_) | Plan::Remove) {
                // Read everything again under the lock: another copy may have changed it while this one decided.
                let Some(l) = self.locked() else {
                    return Started::Busy;
                };
                lock = Some(l);
                p = self.decide_with(waited, resuming, false).2;
            }
            let recorded_now = self.record.load();
            log::info!(
                plan:? = p,
                registration:? = inspection.registration,
                job:? = inspection.job,
                login_item:? = inspection.login_item,
                answers = answers,
                recorded:? = recorded_now,
                current = self.version.as_str(),
                waited = waited;
                "service start: what the OS and the record show, and the plan"
            );
            match p {
                Plan::Leave(h) => {
                    match &h {
                        Hold::Unanswered(n) | Hold::Refused(n) => {
                            log::warn!(count = *n; "this version registered the service {n} times and it did not come up; it is held and not registered again by itself. Switching start at login off and on, an update, or a restart of the computer tries again")
                        }
                        _ => {}
                    }
                    return Started::Left(h);
                }
                Plan::MarkCurrent => {
                    self.mark(&inspection, answers);
                    return Started::Marked;
                }
                Plan::Offer(r) => return Started::Offered(r),
                Plan::Await(_) => {
                    // This version registered it already: give the service the time a start takes before removing anything.
                    if self.await_start(cancel) {
                        self.remember();
                        return Started::Marked;
                    }
                    waited = true;
                }
                Plan::Remove => {
                    let _held = lock;
                    return match self.uninstall_locked() {
                        Ok(()) => Started::Removed,
                        Err(e) => {
                            log::warn!(error:% = e; "the unfinished removal could not be finished");
                            Started::Left(Hold::Unavailable(e.to_string()))
                        }
                    };
                }
                Plan::Replace(r) => {
                    let mut held = lock;
                    return match self.run_replace_locked(&mut held, r, false, cancel, false) {
                        Ran::Outcome(o) => Started::Repaired(r, o),
                        Ran::Busy => Started::Busy,
                        Ran::Unrecorded(why) => {
                            log::warn!(why = why.as_str(); "the service state cannot be written, so nothing is changed");
                            Started::Left(Hold::RecordUnavailable(why))
                        }
                    };
                }
            }
        }
        Started::Busy
    }

    /// The user pressed Repair (or Install): replaces the registration now, past the cap. What the user switched off is
    /// not touched; Login Items is opened for them instead where there is one.
    pub fn repair(&mut self, cancel: &AtomicBool) -> Result<RepairOutcome, ServiceError> {
        let inspection = self.backend.inspect();
        let reason = match crate::plan::request(&inspection) {
            Ok(r) => r,
            Err(refused) => return Err(self.opened(refusal(refused))),
        };
        // A registration that is current, answers and is this copy's is left alone: replacing it could only lose it (ADR 0056).
        let (_, answers, p) = self.decide(false, false);
        if answers && matches!(p, Plan::Leave(Hold::Healthy)) {
            log::info!("{}", already_working(&self.app));
            return Ok(RepairOutcome::Recovered);
        }
        let mut lock = self.locked();
        if lock.is_none() {
            return Err(ServiceError::Busy);
        }
        match self.run_replace_locked(&mut lock, reason, true, cancel, false) {
            Ran::Busy => Err(ServiceError::Busy),
            Ran::Unrecorded(why) => Err(ServiceError::Failed(format!(
                "Nothing was changed, because the service state cannot be saved: {why}"
            ))),
            Ran::Outcome(o) => outcome_result(o).map_err(|e| self.opened(e)),
        }
    }

    /// Opens Login Items for an error the user fixes there.
    fn opened(&mut self, e: ServiceError) -> ServiceError {
        if matches!(e, ServiceError::SwitchedOff | ServiceError::NeedsApproval) {
            self.backend.open_login_items();
        }
        e
    }

    /// The user asked for the service (consent, Install, launch at login on): registers it, replacing whatever is
    /// there. The same as [`Manager::repair`].
    pub fn install(&mut self, cancel: &AtomicBool) -> Result<RepairOutcome, ServiceError> {
        self.repair(cancel)
    }

    /// Stops and removes the registration and forgets the version; removing what is not there succeeds. The record says
    /// `Removing` first, so a crash half-way is finished by the next start and never brings the service back.
    pub fn uninstall(&mut self) -> Result<(), ServiceError> {
        let _lock = self.locked().ok_or(ServiceError::Busy)?;
        self.uninstall_locked()
    }

    /// [`Manager::uninstall`] that waits for another registration or repair to finish, up to `tries` more looks.
    pub fn uninstall_patient(&mut self, tries: u32, pause: Duration) -> Result<(), ServiceError> {
        let mut result = self.uninstall();
        for _ in 0..tries {
            if !matches!(result, Err(ServiceError::Busy)) {
                break;
            }
            self.backend.pause(pause);
            result = self.uninstall();
        }
        result
    }

    fn uninstall_locked(&mut self) -> Result<(), ServiceError> {
        if self.tracker.working() {
            return Err(ServiceError::Busy);
        }
        let began = self.clock.monotonic();
        let since = |clock: &dyn Clock| clock.monotonic().saturating_sub(began);
        log::info!("{}", removal_began(self.backend.mechanism()));
        let before = self.record.load().state;
        // No OS call without a durable write, for the user's own removal too.
        if let Err(e) = self.write_state(State::Removing) {
            return Err(ServiceError::Failed(format!(
                "Nothing was changed, because the service state cannot be saved: {e}"
            )));
        }
        let unregistered = self.backend.unregister();
        if let Err(e) = self.backend.evict() {
            log::info!(error:% = e; "removing the service's job did not succeed");
        }
        let mut polled = Duration::ZERO;
        while !self.backend.job_gone()
            && polled.max(since(self.clock.as_ref())) < self.timings.gone_wait
        {
            self.backend.pause(self.timings.gone_poll);
            polled += self.timings.gone_poll;
        }
        if let Err(e) = unregistered
            && self.backend.inspect().present()
        {
            // Not removed (declined, failed): back to what it was, so the next start does not finish a removal that never was.
            if let Err(w) = self.write_state(before) {
                log::warn!(error:% = w; "the state before the removal could not be restored");
            }
            return Err(ServiceError::Failed(format!(
                "The background service could not be removed: {e}"
            )));
        }
        let forgotten = self.record.forget();
        log::info!(
            "{}",
            removal_done(
                u64::try_from(since(self.clock.as_ref()).as_millis()).unwrap_or(u64::MAX),
                forgotten.is_ok()
            )
        );
        forgotten
    }

    /// Opens the OS page where the user switches the service (macOS Login Items); `false` where there is none. The
    /// switch stays the user's.
    pub fn open_login_items(&mut self) -> bool {
        self.backend.open_login_items()
    }

    /// The definition this copy registers (`print-unit`, `print-plist`), when the mechanism has a file.
    pub fn definition(&self) -> Option<String> {
        self.backend.definition()
    }

    /// Waits up to `start_wait` (real time counts, not only the polls) for a registration this version made earlier to come
    /// up (the service answers or the OS runs the job), removing nothing and holding no lock. `true`: it did. `false`: it
    /// did not in time, the OS now refuses the job, or the wait was cancelled, so the plan is made again.
    pub(super) fn await_start(&mut self, cancel: &AtomicBool) -> bool {
        let began = self.clock.monotonic();
        let mut polled = Duration::ZERO;
        // The decision's look is not this wait's first: a job that runs is named by two looks of the wait, a poll apart (rule R2).
        self.seen_job = None;
        let (up, why) = loop {
            // The old build's job that outlived the removal answers too: only a job started by this register settles it.
            let ours = {
                let at = self.record.peek().0.registered_at;
                self.job_after_register(at) != Some(false)
            };
            if ours && (self.answers)() {
                break (true, "the service answers");
            }
            let job = self.backend.inspect().job;
            match job {
                crate::backend::Job::Running if ours && self.running_twice() => {
                    break (true, "the OS runs the job");
                }
                crate::backend::Job::Refused => break (false, "the OS refuses the job"),
                _ => {}
            }
            if cancel.load(Ordering::Relaxed) {
                break (false, "the wait was cancelled");
            }
            if polled.max(self.clock.monotonic().saturating_sub(began)) >= self.timings.start_wait {
                break (false, "the time a start takes ran out");
            }
            self.backend.pause(self.timings.start_poll);
            polled += self.timings.start_poll;
        };
        log::info!(up = up, why = why, waited_ms = u64::try_from(polled.max(self.clock.monotonic().saturating_sub(began)).as_millis()).unwrap_or(u64::MAX); "waited for a registration this version made");
        up
    }
}

/// Who holds the repair lock while a replace waits for the service.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WaitLock {
    /// The replace holds it (never let go, or taken again for a later cycle).
    Held,
    /// Let go for a wait and not taken again.
    LetGo,
    /// Taken again and not given back to the replace: the record or the lock was somebody else's.
    Lost,
}

/// How many register calls a replace may make: a request runs its cycles, an unattended one gets what is left of the attempts
/// (plus the free finish of a crashed registration).
fn allowed_registers(requested: bool, max_cycles: u32, attempts: u32, finishing: bool) -> u32 {
    if requested {
        max_cycles + 1
    } else {
        MAX_ATTEMPTS.saturating_sub(attempts) + u32::from(finishing)
    }
}

/// The log line that begins a removal (a user can read from a bundle which registration was taken out).
fn removal_began(mechanism: Mechanism) -> String {
    format!(
        "removing the background service's registration ({})",
        mechanism.describe()
    )
}

/// The log line that ends a removal, with how long it took and whether the record was forgotten.
fn removal_done(ms: u64, recorded: bool) -> String {
    format!(
        "the background service was removed in {ms} ms{}",
        if recorded {
            ""
        } else {
            "; the record could not be cleared"
        }
    )
}

/// What a request says when the service is current and answers: nothing is replaced.
pub fn already_working(app: &str) -> String {
    format!("{app}'s background service is already working.")
}

/// Why a requested repair is not done.
fn refusal(r: Refusal) -> ServiceError {
    match r {
        Refusal::SwitchedOff => ServiceError::SwitchedOff,
        Refusal::NeedsApproval => ServiceError::NeedsApproval,
        Refusal::Unavailable(why) => ServiceError::Unavailable(why),
    }
}

/// A requested repair's outcome as the caller's result: registered (or cancelled before any change) is `Ok`.
fn outcome_result(o: RepairOutcome) -> Result<RepairOutcome, ServiceError> {
    match o {
        RepairOutcome::NeedsApproval => Err(ServiceError::NeedsApproval),
        RepairOutcome::NeedsElevation => Err(ServiceError::NeedsElevation),
        RepairOutcome::Declined => Err(ServiceError::Declined),
        RepairOutcome::Unavailable(why) => Err(ServiceError::Unavailable(why)),
        RepairOutcome::Failed(why) => Err(ServiceError::Failed(why)),
        o @ (RepairOutcome::Recovered | RepairOutcome::Registered | RepairOutcome::Cancelled) => {
            Ok(o)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_of_a_request_on_a_working_service() {
        assert_eq!(
            already_working("Dyno"),
            "Dyno's background service is already working."
        );
    }

    #[test]
    fn a_removal_logs_how_it_began_and_how_it_ended() {
        assert!(removal_began(Mechanism::SystemdUser).contains("systemd user service"));
        assert_eq!(
            removal_done(12, true),
            "the background service was removed in 12 ms"
        );
        assert!(removal_done(12, false).contains("record could not be cleared"));
    }

    #[test]
    fn a_request_runs_its_cycles_and_an_unattended_replace_gets_what_is_left_of_the_attempts() {
        assert_eq!(allowed_registers(true, 3, 9, false), 4);
        assert_eq!(allowed_registers(false, 3, 1, false), MAX_ATTEMPTS - 1);
        assert_eq!(allowed_registers(false, 3, MAX_ATTEMPTS, false), 0);
        assert_eq!(
            allowed_registers(false, 3, MAX_ATTEMPTS, true),
            1,
            "the free finish"
        );
    }

    #[test]
    fn every_hold_and_outcome_maps_to_what_the_user_is_told() {
        assert!(matches!(
            refusal(Refusal::SwitchedOff),
            ServiceError::SwitchedOff
        ));
        assert!(matches!(
            refusal(Refusal::NeedsApproval),
            ServiceError::NeedsApproval
        ));
        assert!(
            matches!(refusal(Refusal::Unavailable("w".into())), ServiceError::Unavailable(w) if w == "w")
        );
        assert!(matches!(
            outcome_result(RepairOutcome::NeedsApproval),
            Err(ServiceError::NeedsApproval)
        ));
        assert!(matches!(
            outcome_result(RepairOutcome::NeedsElevation),
            Err(ServiceError::NeedsElevation)
        ));
        assert!(matches!(
            outcome_result(RepairOutcome::Declined),
            Err(ServiceError::Declined)
        ));
        assert!(matches!(
            outcome_result(RepairOutcome::Unavailable("w".into())),
            Err(ServiceError::Unavailable(_))
        ));
        assert!(matches!(
            outcome_result(RepairOutcome::Failed("w".into())),
            Err(ServiceError::Failed(_))
        ));
        for o in [
            RepairOutcome::Recovered,
            RepairOutcome::Registered,
            RepairOutcome::Cancelled,
        ] {
            assert_eq!(outcome_result(o.clone()).unwrap(), o);
        }
    }
}
