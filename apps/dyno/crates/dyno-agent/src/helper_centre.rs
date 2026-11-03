//! The helper as the agent runs it (ADR 0027 WP-11): its state for Settings, and Turn on / Repair / Update / Turn off as one
//! job with phases (ER-17/18/19). The OS prompt and the install are behind [`HelperInstaller`]; the helper's answers behind the
//! [`HelperLink`]; waiting behind a pause, so tests drive all of it with fakes and never touch the machine (ER-14).

use crate::helper_link::{HelperLink, HelperTransport};
use dyno_core::helper::HelperHost;
use dyno_core::helper_wire::{HelperAction, HelperState, HelperStatus};
use paddock_jobs::{JobContext, JobError, Phase};
use std::sync::Mutex;
use std::time::Duration;

/// How many times the job asks whether the started helper answers, and how long it pauses between asks (about a minute).
const ASKS: u32 = 120;
const PAUSE: Duration = Duration::from_millis(500);

/// The privileged install and removal behind the OS's one prompt.
pub trait HelperInstaller: Send {
    /// Installs the bundled helper (turn on, repair, update). Blocks until the prompt is answered and the install ran.
    ///
    /// # Errors
    /// What the user did or what went wrong, in words with the next step.
    fn install(&mut self) -> Result<(), String>;
    /// Removes the installed helper through the prompt (when it cannot remove itself).
    ///
    /// # Errors
    /// As [`HelperInstaller::install`].
    fn uninstall(&mut self) -> Result<(), String>;
}

/// The helper's state and the actions on it.
pub struct HelperCentre<T: HelperTransport> {
    link: HelperLink<T>,
    host: HelperHost,
    supported: Result<(), String>,
    version: String,
    installer: Mutex<Box<dyn HelperInstaller>>,
    pause: Box<dyn Fn(Duration) + Send + Sync>,
}

impl<T: HelperTransport> HelperCentre<T> {
    /// A centre over `link`. `supported` is `Err(reason)` where the helper cannot exist.
    pub fn new(
        link: HelperLink<T>,
        host: HelperHost,
        supported: Result<(), String>,
        version: &str,
        installer: Box<dyn HelperInstaller>,
    ) -> Self {
        HelperCentre {
            link,
            host,
            supported,
            version: version.to_string(),
            installer: Mutex::new(installer),
            pause: Box::new(std::thread::sleep),
        }
    }

    /// Replaces the pause between asks (tests pass a fake clock's advance).
    pub fn with_pause(mut self, pause: Box<dyn Fn(Duration) + Send + Sync>) -> Self {
        self.pause = pause;
        self
    }

    /// The helper link, for the readers.
    pub fn link(&self) -> &HelperLink<T> {
        &self.link
    }

    /// What Settings shows.
    pub fn status(&self) -> HelperStatus {
        self.link.status(self.host, self.supported.clone(), &self.version)
    }

    /// Runs `action` as the body of a job. Each phase says what is happening; Cancel works until the prompt is open, and the
    /// prompt phase says so; nothing here has a deadline the UI imposes.
    pub fn run(&self, ctx: &JobContext, action: HelperAction) -> Result<HelperStatus, JobError> {
        ctx.phase(Phase::discovering("check", "Checking the helper"));
        ctx.check()?;
        let before = self.status();
        if let Err(why) = &self.supported {
            return Err(JobError::Failed(why.clone()));
        }
        match action {
            HelperAction::TurnOn | HelperAction::Repair | HelperAction::Update => self.install(ctx, action, &before),
            HelperAction::TurnOff => self.remove(ctx, &before),
        }
    }

    fn prompt<R>(
        &self,
        ctx: &JobContext,
        label: &str,
        work: impl FnOnce(&mut dyn HelperInstaller) -> Result<R, String>,
    ) -> Result<R, JobError> {
        ctx.phase(Phase::discovering("prompt", label));
        ctx.check()?;
        let step = ctx.uncancellable("The system's permission prompt is open; Cancel takes effect when you have answered it.");
        let mut installer = self.installer.lock().unwrap_or_else(|p| p.into_inner());
        let out = work(&mut **installer);
        drop(step);
        drop(installer);
        ctx.check()?;
        out.map_err(JobError::Failed)
    }

    fn install(&self, ctx: &JobContext, action: HelperAction, before: &HelperStatus) -> Result<HelperStatus, JobError> {
        if action == HelperAction::TurnOn && before.state == HelperState::Ready {
            return Ok(before.clone());
        }
        self.prompt(ctx, "Waiting for the system's permission prompt", |i| i.install())?;
        ctx.phase(Phase::discovering("start", "Starting the helper"));
        for _ in 0..ASKS {
            ctx.check()?;
            let now = self.status();
            if now.state == HelperState::Ready {
                ctx.phase(Phase::discovering("verify", "Checking the helper answers"));
                log::info!(Action = format!("{action:?}").as_str(); "The helper job finished");
                return Ok(now);
            }
            if now.state == HelperState::Outdated {
                // It answers, but it is not a build this Dyno accepts: waiting longer changes nothing.
                return Err(JobError::Failed(format!(
                    "The helper started but this Dyno cannot use it. {}",
                    now.message
                )));
            }
            (self.pause)(PAUSE);
        }
        let last = self.status();
        Err(JobError::Failed(format!(
            "The helper was installed but did not answer within a minute ({}). Choose Repair, or open the log from Settings.",
            last.message
        )))
    }

    fn remove(&self, ctx: &JobContext, before: &HelperStatus) -> Result<HelperStatus, JobError> {
        if before.state == HelperState::Off {
            return Ok(before.clone());
        }
        ctx.phase(Phase::discovering("remove", "Asking the helper to remove itself"));
        // A running helper removes itself on an authenticated request (no prompt), except on Windows: a running program cannot be
        // deleted there, so the request leaves `dyno-helper.exe` behind (measured in the Windows 11 VM). Windows goes through the
        // prompt, where the bundled copy removes the installed one completely.
        let can_ask = matches!(before.state, HelperState::Ready | HelperState::Outdated) && self.host != HelperHost::Windows;
        let asked = can_ask && self.link.uninstall().is_ok();
        if !asked {
            self.prompt(ctx, "Waiting for the system's permission prompt", |i| i.uninstall())?;
        }
        ctx.phase(Phase::discovering("verify", "Checking the helper is gone"));
        for _ in 0..ASKS {
            ctx.check()?;
            let now = self.status();
            if matches!(now.state, HelperState::Off | HelperState::NotIncluded) {
                return Ok(now);
            }
            (self.pause)(PAUSE);
        }
        Err(JobError::Failed(
            "The helper is still answering after it was asked to go. Try again, or restart the computer.".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper_link::TransportError;
    use crate::jobs::testing::Run;
    use dyno_core::helper_policy::Visibility;
    use dyno_core::helper_wire::{CallerView, HelperInfo, HelperReply, HelperRequest, HELPER_API, MIN_HELPER_VERSION};
    use paddock_jobs::time::{Clock, FakeClock};
    use std::sync::Arc;

    /// A helper that answers `Info` only after `after` asks, and goes away on `Uninstall`.
    struct Machine {
        version: Mutex<String>,
        up: Mutex<bool>,
        asks_until_up: Mutex<u32>,
        installed: Mutex<bool>,
    }
    struct Fake(Arc<Machine>);
    impl HelperTransport for Fake {
        fn call(&self, request: HelperRequest) -> Result<HelperReply, TransportError> {
            let m = &self.0;
            match request {
                HelperRequest::Info => {
                    if !*m.installed.lock().unwrap() {
                        return Err(TransportError::Absent);
                    }
                    let mut n = m.asks_until_up.lock().unwrap();
                    if *n > 0 {
                        *n -= 1;
                        return Err(TransportError::Silent("starting".into()));
                    }
                    *m.up.lock().unwrap() = true;
                    Ok(HelperReply::About(
                        HelperInfo {
                            version: m.version.lock().unwrap().clone(),
                            api: HELPER_API,
                            host: HelperHost::Linux,
                            agent_exe: "/x".into(),
                        },
                        CallerView {
                            scope: Visibility::Full,
                            tools: vec![],
                        },
                    ))
                }
                HelperRequest::Audit(_) => Ok(HelperReply::Entries(vec![])),
                HelperRequest::Uninstall => {
                    *m.installed.lock().unwrap() = false;
                    Ok(HelperReply::Bye)
                }
                _ => Err(TransportError::Silent("unexpected".into())),
            }
        }
    }

    struct Prompt {
        machine: Arc<Machine>,
        answer: Result<(), String>,
        asked: Arc<Mutex<u32>>,
    }
    impl HelperInstaller for Prompt {
        fn install(&mut self) -> Result<(), String> {
            *self.asked.lock().unwrap() += 1;
            if self.answer.is_ok() {
                *self.machine.installed.lock().unwrap() = true;
            }
            self.answer.clone()
        }
        fn uninstall(&mut self) -> Result<(), String> {
            *self.asked.lock().unwrap() += 1;
            *self.machine.installed.lock().unwrap() = false;
            self.answer.clone()
        }
    }

    fn centre_v(
        installed: bool,
        asks_until_up: u32,
        answer: Result<(), String>,
        version: &str,
    ) -> (HelperCentre<Fake>, Arc<Mutex<u32>>, FakeClock) {
        let machine = Arc::new(Machine {
            version: Mutex::new(version.into()),
            up: Mutex::new(false),
            asks_until_up: Mutex::new(asks_until_up),
            installed: Mutex::new(installed),
        });
        let asked = Arc::new(Mutex::new(0));
        let clock = FakeClock::new();
        let c = HelperCentre::new(
            HelperLink::new(Fake(machine.clone()), Arc::new(clock.clone())),
            HelperHost::Linux,
            Ok(()),
            "1.1.0",
            Box::new(Prompt {
                machine,
                answer,
                asked: asked.clone(),
            }),
        );
        let tick = clock.clone();
        (c.with_pause(Box::new(move |d| tick.advance(d))), asked, clock)
    }

    fn centre(installed: bool, asks_until_up: u32, answer: Result<(), String>) -> (HelperCentre<Fake>, Arc<Mutex<u32>>, FakeClock) {
        centre_v(installed, asks_until_up, answer, MIN_HELPER_VERSION)
    }

    fn ids(run: &Run) -> Vec<String> {
        run.phases().into_iter().map(|p| p.id).collect()
    }

    #[test]
    fn turning_on_prompts_once_waits_for_the_helper_and_reports_each_phase() {
        let (c, asked, clock) = centre(false, 3, Ok(()));
        let run = Run::new();
        let out = run.job(|ctx| c.run(ctx, HelperAction::TurnOn)).unwrap();
        assert_eq!(out.state, HelperState::Ready);
        assert_eq!(*asked.lock().unwrap(), 1);
        assert_eq!(ids(&run), ["check", "prompt", "start", "verify"]);
        assert!(
            clock.monotonic() >= Duration::from_millis(1500),
            "it waited on the injected clock, not the wall clock"
        );
    }

    #[test]
    fn turning_on_a_ready_helper_asks_nothing_and_a_declined_prompt_changes_nothing() {
        let (c, asked, _) = centre(true, 0, Ok(()));
        assert_eq!(
            Run::new().job(|ctx| c.run(ctx, HelperAction::TurnOn)).unwrap().state,
            HelperState::Ready
        );
        assert_eq!(*asked.lock().unwrap(), 0);
        let (c, _, _) = centre(false, 0, Err("The permission prompt was declined, so nothing was changed.".into()));
        let err = Run::new().job(|ctx| c.run(ctx, HelperAction::TurnOn)).unwrap_err();
        assert!(matches!(err, JobError::Failed(m) if m.contains("declined")));
    }

    #[test]
    fn a_helper_that_answers_but_is_too_old_fails_at_once_and_says_so() {
        let (c, _, clock) = centre_v(false, 0, Ok(()), "1.0.0");
        let err = Run::new().job(|ctx| c.run(ctx, HelperAction::TurnOn)).unwrap_err();
        assert!(
            matches!(&err, JobError::Failed(m) if m.contains("cannot use it") && m.contains("needs helper version")),
            "{err:?}"
        );
        assert!(
            clock.monotonic() < Duration::from_secs(1),
            "no minute of waiting for a helper that is already answering"
        );
    }

    #[test]
    fn a_helper_that_never_answers_fails_with_the_next_step_after_about_a_minute() {
        let (c, _, clock) = centre(false, u32::MAX, Ok(()));
        let err = Run::new().job(|ctx| c.run(ctx, HelperAction::Repair)).unwrap_err();
        assert!(
            matches!(&err, JobError::Failed(m) if m.contains("Repair") && m.contains("log")),
            "{err:?}"
        );
        assert!(clock.monotonic() >= Duration::from_secs(59));
    }

    #[test]
    fn turning_off_asks_the_running_helper_first_and_prompts_only_when_it_cannot_answer() {
        let (c, asked, _) = centre(true, 0, Ok(()));
        let out = Run::new().job(|ctx| c.run(ctx, HelperAction::TurnOff)).unwrap();
        assert_eq!(out.state, HelperState::Off);
        assert_eq!(*asked.lock().unwrap(), 0, "it removed itself on the authenticated request");
        let (c, asked, _) = centre(true, 1, Ok(()));
        let out = Run::new().job(|ctx| c.run(ctx, HelperAction::TurnOff)).unwrap();
        assert_eq!(
            (out.state, *asked.lock().unwrap()),
            (HelperState::Off, 1),
            "not running: the prompt removes it"
        );
        let (c, asked, _) = centre(false, 0, Ok(()));
        assert_eq!(
            Run::new().job(|ctx| c.run(ctx, HelperAction::TurnOff)).unwrap().state,
            HelperState::Off
        );
        assert_eq!(*asked.lock().unwrap(), 0);
    }

    #[test]
    fn windows_turns_off_through_the_prompt_even_when_the_helper_answers() {
        let (c, asked, _) = centre(true, 0, Ok(()));
        let c = HelperCentre {
            host: HelperHost::Windows,
            ..c
        };
        let out = Run::new().job(|ctx| c.run(ctx, HelperAction::TurnOff)).unwrap();
        assert_eq!(
            (out.state, *asked.lock().unwrap()),
            (HelperState::Off, 1),
            "the bundled copy removes the installed one"
        );
    }

    #[test]
    fn where_the_helper_cannot_exist_the_job_says_why() {
        let machine = Arc::new(Machine {
            version: Mutex::new(MIN_HELPER_VERSION.into()),
            up: Mutex::new(false),
            asks_until_up: Mutex::new(0),
            installed: Mutex::new(false),
        });
        let c = HelperCentre::new(
            HelperLink::new(Fake(machine.clone()), Arc::new(FakeClock::new())),
            HelperHost::Linux,
            Err("This AppImage has no helper; install the .deb.".into()),
            "1.1.0",
            Box::new(Prompt {
                machine,
                answer: Ok(()),
                asked: Arc::default(),
            }),
        );
        let err = Run::new().job(|ctx| c.run(ctx, HelperAction::TurnOn)).unwrap_err();
        assert!(matches!(err, JobError::Failed(m) if m.contains("AppImage")));
        assert_eq!(c.status().state, HelperState::NotIncluded);
    }
}
