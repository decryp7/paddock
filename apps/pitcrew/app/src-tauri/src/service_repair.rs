//! Keeping the macOS background service registered: the pure parts, so they are
//! tested everywhere. The macOS glue in `service_install.rs` only implements [`RepairOps`] over `SMAppService` and
//! `launchctl`.
//!
//! Two things can keep the service from running there:
//! - **A stale registration.** After the app was replaced launchd keeps the launch constraint (LWCR: the cdhash of the
//!   ad-hoc signed build that was registered) and refuses to start the new binary: `job state = spawn failed`, exit 78
//!   (EX_CONFIG), `has LWCR`, looping. Registering again over the old job does not replace the constraint, so
//!   [`run_repair`] removes the job (unregister and `launchctl bootout`), waits until launchd has dropped it, and only
//!   then registers; it retries a bounded number of times and says every step to the log and to the UI
//!   ([`RepairTracker`]). Because every ad-hoc build has a new cdhash, [`plan`] also re-registers once after every
//!   version change instead of waiting for the failure.
//! - **The user turned it off** in System Settings, Login Items ([`disabled_in_login_items`]). That is the user's
//!   choice: it is reported, never repaired, and Pitcrew never switches it back on by itself.

use serde::Serialize;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How many full unregister, wait, register cycles follow the first re-registration while launchd still says stale.
pub const MAX_CYCLES: u32 = 3;
/// The pause after registering before launchd's report is read.
pub const SETTLE: Duration = Duration::from_secs(3);
/// How long launchd gets to drop the removed job before registering again.
pub const GONE_WAIT: Duration = Duration::from_secs(20);
/// How often launchd is asked whether the job is gone.
pub const GONE_POLL: Duration = Duration::from_secs(1);
/// How long a registered, not stale service gets to answer (launchd needed about a minute on the owner's Mac).
pub const START_WAIT: Duration = Duration::from_secs(120);
/// How often the service is asked while waiting.
pub const POLL: Duration = Duration::from_secs(3);
/// How long "Service is running again" stays on screen.
pub const SUCCESS_SHOWN: Duration = Duration::from_secs(120);

/// What the UI shows while the repair works.
pub const WORKING_MESSAGE: &str = "Repairing the background service: re-registering it with macOS… (this can take a minute)";
/// What the UI shows when it worked.
pub const SUCCESS_MESSAGE: &str = "Service is running again";
/// Next steps appended to a failure.
pub const NEXT_STEPS: &str =
    "Press Repair again, or restart the Mac. If Pitcrew is listed in System Settings ▸ General ▸ Login Items, switch it off and on there. The app log has each step.";

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

/// What `launchctl print gui/<uid>/<label>` says about the job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JobReport {
    /// launchd knows the job at all (an empty report means it dropped it).
    pub found: bool,
    /// `state = running`.
    pub running: bool,
    /// `job state = spawn failed`.
    pub spawn_failed: bool,
    /// `last exit code = 78` (EX_CONFIG): launchd refused the program's code identity.
    pub exit_config: bool,
    /// `needs LWCR update`.
    pub needs_lwcr_update: bool,
    /// `has LWCR`: a launch constraint from the registered build is stored.
    pub has_lwcr: bool,
}

/// Reads the report of `launchctl print` for the launch agent. Only the job's own keys count: nested blocks (`arguments`,
/// `environment`, and on macOS 26 the resource and jetsam coalitions with their own `state = active`) are skipped.
pub fn parse_job(launchctl_print: &str) -> JobReport {
    let mut job = JobReport::default();
    // 0 before the `<target> = {` header (fakes print bare lines), 1 inside it, more inside a nested block.
    let mut depth = 0usize;
    for line in launchctl_print.lines().map(str::trim) {
        if line == "}" {
            depth = depth.saturating_sub(1);
            continue;
        }
        let own = depth <= 1;
        if line.ends_with('{') {
            depth += 1;
            continue;
        }
        if !own {
            continue;
        }
        if line.starts_with("state = ") {
            job.found = true;
            job.running = line == "state = running";
        } else if line == "job state = spawn failed" {
            job.spawn_failed = true;
        } else if line.starts_with("last exit code = 78") {
            job.exit_config = true;
        } else if line.starts_with("properties = ") {
            job.needs_lwcr_update = line.contains("needs LWCR update");
            job.has_lwcr = line.contains("has LWCR");
        }
    }
    job
}

impl JobReport {
    /// Whether launchd holds a registration it can no longer start. macOS keeps the launch constraint of the build that
    /// was registered; every ad-hoc signed build has another cdhash, so after an update launchd refuses the new binary
    /// (spawn failed, exit 78 EX_CONFIG, "needs LWCR update") until the job is replaced. `has LWCR` alone is not a sign:
    /// every healthy `SMAppService` job has it, and a pending or restarting job is not refused. A running job is not
    /// refused whatever an earlier run left in the report.
    pub fn stale(&self) -> bool {
        self.found && !self.running && (self.spawn_failed || self.exit_config || self.needs_lwcr_update)
    }
}

/// Why a repair runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairReason {
    /// launchd refuses the job and the server does not answer.
    Stale,
    /// The app is a different version than the one that registered the service: a new cdhash.
    VersionChanged,
    /// The user pressed Repair.
    Requested,
}

/// What the app observed, for [`plan`].
pub struct Observed<'a> {
    /// `SMAppService.status`.
    pub status: RegistrationStatus,
    /// launchd's disabled mark ([`parse_print_disabled`]).
    pub launchd_disabled: Option<bool>,
    /// The server answers.
    pub answers: bool,
    /// launchd's report of the job.
    pub job: JobReport,
    /// The app version that registered the service, if recorded.
    pub registered_version: Option<&'a str>,
    /// This app's version.
    pub current_version: &'a str,
    /// The user asked for the repair.
    pub requested: bool,
}

/// What to do about the registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// Leave it alone.
    Nothing,
    /// Nothing is registered: just record that this version has been looked at.
    MarkCurrent,
    /// Replace the registration.
    Repair(RepairReason),
}

/// Decides whether to repair. What the user switched off in Login Items is never touched (not even when asked: the
/// repair would switch it back on); a missing launch agent cannot be repaired.
pub fn plan(o: &Observed) -> Plan {
    if o.status == RegistrationStatus::NotFound || disabled_in_login_items(o.status, o.launchd_disabled) {
        return Plan::Nothing;
    }
    if o.requested {
        return Plan::Repair(RepairReason::Requested);
    }
    if o.job.stale() && !o.answers {
        return Plan::Repair(RepairReason::Stale);
    }
    if o.registered_version != Some(o.current_version) {
        return if o.status == RegistrationStatus::Enabled || o.job.found {
            Plan::Repair(RepairReason::VersionChanged)
        } else {
            Plan::MarkCurrent
        };
    }
    Plan::Nothing
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
    /// Removes the job from launchd (`launchctl bootout`); a failure is only logged: the job may already be gone.
    fn bootout(&mut self) -> Result<(), String>;
    /// Whether launchd no longer knows the job.
    fn job_gone(&mut self) -> bool;
    /// Whether launchd now reports the registration as stale ([`JobReport::stale`]).
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

/// Replaces the registration: unregister, `launchctl bootout`, wait until launchd has dropped the job, register. Launchd
/// keeps the old launch constraint while the job lives, so registering over it does not help. While launchd still
/// reports the stale registration the cycle is repeated (at most [`MAX_CYCLES`] more times, waiting longer each time),
/// then the service gets at most [`START_WAIT`] to answer. `say` gets each step in words, for the log and the UI.
pub fn run_repair(ops: &mut dyn RepairOps, say: &mut dyn FnMut(&str)) -> RepairOutcome {
    say("re-registering the service");
    for cycle in 0..=MAX_CYCLES {
        if cycle > 0 {
            say(&format!(
                "launchd still reports a stale registration; retry {cycle} of {MAX_CYCLES}"
            ));
        }
        say("removing the old registration");
        if let Err(e) = ops.unregister() {
            say(&format!("unregister did not succeed (continuing): {e}"));
        }
        if let Err(e) = ops.bootout() {
            say(&format!("bootout did not succeed (continuing): {e}"));
        }
        let mut waited = Duration::ZERO;
        while !ops.job_gone() {
            if waited >= GONE_WAIT {
                say("launchd still holds the old job; registering anyway");
                break;
            }
            ops.pause(GONE_POLL);
            waited += GONE_POLL;
        }
        ops.pause(Duration::from_secs(2 * u64::from(cycle)));
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
    let why = format!(
        "launchd still refuses the service (needs LWCR update) after {} full re-registrations. {NEXT_STEPS}",
        MAX_CYCLES + 1
    );
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

    /// Starts a repair unless one is already working; `false` when it is.
    pub fn start(&self, now: Instant) -> bool {
        let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if guard.as_ref().is_some_and(|(n, _)| n.state == RepairState::Working) {
            return false;
        }
        *guard = Some((
            RepairNotice {
                state: RepairState::Working,
                message: WORKING_MESSAGE.to_string(),
            },
            now,
        ));
        true
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

    /// The whole of a real `launchctl print` of a healthy job (macOS 26.6.2, label and ids invented): the canonical
    /// capture of `paddock-service`, with its nested coalition blocks that print their own `state = active`.
    const REAL_RUNNING: &str = include_str!("../../../../../crates/paddock-service/tests/fixtures/launchctl-print-running.txt");

    #[test]
    fn a_real_running_print_reads_as_running_and_not_stale() {
        let job = parse_job(REAL_RUNNING);
        assert!(job.found && job.running && job.has_lwcr, "{job:?}");
        assert!(!job.spawn_failed && !job.exit_config && !job.needs_lwcr_update, "{job:?}");
        assert!(!job.stale());
    }

    #[test]
    fn a_pending_job_with_a_constraint_is_not_stale() {
        let pending = REAL_RUNNING.replacen("\tstate = running", "\tstate = spawn scheduled", 1);
        let job = parse_job(&pending);
        assert!(job.found && !job.running && job.has_lwcr && !job.stale(), "{job:?}");
    }

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

    /// The real `launchctl print gui/501/net.decryptology.pitcrew.service` of a Mac after an update (trimmed).
    const SPAWN_FAILED: &str = "gui/501/net.decryptology.pitcrew.service = {\n\tactive count = 0\n\tpath = (submitted by smd.362)\n\ttype = Submitted\n\tmanaged_by = com.apple.xpc.ServiceManagement\n\tstate = spawn scheduled\n\n\tprogram identifier = Contents/MacOS/pitcrew-server (mode: 2)\n\tparent bundle identifier = net.decryptology.pitcrew\n\tparent bundle version = 1.0.2\n\truns = 534\n\tlast exit code = 78: EX_CONFIG\n\n\tjob state = spawn failed\n\tsanitizer flags = 0x0\n\n\tproperties = partial import | keepalive | runatload | resolve program | has LWCR\n}\n";
    const HEALTHY: &str = "gui/501/net.decryptology.pitcrew.service = {\n\tstate = running\n\tpid = 88412\n\truns = 1\n\tlast exit code = (never exited)\n\tjob state = running\n\tproperties = keepalive | runatload | has LWCR\n}\n";

    #[test]
    fn a_registration_launchd_cannot_start_is_stale() {
        let job = parse_job(SPAWN_FAILED);
        assert!(job.found && !job.running && job.spawn_failed && job.exit_config && job.has_lwcr && !job.needs_lwcr_update);
        assert!(job.stale());
        // Each sign alone is enough.
        assert!(parse_job("\tstate = spawn scheduled\n\tlast exit code = 78: EX_CONFIG\n").stale());
        assert!(parse_job("\tstate = spawn scheduled\n\tjob state = spawn failed\n").stale());
        // `has LWCR` alone is every healthy job's mark, not a refusal.
        assert!(!parse_job("\tstate = spawn scheduled\n\tproperties = keepalive | has LWCR\n").stale());
        assert!(parse_job("\tstate = waiting\n\tproperties = keepalive | needs LWCR update\n").stale());
        let healthy = parse_job(HEALTHY);
        assert!(healthy.found && healthy.running && healthy.has_lwcr && !healthy.stale());
        // Gone from launchd.
        assert!(!parse_job("").found && !parse_job("").stale());
    }

    fn observed<'a>(job: JobReport) -> Observed<'a> {
        Observed {
            status: RegistrationStatus::Enabled,
            launchd_disabled: Some(false),
            answers: false,
            job,
            registered_version: Some("1.0.4"),
            current_version: "1.0.4",
            requested: false,
        }
    }

    #[test]
    fn the_plan_repairs_a_refused_job_and_a_new_version_but_never_what_the_user_switched_off() {
        let stale = parse_job(SPAWN_FAILED);
        let ok = parse_job(HEALTHY);
        // The owner's case: unlisted in Login Items (not registered), launchd holds the failing job.
        let mut o = observed(stale);
        o.status = RegistrationStatus::NotRegistered;
        assert_eq!(plan(&o), Plan::Repair(RepairReason::Stale));
        // The same job while the server answers is left alone.
        let mut o = observed(stale);
        o.answers = true;
        assert_eq!(plan(&o), Plan::Nothing);
        assert_eq!(plan(&observed(ok)), Plan::Nothing);
        // A new version is re-registered once, even when everything looks healthy.
        for registered in [None, Some("1.0.3")] {
            let mut o = observed(ok);
            o.answers = true;
            o.registered_version = registered;
            assert_eq!(plan(&o), Plan::Repair(RepairReason::VersionChanged));
        }
        // Nothing registered anywhere: only remember the version, never install without consent.
        let mut o = observed(JobReport::default());
        o.status = RegistrationStatus::NotRegistered;
        o.registered_version = None;
        assert_eq!(plan(&o), Plan::MarkCurrent);
        // The user's switch wins over everything, including a press of Repair.
        for requested in [false, true] {
            let mut o = observed(stale);
            o.status = RegistrationStatus::RequiresApproval;
            o.registered_version = None;
            o.requested = requested;
            assert_eq!(plan(&o), Plan::Nothing);
            let mut o = observed(stale);
            o.status = RegistrationStatus::NotRegistered;
            o.launchd_disabled = Some(true);
            o.requested = requested;
            assert_eq!(plan(&o), Plan::Nothing);
            let mut o = observed(stale);
            o.status = RegistrationStatus::NotFound;
            o.requested = requested;
            assert_eq!(plan(&o), Plan::Nothing);
        }
        // Repair on request.
        let mut o = observed(ok);
        o.answers = true;
        o.requested = true;
        assert_eq!(plan(&o), Plan::Repair(RepairReason::Requested));
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
        /// How many `job_gone` questions say no.
        job_lingers_for: u32,
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
                job_lingers_for: 0,
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
        fn bootout(&mut self) -> Result<(), String> {
            self.calls.push("bootout".into());
            Ok(())
        }
        fn job_gone(&mut self) -> bool {
            if self.job_lingers_for > 0 {
                self.job_lingers_for -= 1;
                false
            } else {
                true
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
    fn the_old_job_is_removed_and_gone_before_registering() {
        let mut f = Fake::new(0, Some(0));
        f.job_lingers_for = 3;
        let (outcome, said) = run(&mut f);
        assert_eq!(outcome, RepairOutcome::Recovered);
        assert_eq!(f.calls, ["unregister", "bootout", "register", "stale?"]);
        assert!(f.pause_total >= Duration::from_secs(3) + SETTLE);
        assert!(said.first().is_some_and(|m| m.contains("re-registering")));
    }

    #[test]
    fn a_job_that_never_goes_away_is_waited_for_a_bounded_time() {
        let mut f = Fake::new(0, Some(0));
        f.job_lingers_for = u32::MAX;
        let (outcome, said) = run(&mut f);
        assert_eq!(outcome, RepairOutcome::Recovered);
        assert!(f.pause_total >= GONE_WAIT && f.pause_total <= GONE_WAIT + SETTLE + Duration::from_secs(1));
        assert!(said.iter().any(|m| m.contains("still holds the old job")));
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
    fn a_stale_registration_is_retried_with_a_full_cycle_each_time() {
        let mut f = Fake::new(2, Some(0));
        let (outcome, said) = run(&mut f);
        assert_eq!(outcome, RepairOutcome::Recovered);
        let cycle = ["unregister", "bootout", "register", "stale?"];
        assert_eq!(f.calls, [cycle, cycle, cycle].concat());
        assert!(said.iter().any(|m| m.contains("retry 1 of 3")) && said.iter().any(|m| m.contains("retry 2 of 3")));
        // The wait before registering again grows with the retry.
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
        assert!(why.contains("needs LWCR update") && why.contains("4 full"), "{why}");
        assert_eq!(f.calls.iter().filter(|c| *c == "unregister").count(), MAX_CYCLES as usize + 1);
        assert_eq!(f.calls.iter().filter(|c| *c == "register").count(), MAX_CYCLES as usize + 1);
        assert!(said.iter().any(|m| m.contains("unregister did not succeed")));
    }

    #[test]
    fn approval_stops_the_repair_and_other_register_errors_are_retried() {
        let mut f = Fake::new(0, Some(0));
        f.register_results = vec![Err(RegisterError::NeedsApproval)];
        assert_eq!(run(&mut f).0, RepairOutcome::NeedsApproval);
        assert_eq!(f.calls, ["unregister", "bootout", "register"]);
        // A failed first register goes on to the check and the cycles.
        let mut f = Fake::new(1, Some(0));
        f.register_results = vec![Err(RegisterError::Other("busy".into()))];
        let (outcome, said) = run(&mut f);
        assert_eq!(outcome, RepairOutcome::Recovered);
        assert!(said.iter().any(|m| m.contains("register failed: busy")));
    }

    #[test]
    fn only_one_repair_works_at_a_time() {
        let t = RepairTracker::new();
        let t0 = Instant::now();
        assert!(t.start(t0));
        assert!(!t.start(t0));
        t.finish(&RepairOutcome::Failed("no".into()), t0);
        assert!(t.start(t0), "a finished repair can be started again");
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
