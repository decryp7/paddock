//! Keeping the macOS background service registered (ADR 0003, ADR 0020's sibling problem): the pure parts, so they are
//! tested everywhere. The macOS glue in `service_install.rs` only implements [`RepairOps`] over `SMAppService` and
//! `launchctl`.
//!
//! Two things can keep the service from running there:
//! - **A stale registration.** After the app was replaced launchd keeps the old code requirement and refuses to start
//!   the new binary ("needs LWCR update", EX_CONFIG 78, spawn scheduled). Registering again fixes it, but launchd can take
//!   about a minute to start the job. [`run_repair`] does that, escalates to a full unregister, wait, register cycle while
//!   launchd still reports the stale state, is bounded, and says every step to the log and to the UI ([`RepairTracker`]).
//! - **The user turned it off** in System Settings, Login Items ([`disabled_in_login_items`]). That is the user's
//!   choice: it is reported, never repaired, and Pitcrew never switches it back on by itself.

use serde::Serialize;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How many full unregister, wait, register cycles follow the first re-registration while launchd still says stale.
pub const MAX_CYCLES: u32 = 3;
/// The pause after registering before launchd's report is read.
pub const SETTLE: Duration = Duration::from_secs(3);
/// How long a registered, not stale service gets to answer (launchd needed about a minute on the owner's Mac).
pub const START_WAIT: Duration = Duration::from_secs(120);
/// How often the service is asked while waiting.
pub const POLL: Duration = Duration::from_secs(3);
/// How long "Service is running again" stays on screen.
pub const SUCCESS_SHOWN: Duration = Duration::from_secs(120);

/// What the UI shows while the repair works.
pub const WORKING_MESSAGE: &str = "Re-registering the background service with macOS… (this can take a minute)";
/// What the UI shows when it worked.
pub const SUCCESS_MESSAGE: &str = "Service is running again";
/// Next steps appended to a failure.
pub const NEXT_STEPS: &str =
    "Open System Settings ▸ General ▸ Login Items and switch Pitcrew off and on again, or restart the Mac. The app log has each step.";

/// `SMAppService.status`, without the Objective-C type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationStatus {
    /// Registered and allowed.
    Enabled,
    /// Not registered (never installed, or removed/switched off).
    NotRegistered,
    /// Registered, but the user has to allow it in Login Items (or switched it off there).
    RequiresApproval,
    /// macOS cannot find the launch agent (the app is not where it can be registered from).
    NotFound,
}

/// Whether the user turned Pitcrew's background item off in System Settings. `launchd_disabled` is launchd's own
/// override database (`launchctl print-disabled`), `None` when it could not be read.
///
/// - `RequiresApproval` is how macOS reports an item the user switched off (or never allowed).
/// - `NotRegistered` alone is also a fresh install, so it only counts when launchd has the label marked disabled.
/// - `Enabled` with launchd saying disabled is the two views disagreeing; the user's switch wins.
pub fn disabled_in_login_items(status: RegistrationStatus, launchd_disabled: Option<bool>) -> bool {
    match status {
        RegistrationStatus::RequiresApproval => true,
        RegistrationStatus::NotRegistered | RegistrationStatus::Enabled => launchd_disabled == Some(true),
        RegistrationStatus::NotFound => false,
    }
}

/// Reads `label`'s entry in the output of `launchctl print-disabled gui/<uid>`: `"label" => disabled` (older systems
/// print `true`/`false`). `None` when the label is not listed.
pub fn parse_print_disabled(output: &str, label: &str) -> Option<bool> {
    let quoted = format!("\"{label}\"");
    output.lines().find_map(|line| {
        let rest = line.trim().strip_prefix(&quoted)?.trim_start().strip_prefix("=>")?;
        match rest.trim() {
            "disabled" | "true" => Some(true),
            "enabled" | "false" => Some(false),
            _ => None,
        }
    })
}

/// Whether `launchctl print` of the launch agent shows a registration launchd can no longer start. macOS keeps the
/// code requirement of the binary that was approved; a rebuilt, ad-hoc signed app has another identity, so launchd
/// refuses the new service ("needs LWCR update", spawn failed, exit 78 EX_CONFIG) until it is registered again.
pub fn stale_registration(launchctl_print: &str) -> bool {
    launchctl_print.contains("needs LWCR update")
        || launchctl_print.contains("job state = spawn failed")
        || launchctl_print.contains("last exit code = 78")
}

/// Why registering failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    /// macOS needs the user's approval (Login Items). Retrying cannot help.
    NeedsApproval,
    /// Anything else, in words.
    Other(String),
}

/// What [`run_repair`] needs from the system. The macOS implementation is thin; tests use a fake that records calls.
pub trait RepairOps {
    /// Registers the launch agent.
    fn register(&mut self) -> Result<(), RegisterError>;
    /// Unregisters it (a failure is only logged: launchd may already have dropped the job).
    fn unregister(&mut self) -> Result<(), String>;
    /// Whether launchd now reports the registration as stale ([`stale_registration`]).
    fn registration_stale(&mut self) -> bool;
    /// Whether the service answers.
    fn service_answers(&mut self) -> bool;
    /// Waits (tests do not).
    fn pause(&mut self, how_long: Duration);
}

/// How a repair ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepairOutcome {
    /// The service answers again.
    Recovered,
    /// macOS needs the user's approval; the user is pointed at Login Items.
    NeedsApproval,
    /// It did not come back; the text says why and what to do.
    Failed(String),
}

/// Re-registers the service, escalating to full unregister, wait, register cycles (at most [`MAX_CYCLES`]) while launchd
/// still reports the stale registration, then waits (at most [`START_WAIT`]) for the service to answer. `say` gets each
/// step in words, for the app log and the UI.
pub fn run_repair(ops: &mut dyn RepairOps, say: &mut dyn FnMut(&str)) -> RepairOutcome {
    say("re-registering the service");
    for cycle in 0..=MAX_CYCLES {
        if cycle > 0 {
            say(&format!(
                "launchd still reports a stale registration; full cycle {cycle} of {MAX_CYCLES}: unregister, wait, register"
            ));
            if let Err(e) = ops.unregister() {
                say(&format!("unregister did not succeed (continuing): {e}"));
            }
            ops.pause(Duration::from_secs(2 * u64::from(cycle)));
        }
        match ops.register() {
            Ok(()) => say("registered"),
            Err(RegisterError::NeedsApproval) => {
                say("macOS needs approval in Login Items; stopping");
                return RepairOutcome::NeedsApproval;
            }
            Err(RegisterError::Other(e)) => say(&format!("register failed: {e}")),
        }
        ops.pause(SETTLE);
        if ops.registration_stale() {
            continue;
        }
        say("launchd accepts the registration; waiting for the service to start");
        let mut waited = Duration::ZERO;
        loop {
            if ops.service_answers() {
                say("the service answers again");
                return RepairOutcome::Recovered;
            }
            if waited >= START_WAIT {
                break;
            }
            ops.pause(POLL);
            waited += POLL;
        }
        let why = format!(
            "the service was registered but did not start within {} seconds. {NEXT_STEPS}",
            START_WAIT.as_secs()
        );
        say(&why);
        return RepairOutcome::Failed(why);
    }
    let why = format!("launchd still refuses the service (needs LWCR update) after {MAX_CYCLES} full re-registrations. {NEXT_STEPS}");
    say(&why);
    RepairOutcome::Failed(why)
}

/// What the UI shows about the repair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairNotice {
    /// Where the repair is.
    pub state: RepairState,
    /// The words to show.
    pub message: String,
}

/// Where the repair is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RepairState {
    /// Working.
    Working,
    /// Done, the service runs.
    Succeeded,
    /// Did not work; the message has next steps.
    Failed,
}

/// The repair's progress, shared between the repair thread and the commands that report it.
pub struct RepairTracker {
    inner: Mutex<Option<(RepairNotice, Instant)>>,
}

impl RepairTracker {
    /// Nothing happened yet.
    pub const fn new() -> Self {
        Self { inner: Mutex::new(None) }
    }

    fn set(&self, state: RepairState, message: &str, now: Instant) {
        *self.inner.lock().unwrap_or_else(PoisonError::into_inner) = Some((
            RepairNotice {
                state,
                message: message.to_string(),
            },
            now,
        ));
    }

    /// A repair starts.
    pub fn begin(&self, now: Instant) {
        self.set(RepairState::Working, WORKING_MESSAGE, now);
    }

    /// A repair ended with `outcome`.
    pub fn finish(&self, outcome: &RepairOutcome, now: Instant) {
        match outcome {
            RepairOutcome::Recovered => self.set(RepairState::Succeeded, SUCCESS_MESSAGE, now),
            RepairOutcome::NeedsApproval => self.set(
                RepairState::Failed,
                "macOS needs your approval: allow Pitcrew in System Settings ▸ General ▸ Login Items.",
                now,
            ),
            RepairOutcome::Failed(why) => self.set(RepairState::Failed, why, now),
        }
    }

    /// The notice to show now: a success disappears after [`SUCCESS_SHOWN`]; working and failure stay until the next
    /// repair.
    pub fn current(&self, now: Instant) -> Option<RepairNotice> {
        let guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let (notice, since) = guard.as_ref()?;
        if notice.state == RepairState::Succeeded && now.saturating_duration_since(*since) > SUCCESS_SHOWN {
            return None;
        }
        Some(notice.clone())
    }
}

impl Default for RepairTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// The app's one tracker.
pub static REPAIR: RepairTracker = RepairTracker::new();

#[cfg(test)]
mod tests {
    use super::*;

    const LABEL: &str = "net.decryptology.pitcrew.service";

    #[test]
    fn the_disabled_mapping() {
        use RegistrationStatus::*;
        assert!(disabled_in_login_items(RequiresApproval, None));
        assert!(disabled_in_login_items(RequiresApproval, Some(false)));
        // A fresh install is "not registered" too: only launchd's disabled mark makes it the user's choice.
        assert!(!disabled_in_login_items(NotRegistered, None));
        assert!(!disabled_in_login_items(NotRegistered, Some(false)));
        assert!(disabled_in_login_items(NotRegistered, Some(true)));
        assert!(disabled_in_login_items(Enabled, Some(true)));
        assert!(!disabled_in_login_items(Enabled, None));
        assert!(!disabled_in_login_items(NotFound, Some(true)));
    }

    #[test]
    fn the_disabled_list_is_parsed() {
        let out = format!("disabled services = {{\n\t\"com.apple.x\" => enabled\n\t\"{LABEL}\" => disabled\n}}\n");
        assert_eq!(parse_print_disabled(&out, LABEL), Some(true));
        assert_eq!(parse_print_disabled(&out.replace("=> disabled", "=> enabled"), LABEL), Some(false));
        assert_eq!(parse_print_disabled(&out.replace("=> disabled", "=> true"), LABEL), Some(true));
        assert_eq!(parse_print_disabled(&out.replace("=> disabled", "=> false"), LABEL), Some(false));
        assert_eq!(parse_print_disabled(&out.replace("=> disabled", "=> maybe"), LABEL), None);
        assert_eq!(parse_print_disabled("", LABEL), None);
        assert_eq!(parse_print_disabled(&format!("\"{LABEL}\" disabled"), LABEL), None);
    }

    #[test]
    fn a_registration_launchd_cannot_start_is_stale() {
        let failed = "\tstate = spawn scheduled\n\tlast exit code = 78: EX_CONFIG\n\tjob state = spawn failed\n\tproperties = partial import | keepalive | needs LWCR update | has LWCR\n";
        assert!(stale_registration(failed));
        assert!(stale_registration("\tproperties = keepalive | needs LWCR update\n"));
        let healthy =
            "\tstate = running\n\tpid = 88412\n\tlast exit code = (never exited)\n\tproperties = keepalive | runatload | has LWCR\n";
        assert!(!stale_registration(healthy));
        assert!(!stale_registration(""));
    }

    /// A fake system: `stale_for` reports stale for that many checks, the service answers after `answers_after` asks.
    struct Fake {
        calls: Vec<String>,
        stale_for: u32,
        answers_after: Option<u32>,
        asked: u32,
        pause_total: Duration,
        register_results: Vec<Result<(), RegisterError>>,
        unregister_fails: bool,
    }

    impl Fake {
        fn new(stale_for: u32, answers_after: Option<u32>) -> Self {
            Self {
                calls: Vec::new(),
                stale_for,
                answers_after,
                asked: 0,
                pause_total: Duration::ZERO,
                register_results: Vec::new(),
                unregister_fails: false,
            }
        }
    }

    impl RepairOps for Fake {
        fn register(&mut self) -> Result<(), RegisterError> {
            self.calls.push("register".into());
            if self.register_results.is_empty() {
                Ok(())
            } else {
                self.register_results.remove(0)
            }
        }
        fn unregister(&mut self) -> Result<(), String> {
            self.calls.push("unregister".into());
            if self.unregister_fails {
                Err("not found".into())
            } else {
                Ok(())
            }
        }
        fn registration_stale(&mut self) -> bool {
            self.calls.push("stale?".into());
            if self.stale_for > 0 {
                self.stale_for -= 1;
                true
            } else {
                false
            }
        }
        fn service_answers(&mut self) -> bool {
            self.asked += 1;
            self.answers_after.is_some_and(|n| self.asked > n)
        }
        fn pause(&mut self, how_long: Duration) {
            self.pause_total += how_long;
        }
    }

    fn run(f: &mut Fake) -> (RepairOutcome, Vec<String>) {
        let mut said = Vec::new();
        let outcome = run_repair(f, &mut |m| said.push(m.to_string()));
        (outcome, said)
    }

    #[test]
    fn one_re_registration_is_enough_when_launchd_accepts_it() {
        let mut f = Fake::new(0, Some(0));
        let (outcome, said) = run(&mut f);
        assert_eq!(outcome, RepairOutcome::Recovered);
        assert_eq!(f.calls, ["register", "stale?"]);
        assert!(said.first().is_some_and(|m| m.contains("re-registering")));
    }

    #[test]
    fn the_service_gets_a_minute_to_come_up_and_the_wait_is_bounded() {
        // Answers after 20 asks (60 s of polling), like the owner's Mac.
        let mut f = Fake::new(0, Some(20));
        assert_eq!(run(&mut f).0, RepairOutcome::Recovered);
        assert!(f.pause_total >= Duration::from_secs(60) && f.pause_total < START_WAIT + SETTLE + POLL);
        // Never answers: gives up at START_WAIT with next steps.
        let mut f = Fake::new(0, None);
        let (outcome, _) = run(&mut f);
        let RepairOutcome::Failed(why) = outcome else {
            panic!("{outcome:?}")
        };
        assert!(why.contains("did not start") && why.contains("Login Items"), "{why}");
        assert!(f.pause_total <= SETTLE + START_WAIT + POLL);
    }

    #[test]
    fn a_stale_registration_escalates_to_full_cycles() {
        let mut f = Fake::new(2, Some(0));
        let (outcome, said) = run(&mut f);
        assert_eq!(outcome, RepairOutcome::Recovered);
        assert_eq!(
            f.calls,
            [
                "register",
                "stale?",
                "unregister",
                "register",
                "stale?",
                "unregister",
                "register",
                "stale?"
            ]
        );
        assert!(said.iter().any(|m| m.contains("full cycle 1 of 3")) && said.iter().any(|m| m.contains("full cycle 2 of 3")));
        // The wait before registering again grows with the cycle.
        assert!(f.pause_total >= Duration::from_secs(2 + 4) + SETTLE * 3);
    }

    #[test]
    fn a_registration_that_stays_stale_fails_after_the_bounded_retries() {
        let mut f = Fake::new(u32::MAX, Some(0));
        f.unregister_fails = true;
        let (outcome, said) = run(&mut f);
        let RepairOutcome::Failed(why) = outcome else {
            panic!("{outcome:?}")
        };
        assert!(why.contains("needs LWCR update") && why.contains("3 full"), "{why}");
        assert_eq!(f.calls.iter().filter(|c| *c == "unregister").count(), MAX_CYCLES as usize);
        assert_eq!(f.calls.iter().filter(|c| *c == "register").count(), MAX_CYCLES as usize + 1);
        assert!(said.iter().any(|m| m.contains("unregister did not succeed")));
    }

    #[test]
    fn approval_stops_the_repair_and_other_register_errors_are_retried() {
        let mut f = Fake::new(0, Some(0));
        f.register_results = vec![Err(RegisterError::NeedsApproval)];
        assert_eq!(run(&mut f).0, RepairOutcome::NeedsApproval);
        assert_eq!(f.calls, ["register"]);
        // A failed first register goes on to the check and the cycles.
        let mut f = Fake::new(1, Some(0));
        f.register_results = vec![Err(RegisterError::Other("busy".into()))];
        let (outcome, said) = run(&mut f);
        assert_eq!(outcome, RepairOutcome::Recovered);
        assert!(said.iter().any(|m| m.contains("register failed: busy")));
    }

    #[test]
    fn the_tracker_reports_progress_and_lets_a_success_fade() {
        let t = RepairTracker::default();
        let t0 = Instant::now();
        assert_eq!(t.current(t0), None);
        t.begin(t0);
        let n = t.current(t0).unwrap();
        assert_eq!((n.state, n.message.as_str()), (RepairState::Working, WORKING_MESSAGE));
        // Working never fades.
        assert!(t.current(t0 + Duration::from_secs(3600)).is_some());
        t.finish(&RepairOutcome::Recovered, t0);
        assert_eq!(t.current(t0 + Duration::from_secs(5)).unwrap().message, SUCCESS_MESSAGE);
        assert_eq!(t.current(t0 + SUCCESS_SHOWN + Duration::from_secs(1)), None);
        t.finish(&RepairOutcome::Failed("no".into()), t0);
        let n = t.current(t0 + Duration::from_secs(3600)).unwrap();
        assert_eq!((n.state, n.message.as_str()), (RepairState::Failed, "no"));
        t.finish(&RepairOutcome::NeedsApproval, t0);
        assert!(t.current(t0).unwrap().message.contains("Login Items"));
    }

    #[test]
    fn a_poisoned_lock_is_recovered() {
        static T: RepairTracker = RepairTracker::new();
        let _ = std::thread::spawn(|| {
            let _g = T.inner.lock().unwrap();
            panic!("poison");
        })
        .join();
        T.begin(Instant::now());
        assert!(T.current(Instant::now()).is_some());
    }

    #[test]
    fn the_notice_serialises_in_camel_case() {
        let json = serde_json::to_string(&RepairNotice {
            state: RepairState::Working,
            message: "m".into(),
        })
        .unwrap();
        assert_eq!(json, r#"{"state":"working","message":"m"}"#);
    }
}
