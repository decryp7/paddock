//! The parts of [`Manager`] that follow a stop on request ("Quit and stop measuring"): [`Manager::supervises`],
//! [`Manager::stop_mechanism`] and [`Manager::resume`] (ported from Dyno, whose macOS service code this replaces).
//!
//! A stop keeps the registration (`launchctl bootout` of the job), so the next start has to load the job again.
//! The user's switch in Login Items is theirs: a resume never touches an item switched off there, one it cannot
//! tell (the list of switches could not be read) or one the OS has no record of; a version change goes through the
//! full repair (the pinned build identity is new); and it never opens System Settings, whatever the repair meets.

use super::{Manager, outcome_result, refusal};
use crate::backend::DisabledMark;
use crate::error::ServiceError;
use crate::manager::Started;
use crate::plan::{ResumePath, installed, request, resume_path, supervises};
use crate::port::RegisterError;
use crate::repair::{RepairOutcome, Timings};
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
        supervises(&self.backend.inspect())
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
        let inspection = self.backend.inspect();
        let recorded = self.record.load();
        match resume_path(
            &inspection,
            recorded.registered_version.as_deref(),
            &self.version,
        ) {
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
                // A register that reads back wrong may only be the OS lagging: look for the job before replacing anything
                // (a removal and a second registration would announce the item again).
                let registered = match self.backend.register() {
                    Ok(_) => true,
                    Err(RegisterError::NotAsWritten(_)) => {
                        say(
                            "registering the enabled item read back unexpectedly; looking for the job first",
                        );
                        true
                    }
                    Err(e) => {
                        say(&format!(
                            "registering the enabled item failed ({e}); running the full repair"
                        ));
                        false
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
        // The full repair, on the user's behalf but unasked in the foreground: what the user switched is left alone and
        // Login Items is never opened (the background never opens a window).
        let inspection = self.backend.inspect();
        let reason = match request(&inspection) {
            Ok(reason) => reason,
            Err(refused) => {
                say(&format!("the repair is not done: {}", refusal(refused)));
                return Ok(false);
            }
        };
        match self.run_replace(reason, cancel) {
            None => Err(ServiceError::Busy),
            Some(RepairOutcome::Registered) => {
                say("the full repair registered the service, but it did not start in time");
                Err(ServiceError::Failed(
                    "The background service was registered again but did not start. It is tried again at the next start."
                        .to_string(),
                ))
            }
            Some(outcome) => outcome_result(outcome).map(|o| o.registered()),
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

    /// [`Manager::install`] for a start nobody asked for (first run): the same registration, but System Settings is
    /// never opened for a switch or an approval the user has to give.
    ///
    /// # Errors
    /// [`ServiceError`], as [`Manager::install`].
    pub fn install_unattended(
        &mut self,
        cancel: &AtomicBool,
    ) -> Result<RepairOutcome, ServiceError> {
        let inspection = self.backend.inspect();
        let reason = request(&inspection).map_err(refusal)?;
        match self.run_replace(reason, cancel) {
            None => Err(ServiceError::Busy),
            Some(outcome) => outcome_result(outcome),
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
}
