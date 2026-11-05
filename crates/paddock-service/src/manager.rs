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
use crate::status::{Seen, ServiceStatus, compose};
use crate::tracker::RepairTracker;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

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
    cover: Box<dyn FnMut() -> bool + Send>,
    wanted: Box<dyn FnMut() -> Option<bool> + Send>,
    /// The app said what its switch is ([`Manager::with_app_state`]).
    app_state: bool,
    first_run_done: Box<dyn FnMut() -> bool + Send>,
    location_ok: Box<dyn FnMut() -> bool + Send>,
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
            cover: Box::new(|| false),
            wanted: Box::new(|| Some(true)),
            app_state: false,
            first_run_done: Box::new(|| false),
            location_ok: Box::new(|| true),
        }
    }

    /// Another record (the memory store of the model test).
    #[must_use]
    pub fn with_record(mut self, record: RecordFile) -> Self {
        self.record = record;
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
        let answers = (self.answers)();
        let covered = (self.cover)();
        let (recorded, record_damaged) = self.record.peek();
        self.backend.know_copy(self.program.as_deref(), recorded.registered_program.as_deref());
        let inspection = self.backend.inspect();
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
        });
        (inspection, answers, p)
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
            repair: self.tracker.current(Instant::now()),
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
        let used_up = self.record.peek().0.attempts_of(&self.version) >= MAX_ATTEMPTS;
        match self.decide(false, false).2 {
            Plan::Await(_) | Plan::Replace(_) => !used_up,
            _ => false,
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
    fn run_replace_locked(&mut self, reason: Reason, requested: bool, cancel: &AtomicBool) -> Ran {
        let before = self.record.load();
        let attempts0 = before.attempts_of(&self.version);
        // Nothing registered and no job held: there is nothing to remove, so the first step is the registration.
        let nothing_to_remove = {
            let i = self.backend.inspect();
            i.registration == crate::backend::Registration::Absent
                && matches!(i.job, crate::backend::Job::Absent | crate::backend::Job::Unknown)
        };
        // Only this version's own replace that was begun (a crash after the removal, before or during the register call) is a
        // free finish: its register call was already counted or never made. Another version's attempts are not this one's.
        let finishing = nothing_to_remove
            && matches!(
                before.phase_of(&self.version),
                Some(Phase::Registering | Phase::RemovingOld { .. })
            );
        let allowed = allowed_registers(requested, self.timings.max_cycles, attempts0, finishing);
        if allowed == 0 {
            return Ran::Outcome(RepairOutcome::Failed(format!(
                "The service was registered {attempts0} times and did not start; it is not registered again by itself."
            )));
        }
        // What is replaced is this version's own when it saw the service come up or registered it and waits for it.
        let ours = before.registered_version.as_deref() == Some(self.version.as_str())
            || matches!(
                before.phase_of(&self.version),
                Some(Phase::Registering | Phase::Awaiting | Phase::RemovingOld { ours: true })
            );
        let removing = Phase::RemovingOld { ours };
        let first = State::Attempting {
            version: self.version.clone(),
            attempts: attempts0,
            phase: if finishing || nothing_to_remove { Phase::Registering } else { removing },
        };
        // No OS call without a durable write, not even for what the user asked: a service the record does not know about would be
        // replaced again at the next start.
        if let Err(e) = self.record.set_state_and_program(first, self.program.as_deref()) {
            return Ran::Unrecorded(e.to_string());
        }
        if !self.tracker.start(Instant::now()) {
            return Ran::Busy;
        }
        log::info!(reason = reason.describe(), mechanism:? = self.backend.mechanism(), attempts = attempts0, allowed = allowed; "replacing the background service's registration");
        let tracker = self.tracker.clone();
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
            register_only: nothing_to_remove,
        };
        let outcome = replace_bounded(
            self.backend.as_mut(),
            &mut || answers(),
            &bounds,
            &self.timings,
            &mut |step| {
                log::info!(step:% = step; "service repair");
                tracker.step(&step);
                let phase = match step {
                    Step::Removing => Some(removing),
                    Step::Registering => {
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
        self.tracker.finish(&outcome, Instant::now());
        log::info!(outcome:? = outcome; "service repair finished");
        let settled = match &outcome {
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
                phase: if register_failed {
                    Phase::RegisterFailed(crate::record::FailKind::Other)
                } else {
                    removing
                },
            }),
            // Nothing was counted: back to what it was.
            // A register that was made and then cancelled stays counted (the registration may be there): waiting for it.
            RepairOutcome::Cancelled if attempts > attempts0 => self.write_state(State::Attempting {
                version: self.version.clone(),
                attempts,
                phase: Phase::Awaiting,
            }),
            RepairOutcome::Cancelled => self.write_state(before.state.clone()),
            // The replace removed what an attempt of another version made: that attempt is over, so it is not restored (a
            // downgrade would otherwise take it for its own and settle a registration it did not make).
            _ => self.write_state(match &before.state {
                State::Attempting { version, .. } if *version != self.version => State::Idle,
                other => other.clone(),
            }),
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
                p = self.decide(waited, resuming).2;
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
                        Hold::Unanswered(n) | Hold::Refused(n) => log::warn!(count = *n; "this version registered the service {n} times and it did not come up; it is held and not registered again by itself. Switching start at login off and on, an update, or a restart of the computer tries again"),
                        _ => {}
                    }
                    return Started::Left(h);
                }
                Plan::MarkCurrent => {
                    self.remember();
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
                    let _held = lock;
                    return match self.run_replace_locked(r, false, cancel) {
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
        let Some(_lock) = self.locked() else {
            return Err(ServiceError::Busy);
        };
        match self.run_replace_locked(reason, true, cancel) {
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
        let began = Instant::now();
        while !self.backend.job_gone() && polled.max(began.elapsed()) < self.timings.gone_wait {
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
        self.record.forget()
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
        let began = Instant::now();
        let mut polled = Duration::ZERO;
        let (up, why) = loop {
            if (self.answers)() {
                break (true, "the service answers");
            }
            match self.backend.inspect().job {
                crate::backend::Job::Running => break (true, "the OS runs the job"),
                crate::backend::Job::Refused => break (false, "the OS refuses the job"),
                _ => {}
            }
            if cancel.load(Ordering::Relaxed) {
                break (false, "the wait was cancelled");
            }
            if polled.max(began.elapsed()) >= self.timings.start_wait {
                break (false, "the time a start takes ran out");
            }
            self.backend.pause(self.timings.start_poll);
            polled += self.timings.start_poll;
        };
        log::info!(up = up, why = why, waited_ms = u64::try_from(polled.max(began.elapsed()).as_millis()).unwrap_or(u64::MAX); "waited for a registration this version made");
        up
    }
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
    fn a_request_runs_its_cycles_and_an_unattended_replace_gets_what_is_left_of_the_attempts() {
        assert_eq!(allowed_registers(true, 3, 9, false), 4);
        assert_eq!(allowed_registers(false, 3, 1, false), MAX_ATTEMPTS - 1);
        assert_eq!(allowed_registers(false, 3, MAX_ATTEMPTS, false), 0);
        assert_eq!(allowed_registers(false, 3, MAX_ATTEMPTS, true), 1, "the free finish");
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
