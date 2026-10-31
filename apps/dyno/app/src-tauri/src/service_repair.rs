//! Keeping the agent's macOS launch agent registered across updates (Paddock rule 24): the pure parts, so they are
//! tested on every OS. `login.rs` only implements [`RepairOps`] over `SMAppService` and `launchctl`; nothing here or in
//! the tests touches a real launch agent.
//!
//! Ported from Pitwall (`service_repair.rs`, where it was reproduced on a Mac). Two things keep the agent from running:
//! - **A stale registration.** After the app was replaced launchd keeps the launch constraint (LWCR: the cdhash of the
//!   ad-hoc signed build that was registered) and refuses to start the new binary: `job state = spawn failed`, exit 78
//!   (EX_CONFIG), `has LWCR`, looping. Registering again over the old job does not replace the constraint, so
//!   [`run_repair`] removes the job (unregister and `launchctl bootout`), waits until launchd has dropped it, and only
//!   then registers; it retries a bounded number of times and logs every step. Because every ad-hoc build has a new
//!   cdhash, [`plan`] also re-registers once after every version change ([`registered_version`]) instead of waiting for
//!   the failure.
//! - **The user turned it off** in System Settings, Login Items ([`disabled_in_login_items`]). That is the user's
//!   choice: it is never repaired, and Dyno never switches it back on by itself.

use std::path::Path;
use std::time::Duration;

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

/// Next steps appended to a failure.
pub const NEXT_STEPS: &str =
    "Quit and reopen Dyno, or restart the Mac. If Dyno is listed in System Settings ▸ General ▸ Login Items, switch it off and on there. Dyno's log has each step.";

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

/// Whether the user turned Dyno's background item off in System Settings. `launchd_disabled` is launchd's own
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

/// Whether launchd starts the agent now and restarts it (`KeepAlive`), so the app must wait for it: the item is enabled,
/// the user did not switch it off in Login Items ([`disabled_in_login_items`]) and launchd still has the job loaded
/// (`job.found`). Anything less supervises nothing, and waiting for it would wait for an agent that never comes.
pub fn service_supervises(status: RegistrationStatus, launchd_disabled: Option<bool>, job: JobReport) -> bool {
    status == RegistrationStatus::Enabled && !disabled_in_login_items(status, launchd_disabled) && job.found
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

/// Reads the report of `launchctl print` for the launch agent. Only the job's own lines count: a real print holds
/// nested blocks (`resource coalition = { ... state = active ... }`, `jetsam coalition`, `environment`, `arguments`)
/// whose lines must never overwrite the job's `state = running`. The outer `gui/<uid>/<label> = {` is depth one; a
/// line deeper than that is skipped.
pub fn parse_job(launchctl_print: &str) -> JobReport {
    let mut job = JobReport::default();
    let mut depth = 0usize;
    for line in launchctl_print.lines().map(str::trim) {
        if line == "}" {
            depth = depth.saturating_sub(1);
            continue;
        }
        let opens = line.ends_with("= {");
        if depth <= 1 {
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
        if opens {
            depth += 1;
        }
    }
    job
}

impl JobReport {
    /// Whether launchd holds a registration it can no longer start. macOS keeps the launch constraint of the build that
    /// was registered; every ad-hoc signed build has another cdhash, so after an update launchd refuses the new binary
    /// (spawn failed, exit 78 EX_CONFIG, "needs LWCR update" when it says so) until the job is replaced. `has LWCR` alone
    /// is not a sign: every healthy `SMAppService` job has it, and a pending or restarting job is not refused.
    pub fn stale(&self) -> bool {
        self.found && (self.spawn_failed || self.exit_config || self.needs_lwcr_update)
    }

    /// The observed state in words, for the log and the failure message.
    pub fn describe(&self) -> String {
        if !self.found {
            return "launchd does not know the job".into();
        }
        let mut parts = vec![if self.running { "running" } else { "not running" }];
        for (on, what) in [
            (self.spawn_failed, "spawn failed"),
            (self.exit_config, "exit code 78 (EX_CONFIG)"),
            (self.needs_lwcr_update, "needs LWCR update"),
            (self.has_lwcr, "has a launch constraint"),
        ] {
            if on {
                parts.push(what);
            }
        }
        parts.join(", ")
    }
}

/// Runs `program` and returns its standard output, `None` when it cannot be run, fails to be read, or has not finished
/// within `timeout` (it is then killed). The output is read while the program runs: a program that prints more than a
/// pipe holds (64 KiB; `launchctl print-disabled` lists every service of the domain) would otherwise block on its write,
/// never exit, and cost the whole timeout on every look.
pub fn run_bounded(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    // Callers pass fixed system tools; no console window to hide, and the wait is bounded.
    #[allow(clippy::disallowed_methods)]
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("dyno-tool-output".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            let _ = tx.send(stdout.read_to_end(&mut bytes).map(|_| bytes));
        })
        .ok()?;
    let read = rx.recv_timeout(timeout);
    // Reading ended (the program closed its output) or the time is up: either way it is reaped, never left behind.
    if read.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    let bytes = read.ok()?.ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Why a repair runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairReason {
    /// launchd refuses the job and the agent does not answer.
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
    /// The agent answers.
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
    // A running job is never replaced because the app has not connected yet: the agent is up, and killing it is the harm.
    if o.job.stale() && !o.answers && !o.job.running {
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
    /// What launchd now reports about the job ([`JobReport::stale`] says whether it is refused).
    fn job(&mut self) -> JobReport;
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
    let mut last = JobReport::default();
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
        last = ops.job();
        if last.stale() {
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
        "launchd still reports a refused registration after {} full re-registrations (observed: {}). {NEXT_STEPS}",
        MAX_CYCLES + 1,
        last.describe()
    );
    say(&why);
    RepairOutcome::Failed(why)
}

/// Carries out a [`plan`] at start-up: nothing, only remembering this version (`record`), or a full [`run_repair`]
/// that records the version when the agent runs again. `Ok(true)` when the registration was replaced; the error says
/// why it did not come back and what to do. `say` gets every step for the log.
pub fn keep_registered(plan: Plan, ops: &mut dyn RepairOps, record: &mut dyn FnMut(), say: &mut dyn FnMut(&str)) -> Result<bool, String> {
    match plan {
        Plan::Nothing => Ok(false),
        Plan::MarkCurrent => {
            record();
            Ok(false)
        }
        Plan::Repair(reason) => {
            say(&format!("replacing the agent's registration ({reason:?})"));
            match run_repair(ops, say) {
                RepairOutcome::Recovered => {
                    record();
                    Ok(true)
                }
                RepairOutcome::NeedsApproval => {
                    Err("macOS needs your approval: allow Dyno in System Settings > General > Login Items".into())
                }
                RepairOutcome::Failed(why) => Err(why),
            }
        }
    }
}

/// The file in Dyno's data folder that records which app version last registered the agent service.
pub const REGISTERED_VERSION_FILE: &str = "service-registered-version";

/// The app version that last registered the agent service, from `file`. `None` when it is missing, unreadable or not a
/// version (a damaged file only means one extra re-registration).
pub fn registered_version(file: &Path) -> Option<String> {
    let text = std::fs::read_to_string(file).ok()?;
    let v = text.trim();
    let plausible = !v.is_empty()
        && v.len() <= 64
        && v.starts_with(|c: char| c.is_ascii_digit())
        && v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
    plausible.then(|| v.to_string())
}

/// Records that `version` registered the agent service.
pub fn record_registered_version(file: &Path, version: &str) -> Result<(), String> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    std::fs::write(file, format!("{version}\n")).map_err(|e| format!("could not write {}: {e}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_agent::service::LABEL;

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
    fn launchd_supervises_only_an_enabled_item_the_user_did_not_switch_off_whose_job_is_loaded() {
        use RegistrationStatus::*;
        let loaded = JobReport {
            found: true,
            ..JobReport::default()
        };
        assert!(service_supervises(Enabled, None, loaded));
        assert!(service_supervises(Enabled, Some(false), loaded));
        // Enabled for SMAppService but off in Login Items (launchd's override database): nothing runs the agent.
        assert!(!service_supervises(Enabled, Some(true), loaded));
        // Waiting for approval, not registered, not found: launchd runs nothing.
        assert!(!service_supervises(RequiresApproval, None, loaded));
        assert!(!service_supervises(NotRegistered, None, loaded));
        assert!(!service_supervises(NotFound, None, loaded));
        // Enabled, but launchd dropped the job (or `launchctl print` could not be read): it starts nothing.
        assert!(!service_supervises(Enabled, None, JobReport::default()));
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

    /// The real `launchctl print gui/501/net.decryptology.dyno.agent` of a Mac after an update (trimmed).
    const SPAWN_FAILED: &str = "gui/501/net.decryptology.dyno.agent = {\n\tactive count = 0\n\tpath = (submitted by smd.362)\n\ttype = Submitted\n\tmanaged_by = com.apple.xpc.ServiceManagement\n\tstate = spawn scheduled\n\n\tprogram identifier = Contents/MacOS/dyno-agent (mode: 2)\n\tparent bundle identifier = net.decryptology.dyno\n\tparent bundle version = 0.1.0\n\truns = 534\n\tlast exit code = 78: EX_CONFIG\n\n\tjob state = spawn failed\n\tsanitizer flags = 0x0\n\n\tproperties = partial import | keepalive | runatload | resolve program | has LWCR\n}\n";
    const HEALTHY: &str = "gui/501/net.decryptology.dyno.agent = {\n\tstate = running\n\tpid = 88412\n\truns = 1\n\tlast exit code = (never exited)\n\tjob state = running\n\tproperties = keepalive | runatload | has LWCR\n}\n";

    #[test]
    fn a_registration_launchd_cannot_start_is_stale() {
        let job = parse_job(SPAWN_FAILED);
        assert!(job.found && !job.running && job.spawn_failed && job.exit_config && job.has_lwcr && !job.needs_lwcr_update);
        assert!(job.stale());
        // Each sign alone is enough.
        assert!(parse_job("\tstate = spawn scheduled\n\tlast exit code = 78: EX_CONFIG\n").stale());
        assert!(parse_job("\tstate = spawn scheduled\n\tjob state = spawn failed\n").stale());
        assert!(parse_job("\tstate = waiting\n\tproperties = keepalive | needs LWCR update\n").stale());
        let healthy = parse_job(HEALTHY);
        assert!(healthy.found && healthy.running && healthy.has_lwcr && !healthy.stale());
        // Gone from launchd.
        assert!(!parse_job("").found && !parse_job("").stale());
    }

    /// The real, untrimmed `launchctl print` of a healthy job (macOS 26.6.2 VM, 2026-10-31): nested `resource coalition`
    /// and `jetsam coalition` blocks carry their own `state = active` lines after the job's `state = running`.
    const REAL_RUNNING: &str = include_str!("../tests/fixtures/launchctl-print-running.txt");

    #[cfg(unix)]
    #[test]
    fn a_tool_that_prints_more_than_a_pipe_holds_is_read_in_full() {
        // 300 kB: more than the 64 KiB a pipe buffers. Read only after exit, the tool would block and time out.
        let out = run_bounded("/bin/sh", &["-c", "head -c 300000 /dev/zero | tr '\\0' a"], Duration::from_secs(20));
        assert_eq!(out.map(|o| o.len()), Some(300_000));
    }

    #[cfg(unix)]
    #[test]
    fn a_tool_that_does_not_finish_is_killed_at_the_timeout_and_one_that_is_missing_gives_none() {
        let started = std::time::Instant::now();
        assert_eq!(run_bounded("/bin/sh", &["-c", "exec sleep 30"], Duration::from_millis(300)), None);
        assert!(started.elapsed() < Duration::from_secs(10), "killed at the timeout, not waited for");
        assert_eq!(run_bounded("/nonexistent/tool", &[], Duration::from_secs(1)), None);
        assert_eq!(run_bounded("/bin/sh", &["-c", "printf ok"], Duration::from_secs(20)), Some("ok".to_string()));
    }

    #[test]
    fn a_real_running_print_reads_running_and_is_not_stale() {
        let job = parse_job(REAL_RUNNING);
        assert!(job.found && job.running && job.has_lwcr, "{job:?}");
        assert!(!job.spawn_failed && !job.exit_config && !job.needs_lwcr_update, "{job:?}");
        assert!(!job.stale(), "a healthy job that merely has a launch constraint is not stale");
    }

    #[test]
    fn nested_blocks_never_overwrite_the_jobs_own_lines() {
        // A real print edited to the other states launchd reports: the nested `state = active` still must not count.
        let pending = REAL_RUNNING.replacen("\tstate = running", "\tstate = spawn scheduled", 1).replace("job state = running", "job state = pending");
        let job = parse_job(&pending);
        assert!(job.found && !job.running && !job.stale(), "pending is not stale: {job:?}");
        let failed = pending.replace("job state = pending", "job state = spawn failed");
        assert!(parse_job(&failed).stale());
        // `last exit code = 78` inside a nested block is not the job's.
        let nested = REAL_RUNNING.replace("\t\tname = net.decryptology.dyno.agent\n\t}\n\n\tjetsam", "\t\tlast exit code = 78\n\t}\n\n\tjetsam");
        assert!(!parse_job(&nested).exit_config, "{nested}");
    }

    #[test]
    fn a_running_job_is_never_replaced_because_the_app_has_not_connected_yet() {
        let mut o = observed(parse_job(REAL_RUNNING));
        o.answers = false;
        assert_eq!(plan(&o), Plan::Nothing);
        // Even when it holds a refusal sign from an earlier run, a running job is left alone.
        o.job.exit_config = true;
        assert_eq!(plan(&o), Plan::Nothing);
    }

    #[test]
    fn a_job_between_restarts_with_a_constraint_is_not_stale() {
        let between = parse_job("\tstate = spawn scheduled\n\tlast exit code = 3\n\tproperties = keepalive | has LWCR\n");
        assert!(between.found && !between.stale());
    }

    fn observed<'a>(job: JobReport) -> Observed<'a> {
        Observed {
            status: RegistrationStatus::Enabled,
            launchd_disabled: Some(false),
            answers: false,
            job,
            registered_version: Some("0.1.1"),
            current_version: "0.1.1",
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
        // The same job while the agent answers is left alone.
        let mut o = observed(stale);
        o.answers = true;
        assert_eq!(plan(&o), Plan::Nothing);
        assert_eq!(plan(&observed(ok)), Plan::Nothing);
        // A new version is re-registered once, even when everything looks healthy.
        for registered in [None, Some("0.1.0")] {
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
        /// What the job reports while it is stale.
        stale_job: JobReport,
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
                stale_job: JobReport { found: true, spawn_failed: true, has_lwcr: true, ..JobReport::default() },
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
        fn job(&mut self) -> JobReport {
            self.calls.push("stale?".into());
            if self.stale_for > 0 {
                self.stale_for -= 1;
                self.stale_job
            } else {
                JobReport { found: true, running: true, ..JobReport::default() }
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
        // Answers after 20 asks (60 s of polling), like Pitwall on the owner's Mac.
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
        assert!(why.contains("4 full") && why.contains("observed: not running, spawn failed"), "{why}");
        assert!(!why.contains("needs LWCR update"), "the reason is what launchd said, not a made-up one: {why}");
        assert_eq!(f.calls.iter().filter(|c| *c == "unregister").count(), MAX_CYCLES as usize + 1);
        assert_eq!(f.calls.iter().filter(|c| *c == "register").count(), MAX_CYCLES as usize + 1);
        assert!(said.iter().any(|m| m.contains("unregister did not succeed")));
    }

    #[test]
    fn the_failure_message_names_the_lwcr_update_only_when_launchd_said_so() {
        let mut f = Fake::new(u32::MAX, Some(0));
        f.stale_job = JobReport { found: true, needs_lwcr_update: true, ..JobReport::default() };
        let RepairOutcome::Failed(why) = run(&mut f).0 else { panic!() };
        assert!(why.contains("needs LWCR update"), "{why}");
        assert_eq!(JobReport::default().describe(), "launchd does not know the job");
        assert!(JobReport { found: true, running: true, exit_config: true, has_lwcr: true, ..JobReport::default() }
            .describe()
            .contains("running, exit code 78 (EX_CONFIG), has a launch constraint"));
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
    fn keeping_registered_records_the_version_only_when_the_agent_runs() {
        let mut said = Vec::new();
        let mut recorded = 0;
        let mut f = Fake::new(0, Some(0));
        assert_eq!(
            keep_registered(Plan::Nothing, &mut f, &mut || recorded += 1, &mut |m| said.push(m.to_string())),
            Ok(false)
        );
        assert!(f.calls.is_empty() && recorded == 0, "a healthy, unchanged service is not touched");
        assert_eq!(
            keep_registered(Plan::MarkCurrent, &mut f, &mut || recorded += 1, &mut |_| {}),
            Ok(false)
        );
        assert!(
            f.calls.is_empty() && recorded == 1,
            "nothing registered: only the version is remembered"
        );
        let r = keep_registered(
            Plan::Repair(RepairReason::VersionChanged),
            &mut f,
            &mut || recorded += 1,
            &mut |m| said.push(m.to_string()),
        );
        assert_eq!(r, Ok(true));
        assert_eq!(recorded, 2);
        assert!(said.iter().any(|m| m.contains("VersionChanged")), "{said:?}");
        // A repair that does not bring the agent back keeps the old version, so the next start tries again.
        let mut f = Fake::new(0, None);
        let r = keep_registered(Plan::Repair(RepairReason::Stale), &mut f, &mut || recorded += 1, &mut |_| {});
        assert!(r.unwrap_err().contains("did not start"));
        let mut f = Fake::new(0, Some(0));
        f.register_results = vec![Err(RegisterError::NeedsApproval)];
        let r = keep_registered(Plan::Repair(RepairReason::Stale), &mut f, &mut || recorded += 1, &mut |_| {});
        assert!(r.unwrap_err().contains("Login Items"));
        assert_eq!(recorded, 2);
    }

    #[test]
    fn the_registered_version_is_remembered_and_a_damaged_file_is_no_version() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub").join(REGISTERED_VERSION_FILE);
        assert_eq!(registered_version(&file), None, "nothing recorded yet");
        record_registered_version(&file, "0.1.1").unwrap();
        assert_eq!(registered_version(&file).as_deref(), Some("0.1.1"));
        record_registered_version(&file, "1.2.3-beta.1+mac").unwrap();
        assert_eq!(registered_version(&file).as_deref(), Some("1.2.3-beta.1+mac"));
        for junk in ["", "  \n", "garbage", "1.0 two", &"9".repeat(65)] {
            std::fs::write(&file, junk).unwrap();
            assert_eq!(registered_version(&file), None, "{junk:?}");
        }
        std::fs::write(&file, [0xff, 0xfe]).unwrap();
        assert_eq!(registered_version(&file), None, "not UTF-8");
    }

    /// The whole flow after an update, over fakes: the version differs, so the healthy-looking job is replaced once
    /// and the new version is recorded; the next start leaves it alone.
    #[test]
    fn an_update_re_registers_once_and_then_leaves_the_service_alone() {
        let ok = parse_job(HEALTHY);
        let mut o = observed(ok);
        o.answers = true;
        o.registered_version = Some("0.1.0");
        assert_eq!(plan(&o), Plan::Repair(RepairReason::VersionChanged));
        let mut f = Fake::new(0, Some(0));
        assert_eq!(run(&mut f).0, RepairOutcome::Recovered);
        assert_eq!(f.calls, ["unregister", "bootout", "register", "stale?"]);
        o.registered_version = Some("0.1.1");
        assert_eq!(plan(&o), Plan::Nothing);
    }
}
