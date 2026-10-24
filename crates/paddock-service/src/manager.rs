//! The one object an app holds per background service: [`Manager`]. It looks (status), keeps the registration right at
//! every start (rule 24: [`Manager::on_start`]), repairs and installs on request and removes. Every method blocks on
//! bounded OS calls, so the app calls them from a background thread (rule 10) and reads the progress from
//! [`Manager::tracker`] meanwhile.

use crate::backend::{Backend, Inspection, Mechanism};
use crate::error::ServiceError;
use crate::plan::{Facts, Hold, Plan, Reason, plan};
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

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
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

    fn decide(&mut self, requested: bool) -> (Inspection, bool, Plan) {
        let answers = (self.answers)();
        let inspection = self.backend.inspect();
        let recorded = self.record.load();
        let p = plan(&Facts {
            inspection: &inspection,
            answers,
            recorded_version: recorded.registered_version.as_deref(),
            current_version: &self.version,
            requested,
            caps: self.backend.capabilities(),
        });
        (inspection, answers, p)
    }

    /// How the service is doing, for the UI and [`ServiceStatus::health`].
    pub fn status(&mut self) -> ServiceStatus {
        let (inspection, answers, p) = self.decide(false);
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
        self.decide(false).2
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
        if outcome.registered() {
            self.remember();
        }
        Some(outcome)
    }

    /// Keeps the registration right at every start (rule 24): replaces a stale registration while the service is
    /// down, and once after every version change where the OS pins the build (unregister → bootout → wait → register);
    /// re-points a registration whose program is gone or is another, older copy's; never installs unasked, never
    /// touches what the user switched off, never prompts. Call it on a background thread soon after start (and again
    /// later: the service may only fail when it restarts itself onto the new binary).
    pub fn on_start(&mut self, cancel: &AtomicBool) -> Started {
        match self.decide(false).2 {
            Plan::Leave(h) => Started::Left(h),
            Plan::MarkCurrent => {
                self.remember();
                Started::Marked
            }
            Plan::Offer(r) => Started::Offered(r),
            Plan::Replace(r) => match self.run_replace(r, cancel) {
                Some(o) => Started::Repaired(r, o),
                None => Started::Busy,
            },
        }
    }

    /// The user pressed Repair (or Install): replaces the registration now. What the user switched off is not touched;
    /// Login Items is opened for them instead where there is one.
    pub fn repair(&mut self, cancel: &AtomicBool) -> Result<RepairOutcome, ServiceError> {
        let reason = match self.decide(true).2 {
            Plan::Replace(r) => r,
            // A request is always a replacement unless the user's switch or the OS stops it.
            Plan::Leave(hold) => return Err(self.opened(refusal(hold))),
            Plan::MarkCurrent | Plan::Offer(_) => Reason::Requested,
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

    /// The definition this copy registers (`print-unit`, `print-plist`), when the mechanism has a file.
    pub fn definition(&self) -> Option<String> {
        self.backend.definition()
    }
}

/// Why a requested repair is not done.
fn refusal(hold: Hold) -> ServiceError {
    match hold {
        Hold::SwitchedOff => ServiceError::SwitchedOff,
        Hold::NeedsApproval => ServiceError::NeedsApproval,
        Hold::Unavailable(why) => ServiceError::Unavailable(why),
        Hold::NewerOwner(v) => ServiceError::NewerOwner(v),
        Hold::Healthy | Hold::NotInstalled => {
            ServiceError::Failed("Nothing to repair.".to_string())
        }
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
            refusal(Hold::SwitchedOff),
            ServiceError::SwitchedOff
        ));
        assert!(matches!(
            refusal(Hold::NeedsApproval),
            ServiceError::NeedsApproval
        ));
        assert!(
            matches!(refusal(Hold::Unavailable("w".into())), ServiceError::Unavailable(w) if w == "w")
        );
        assert!(
            matches!(refusal(Hold::NewerOwner("2".into())), ServiceError::NewerOwner(v) if v == "2")
        );
        assert!(matches!(refusal(Hold::Healthy), ServiceError::Failed(_)));
        assert!(matches!(
            refusal(Hold::NotInstalled),
            ServiceError::Failed(_)
        ));
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
