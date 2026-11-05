//! The one object an app holds per background service: [`Manager`]. It looks (status), keeps the registration right at
//! every start (rule 24: [`Manager::on_start`]), repairs and installs on request and removes. Every method blocks on
//! bounded OS calls, so the app calls them from a background thread (rule 10) and reads the progress from
//! [`Manager::tracker`] meanwhile.

mod resume;

use crate::backend::{Backend, Inspection, Mechanism};
use crate::error::ServiceError;
use crate::plan::{Attempt, Facts, Hold, Plan, Reason, Refusal, plan};
use crate::record::{RecordFile, Recorded};
use crate::repair::{RepairOutcome, Timings, replace};
use crate::status::{Seen, ServiceStatus, compose};
use crate::tracker::RepairTracker;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
}

#[cfg(test)]
pub(crate) fn unix_now_for_tests() -> u64 {
    unix_now()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub use resume::{LIGHT_LOOKS, LIGHT_POLL};

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
    clock: fn() -> u64,
}

impl Manager {
    /// The service of the app called `app` (for messages), registered through `backend`, remembered in `data_dir` under
    /// `label`; `version` is this app's version (`env!("CARGO_PKG_VERSION")`), `answers` asks the service itself
    /// whether it is up (its own health check, bounded).
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
            clock: unix_now,
        }
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

    /// Another clock for the repair lock (seconds since the Unix epoch).
    #[must_use]
    pub fn with_clock(mut self, clock: fn() -> u64) -> Self {
        self.clock = clock;
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
        self.record.load()
    }

    fn decide(&mut self) -> (Inspection, bool, Plan) {
        let answers = (self.answers)();
        let inspection = self.backend.inspect();
        let recorded = self.record.load();
        let p = plan(&Facts {
            inspection: &inspection,
            answers,
            recorded_version: recorded.registered_version.as_deref(),
            current_version: &self.version,
            caps: self.backend.capabilities(),
            attempted: recorded
                .attempted_version
                .as_deref()
                .map(|version| Attempt {
                    version,
                    count: recorded.attempts,
                }),
        });
        (inspection, answers, p)
    }

    /// How the service is doing, for the UI and [`ServiceStatus::health`].
    pub fn status(&mut self) -> ServiceStatus {
        let (inspection, answers, p) = self.decide();
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
        self.decide().2
    }

    fn remember(&self) {
        if let Err(e) = self.record.save(&self.version, self.program.as_deref()) {
            log::warn!(error:% = e; "the registered service version could not be remembered");
        }
    }

    fn run_replace(&mut self, reason: Reason, cancel: &AtomicBool) -> Option<RepairOutcome> {
        let _lock = self.record.lock((self.clock)())?;
        if !self.tracker.start(Instant::now()) {
            return None;
        }
        log::info!(reason = reason.describe(), mechanism:? = self.backend.mechanism(); "replacing the background service's registration");
        let tracker = self.tracker.clone();
        let answers = &mut self.answers;
        let outcome = replace(
            self.backend.as_mut(),
            &mut || answers(),
            cancel,
            &self.timings,
            &mut |step| {
                log::info!(step:% = step; "service repair");
                tracker.step(&step);
            },
        );
        self.tracker.finish(&outcome, Instant::now());
        log::info!(outcome:? = outcome; "service repair finished");
        // Only a service that answered (or whose job runs) is remembered as registered by this version: a registration the
        // OS accepted but that never answered would otherwise hide a dead service until the next version. Not remembered,
        // the next start looks again.
        if matches!(outcome, RepairOutcome::Recovered) {
            self.remember();
        } else if outcome.registered() {
            match self.record.attempt(&self.version) {
                Ok(count) => log::warn!(
                    count = count;
                    "the service was registered but did not answer; the version is not remembered as done, so the next start gives it time and looks again"
                ),
                Err(e) => {
                    log::warn!(error:% = e; "the unanswered registration could not be remembered")
                }
            }
        }
        Some(outcome)
    }

    /// Keeps the registration right at every start (rule 24): replaces a stale registration while the service is
    /// down, and once after every version change where the OS pins the build (unregister → bootout → wait → register);
    /// re-points a registration whose program is gone or is another, older copy's; never installs unasked, never
    /// touches what the user switched off, never prompts. Call it on a background thread soon after start (and again
    /// later: the service may only fail when it restarts itself onto the new binary).
    pub fn on_start(&mut self, cancel: &AtomicBool) -> Started {
        let (inspection, answers, p) = self.decide();
        log::info!(
            plan:? = p,
            registration:? = inspection.registration,
            job:? = inspection.job,
            login_item:? = inspection.login_item,
            answers = answers,
            recorded:? = self.record.load(),
            current = self.version.as_str();
            "service start: what the OS and the record show, and the plan"
        );
        match p {
            Plan::Leave(h) => {
                if let Hold::Unanswered(n) = &h {
                    log::warn!(count = *n; "this version registered the service {n} times and it never answered; it is held, not replaced again. Repair or a restart of the computer may help");
                }
                Started::Left(h)
            }
            Plan::MarkCurrent => {
                self.remember();
                Started::Marked
            }
            Plan::Offer(r) => Started::Offered(r),
            Plan::Await(r) => {
                // This version registered it already: give the service the time a start takes before removing anything.
                if self.await_start(cancel) {
                    self.remember();
                    return Started::Marked;
                }
                match self.run_replace(r, cancel) {
                    Some(o) => Started::Repaired(r, o),
                    None => Started::Busy,
                }
            }
            Plan::Replace(r) => match self.run_replace(r, cancel) {
                Some(o) => Started::Repaired(r, o),
                None => Started::Busy,
            },
        }
    }

    /// The user pressed Repair (or Install): replaces the registration now. What the user switched off is not touched;
    /// Login Items is opened for them instead where there is one.
    pub fn repair(&mut self, cancel: &AtomicBool) -> Result<RepairOutcome, ServiceError> {
        let inspection = self.backend.inspect();
        let reason = match crate::plan::request(&inspection) {
            Ok(r) => r,
            Err(refused) => return Err(self.opened(refusal(refused))),
        };
        match self.run_replace(reason, cancel) {
            None => Err(ServiceError::Busy),
            Some(o) => outcome_result(o).map_err(|e| self.opened(e)),
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

    /// Stops and removes the registration and forgets the version; removing what is not there succeeds.
    pub fn uninstall(&mut self) -> Result<(), ServiceError> {
        let _lock = self.record.lock((self.clock)()).ok_or(ServiceError::Busy)?;
        if self.tracker.working() {
            return Err(ServiceError::Busy);
        }
        let unregistered = self.backend.unregister();
        if let Err(e) = self.backend.evict() {
            log::info!(error:% = e; "removing the service's job did not succeed");
        }
        let mut waited = Duration::ZERO;
        while !self.backend.job_gone() && waited < self.timings.gone_wait {
            self.backend.pause(self.timings.gone_poll);
            waited += self.timings.gone_poll;
        }
        if let Err(e) = unregistered
            && self.backend.inspect().present()
        {
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
