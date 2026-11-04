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
        RegisterError::Other(_) => None,
    }
}

/// Waits for the service to answer, bounded by `t.start_wait`; a cancel stops the wait (the registration stays).
fn wait_for_answer(
    b: &mut dyn Backend,
    answers: &mut dyn FnMut() -> bool,
    cancel: &AtomicBool,
    t: &Timings,
    say: &mut dyn FnMut(Step),
) -> RepairOutcome {
    say(Step::WaitingForStart);
    let mut waited = Duration::ZERO;
    loop {
        if answers() {
            say(Step::Answers);
            return RepairOutcome::Recovered;
        }
        if waited >= t.start_wait || cancel.load(Ordering::Relaxed) {
            say(Step::Note(format!(
                "The service is registered but did not answer within {} seconds; it may still start.",
                waited.as_secs()
            )));
            return RepairOutcome::Registered;
        }
        b.pause(t.start_poll);
        waited += t.start_poll;
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
    if cancel.load(Ordering::Relaxed) {
        return RepairOutcome::Cancelled;
    }
    if let Some(result) = b.replace_in_one_step() {
        say(Step::Cycle(0));
        return match result {
            Ok(()) => wait_for_answer(b, answers, cancel, t, say),
            Err(e) => stop_for(e.clone())
                .unwrap_or_else(|| RepairOutcome::Failed(format!("{e}. {NEXT_STEPS}"))),
        };
    }
    let mut last_error = None;
    for cycle in 0..=t.max_cycles {
        if cycle > 0 && cancel.load(Ordering::Relaxed) {
            // The previous cycle registered (possibly refused); nothing is left half-removed.
            return RepairOutcome::Cancelled;
        }
        say(Step::Cycle(cycle));
        say(Step::Removing);
        if let Err(e) = b.unregister() {
            say(Step::Note(format!(
                "Unregister did not succeed (going on): {e}"
            )));
        }
        if let Err(e) = b.evict() {
            say(Step::Note(format!(
                "Removing the job did not succeed (going on): {e}"
            )));
        }
        say(Step::WaitingForRemoval);
        let mut waited = Duration::ZERO;
        while !b.job_gone() {
            if waited >= t.gone_wait {
                say(Step::RemovalTimedOut);
                break;
            }
            b.pause(t.gone_poll);
            waited += t.gone_poll;
        }
        if cycle > 0 {
            b.pause(t.backoff * cycle);
        }
        say(Step::Registering);
        match b.register() {
            Ok(()) => last_error = None,
            Err(e) => {
                if let Some(stop) = stop_for(e.clone()) {
                    say(Step::Note(format!("Registering stopped: {e}")));
                    return stop;
                }
                say(Step::Note(format!("Registering failed: {e}")));
                last_error = Some(e.to_string());
            }
        }
        b.pause(t.settle);
        let job = b.inspect().job;
        if job == Job::Refused || (last_error.is_some() && (job == Job::Absent || !answers())) {
            continue;
        }
        return wait_for_answer(b, answers, cancel, t, say);
    }
    let why = match last_error {
        Some(e) => format!(
            "Registering failed {} times ({e}). {NEXT_STEPS}",
            t.max_cycles + 1
        ),
        None => format!(
            "The OS still refuses the service after {} full re-registrations. {NEXT_STEPS}",
            t.max_cycles + 1
        ),
    };
    say(Step::Note(why.clone()));
    RepairOutcome::Failed(why)
}

#[cfg(test)]
mod tests {
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
        fn register(&mut self) -> Result<(), RegisterError> {
            self.calls.push("register");
            let r = if self.register.is_empty() {
                Ok(())
            } else {
                self.register.remove(0)
            };
            self.failed_last = r.is_err();
            r
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
        fn replace_in_one_step(&mut self) -> Option<Result<(), RegisterError>> {
            self.one_step.take()
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
