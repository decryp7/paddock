//! The parts of [`Manager`] that follow a stop on request ("Quit and stop measuring"): [`Manager::supervises`],
//! [`Manager::stop_mechanism`] and [`Manager::resume`] (ported from Dyno, whose macOS service code this replaces).
//!
//! A stop keeps the registration (`launchctl bootout` of the job), so the next start has to load the job again.
//! The user's switch in Login Items is theirs: a resume never touches an item switched off there, one it cannot
//! tell (the list of switches could not be read) or one the OS has no record of; a version change goes through the
//! full repair (the pinned build identity is new); and it never opens System Settings, whatever the repair meets.

use super::{Manager, Ran, outcome_result, refusal};
use crate::backend::{DisabledMark, Job};
use crate::error::ServiceError;
use crate::manager::Started;
use crate::plan::Reason;
use crate::plan::{ResumePath, installed, request, resume_path, supervises};
use crate::record::Phase;
use crate::repair::RepairOutcome;
use crate::stop::StopMechanism;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// How often the light path asks the OS for the job before it gives up on it.
pub const LIGHT_LOOKS: u32 = 20;
/// The pause between two looks of the light path.
pub const LIGHT_POLL: Duration = Duration::from_millis(500);

impl Manager {
    /// Whether the service is registered (or waiting for the user's approval): the app has set it up
    /// ([`installed`]).
    pub fn installed(&mut self) -> bool {
        installed(&self.backend.inspect())
    }

    /// Whether the OS starts the job now and restarts it, so the app must wait for it instead of running its own agent
    /// ([`supervises`]).
    pub fn supervises(&mut self) -> bool {
        let inspection = self.backend.inspect();
        if !supervises(&inspection) {
            return false;
        }
        // Attempts used up and nothing answers: held. The wait that remains ends in a hold, so nothing will start it.
        if self.record.peek().0.attempts_of(&self.version) >= crate::plan::MAX_ATTEMPTS
            && !((self.answers)() || inspection.job == Job::Running)
        {
            return false;
        }
        // A held service (its attempts are used up) is not supervised: nothing will start it, so the app must not wait.
        !matches!(
            self.check(),
            crate::plan::Plan::Leave(
                crate::plan::Hold::Unanswered(_)
                    | crate::plan::Hold::Refused(_)
                    | crate::plan::Hold::RegisterFailed(_)
            )
        )
    }

    /// The job a stop can name, while the OS holds it. The registration is not touched by the stop
    /// ([`crate::stop::stop`] judges it by reading the OS back).
    pub fn stop_mechanism(&mut self) -> Option<StopMechanism> {
        self.backend.stop_mechanism()
    }

    /// Brings the service back after a stop on request; `say` gets every step for the log. Returns whether the OS holds
    /// the job afterwards (`Ok(false)`: left alone on purpose, see the module documentation).
    ///
    /// # Errors
    /// [`ServiceError`] when the full repair could not put the service back (another repair running is
    /// [`ServiceError::Busy`]).
    pub fn resume(
        &mut self,
        cancel: &AtomicBool,
        say: &mut dyn FnMut(&str),
    ) -> Result<bool, ServiceError> {
        let result = self.resume_inner(cancel, say);
        // A resume that was done (the service is back, or left alone on purpose) clears the "stopped on request" marker; one
        // that failed keeps it, so the next start tries again.
        if result.is_ok() {
            self.clear_resume_pending();
        }
        result
    }

    fn clear_resume_pending(&self) {
        if self.record.resume_pending().is_some()
            && let Err(e) = self.record.set_resume_pending(None)
        {
            log::warn!(error:% = e; "the stop marker could not be cleared");
        }
    }

    fn resume_inner(
        &mut self,
        cancel: &AtomicBool,
        say: &mut dyn FnMut(&str),
    ) -> Result<bool, ServiceError> {
        let inspection = self.backend.inspect();
        let recorded = self.record.load();
        if matches!(recorded.state, crate::record::State::Removing) || (self.wanted)() != Some(true)
        {
            say(
                "the user's switch is off or unknown (or a removal was begun); the service stays as it is",
            );
            self.clear_resume_pending();
            return Ok(false);
        }
        if !self.backend.capabilities().automatic {
            say("registering needs the user's permission here; the service stays as it is");
            return Ok(false);
        }
        match resume_path(&inspection, &recorded, &self.version) {
            ResumePath::Nothing => {
                say(
                    if inspection
                        .login_item
                        .is_some_and(|f| f.mark == DisabledMark::Unreadable)
                    {
                        "the list of switched-off items could not be read; the service stays as it is"
                    } else {
                        "switched off in Login Items or missing; the service stays as it is"
                    },
                );
                return Ok(false);
            }
            ResumePath::Repair => {}
            ResumePath::Light => {
                // Registering again is an OS call: it goes through the lock and the executor like every other, with nothing to
                // remove (register only, one cycle, no wait for the service: the job is looked for below).
                let mut lock = self.locked();
                if lock.is_none() {
                    return Err(ServiceError::Busy);
                }
                let saved = self.timings;
                self.timings = saved.without_start_wait();
                let ran = self.run_replace_locked(&mut lock, Reason::Stopped, false, cancel, true);
                self.timings = saved;
                drop(lock);
                let registered = match ran {
                    Ran::Busy => return Err(ServiceError::Busy),
                    Ran::Unrecorded(why) => {
                        say(&format!(
                            "the service state cannot be written, so nothing is registered: {why}"
                        ));
                        return Err(ServiceError::Failed(why));
                    }
                    // A register that reads back wrong may only be the OS lagging: look for the job before replacing anything
                    // (a removal and a second registration would announce the item again).
                    Ran::Outcome(RepairOutcome::Failed(why)) => {
                        say(&format!(
                            "registering the enabled item failed ({why}); looking for the job, then the full repair"
                        ));
                        true
                    }
                    Ran::Outcome(o) if o.registered() => true,
                    Ran::Outcome(_) => {
                        say(
                            "the enabled item cannot be registered without the user; the service stays as it is",
                        );
                        return Ok(false);
                    }
                };
                if registered {
                    for _ in 0..LIGHT_LOOKS {
                        if self.backend.job_found() {
                            say(
                                "the OS holds the job again after registering the enabled item (it may not run yet)",
                            );
                            return Ok(true);
                        }
                        self.backend.pause(LIGHT_POLL);
                    }
                    say("registering the enabled item loaded no job; running the full repair");
                }
            }
        }
        // The full repair goes through the same plan as a start (so a version change, a hold and a registration that is
        // still coming up are judged the same way), unasked in the foreground: what the user switched is left alone and
        // Login Items is never opened (the background never opens a window).
        match self.start_with(cancel, true) {
            Started::Busy => Err(ServiceError::Busy),
            Started::Repaired(_, RepairOutcome::Registered) => {
                say("the full repair registered the service, but it did not start in time");
                Err(ServiceError::Failed(
                    "The background service was registered again but did not start. It is tried again at the next start."
                        .to_string(),
                ))
            }
            Started::Repaired(_, outcome) => outcome_result(outcome).map(|o| o.registered()),
            Started::Marked => Ok(true),
            Started::Left(h) => {
                say(&format!("the service is left as it is: {h:?}"));
                Ok(false)
            }
            Started::Offered(_) | Started::Removed => Ok(false),
        }
    }

    /// [`Manager::on_start`] that waits for another registration or repair to finish (`Busy`) up to `tries` looks, `pause`
    /// apart, instead of giving up on the first one: a start that gave up leaves no record, and the next start would
    /// replace a registration that was fine.
    pub fn on_start_patient(
        &mut self,
        cancel: &AtomicBool,
        tries: u32,
        pause: Duration,
    ) -> Started {
        let mut started = self.on_start(cancel);
        for _ in 0..tries {
            if !matches!(started, Started::Busy) {
                break;
            }
            log::info!("another registration or repair is running; looking again shortly");
            self.backend.pause(pause);
            started = self.on_start(cancel);
        }
        started
    }

    /// [`Manager::install`] (or, `quiet`, [`Manager::install_unattended`]) that waits for another registration or repair
    /// to finish: a first run that lost the lock would otherwise end with nothing registered and no record of having tried.
    ///
    /// # Errors
    /// [`ServiceError`], as [`Manager::install`]; [`ServiceError::Busy`] after `tries` more looks.
    pub fn install_patient(
        &mut self,
        cancel: &AtomicBool,
        quiet: bool,
        tries: u32,
        pause: Duration,
    ) -> Result<RepairOutcome, ServiceError> {
        let mut tried = 0u32;
        let mut result = if quiet {
            self.install_unattended(cancel)
        } else {
            self.install(cancel)
        };
        for _ in 0..tries {
            if !matches!(result, Err(ServiceError::Busy)) {
                break;
            }
            tried += 1;
            log::info!(tried = tried, tries = tries; "another registration or repair holds the lock; the install looks again shortly");
            self.backend.pause(pause);
            result = if quiet {
                self.install_unattended(cancel)
            } else {
                self.install(cancel)
            };
        }
        result
    }

    /// [`Manager::install`] for a start nobody asked for (first run): never removes anything and is bounded like a start. A
    /// registration that is there (any version's) is left to [`Manager::on_start`]'s plan; nothing registered is registered
    /// once, under the attempts left, with a record that can be written, and only where no prompt is needed.
    ///
    /// # Errors
    /// [`ServiceError`], as [`Manager::install`]; [`ServiceError::Failed`] when the attempts are used up or the state cannot be
    /// written.
    pub fn install_unattended(
        &mut self,
        cancel: &AtomicBool,
    ) -> Result<RepairOutcome, ServiceError> {
        let inspection = self.backend.inspect();
        let reason = request(&inspection).map_err(refusal)?;
        let recorded = self.record.peek().0;
        if matches!(recorded.state, crate::record::State::Removing)
            && let Err(e) = self.record.set_state(crate::record::State::Idle)
        {
            // The user switched it on again: an unfinished removal is not to be finished.
            log::warn!(error:% = e; "the unfinished removal could not be cancelled");
        }
        let here = recorded.registered_version.as_deref() == Some(self.version.as_str())
            || matches!(
                recorded.phase_of(&self.version),
                Some(Phase::Registering | Phase::Awaiting)
            );
        // A job that lingers with no registration is left to the start's plan too: a first run removes nothing.
        let present = inspection.registration != crate::backend::Registration::Absent
            || !matches!(inspection.job, Job::Absent | Job::Unknown);
        if present {
            // Something is registered: a first run removes nothing. Up (and this version's): remembered; otherwise the
            // start's plan decides what the registration needs.
            let answers = (self.answers)();
            let up = self.proves_ours(&inspection, answers);
            if up && here {
                self.remember();
            }
            return Ok(if up {
                RepairOutcome::Recovered
            } else {
                RepairOutcome::Registered
            });
        }
        if !self.backend.capabilities().automatic {
            return Err(ServiceError::Unavailable(
                "registering the service needs the user's permission here; use Repair".to_string(),
            ));
        }
        let mut lock = self.locked();
        if lock.is_none() {
            return Err(ServiceError::Busy);
        }
        if recorded.attempts_of(&self.version) >= crate::plan::MAX_ATTEMPTS {
            return Err(ServiceError::Failed(format!(
                "The service was registered {} times and did not start; it is not registered again by itself.",
                recorded.attempts_of(&self.version)
            )));
        }
        match self.run_replace_locked(&mut lock, reason, false, cancel, false) {
            Ran::Busy => Err(ServiceError::Busy),
            Ran::Unrecorded(why) => Err(ServiceError::Failed(why)),
            Ran::Outcome(outcome) => outcome_result(outcome),
        }
    }
}
