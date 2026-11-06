//! The agent's systemd user unit on Linux, through the shared service manager (ADR 0056 section 6): [`Manager`] over the
//! [`Systemd`] backend. The `dyno-agent service` verbs and the app's Settings switch, its repair at start, its Quit and
//! its resume are thin callers of it, so one transition decides what is written, enabled, started or left alone (the
//! user's `systemctl --user disable` is theirs; nothing is installed unasked; a restart loop is systemd's).
//!
//! Everything here is pure over the [`System`] port and an environment table, so it is tested on every OS against the
//! fake systemd of `paddock_service::fake_systemd` and never touches a real unit (ER-14).

use super::{Verbs, LABEL};
use paddock_env::Vars;
use paddock_service::backend::{Systemd, Target};
use paddock_service::{Manager, Plan, Reason, RepairOutcome, ServiceError, ServiceSpec, Started, System, Timings};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// The unit's name without `.service`.
pub const UNIT: &str = "dyno-agent";

/// How many more looks a registration waits for another registration or repair that holds the lock, and how far apart.
pub const PATIENCE: (u32, Duration) = (6, Duration::from_secs(5));

/// Makes the system port of one call (the real one, or a fake in a test).
pub type SystemFactory = Box<dyn Fn() -> Box<dyn System> + Send + Sync>;

/// A manager for the given waits.
pub type ManagerFactory = Box<dyn Fn(Timings) -> Result<Manager, String> + Send + Sync>;

/// What the agent's unit says about Dyno: starting `dyno-agent run`, low priority, a soft memory limit.
pub fn spec() -> Result<ServiceSpec, String> {
    ServiceSpec::builder(LABEL, "Dyno", UNIT)
        .unit(UNIT)
        .display_name("Dyno agent (measures and keeps your computer's history)")
        .docs_url("https://dev.decryptology.net/decryp7/dyno")
        .args(["run"])
        .throttle_secs(5)
        .low_priority()
        .memory_high_mb(256)
        .build()
        .map_err(|e| format!("the agent's service definition is wrong: {e}"))
}

/// The unit's file under the user's `systemd/user` folder (`XDG_CONFIG_HOME` when absolute, else `~/.config`).
pub fn unit_path(vars: &dyn Vars) -> Result<PathBuf, String> {
    paddock_service::paths::systemd_unit_path(&spec()?, vars).map_err(|e| e.to_string())
}

/// The manager of the agent's unit that starts `exe`, remembering its version in `data_dir`. `system` makes the port of
/// each use (a call is bounded by its own timeout); the service answers when systemd says the unit is active.
pub fn manager(
    exe: &Path,
    vars: &dyn Vars,
    data_dir: &Path,
    version: &str,
    system: &SystemFactory,
    timings: Timings,
) -> Result<Manager, String> {
    let spec = spec()?;
    let backend = Systemd::new(
        spec.clone(),
        paddock_service::paths::systemd_unit_path(&spec, vars).map_err(|e| e.to_string())?,
        Target::exe(exe),
        system(),
    );
    let mut probe = system();
    let unit = spec.unit_file_name();
    Ok(Manager::new("Dyno", Box::new(backend), data_dir, LABEL, version, move || {
        probe
            .run("systemctl", &["--user", "is-active", &unit])
            .is_ok_and(|o| o.stdout.trim() == "active")
    })
    .with_program(&exe.to_string_lossy())
    .with_timings(timings))
}

/// The real system for one call: `systemctl` with a bounded wait.
pub fn real_system() -> SystemFactory {
    Box::new(|| -> Box<dyn System> { Box::new(paddock_service::RealSystem::new().with_timeout(Duration::from_secs(10))) })
}

/// The manager of this process's user: the real environment, Dyno's data folder, the real `systemctl`.
pub fn real_manager(exe: &Path, version: &str, timings: Timings) -> Result<Manager, String> {
    manager(
        exe,
        &paddock_env::ProcessVars,
        crate::paths::Dirs::current().root(),
        version,
        &real_system(),
        timings,
    )
}

/// The waits of a registration the user asked for: it returns once the OS accepted it (the link waits for the agent and
/// falls back to one of its own), so it does not wait for the agent to answer.
pub fn install_timings() -> Timings {
    Timings::default().without_start_wait()
}

/// Set by the app when the user quits: the registration or the wait stops at its next step.
pub static CANCEL: AtomicBool = AtomicBool::new(false);

fn never() -> &'static AtomicBool {
    &CANCEL
}

/// What a failed request says to the user, with the Linux words for a switch that is the user's.
pub fn words(e: &ServiceError) -> String {
    match e {
        ServiceError::SwitchedOff => format!(
            "The agent's unit is switched off ({UNIT}.service is disabled). Dyno never switches it back on for you: run \
             `systemctl --user enable --now {UNIT}.service`, or turn Launch at login off and on in Settings after removing it with \
             `dyno-agent service uninstall`"
        ),
        other => other.to_string(),
    }
}

/// `dyno-agent service install` and Launch at login switched on: registers the unit, enables it and starts it.
pub fn install(m: &mut Manager) -> Result<(), String> {
    m.install_patient(never(), false, PATIENCE.0, PATIENCE.1)
        .map(drop)
        .map_err(|e| words(&e))
}

/// The Settings switch on: the same registration, but another change that holds the lock is answered with Busy at once
/// instead of waited for (the save does not wait; the verbs above keep their patience).
pub fn install_without_waiting(m: &mut Manager) -> Result<(), String> {
    m.install_patient(never(), false, 0, Duration::ZERO).map(drop).map_err(|e| words(&e))
}

/// The Settings switch off, without waiting for another change.
pub fn uninstall_without_waiting(m: &mut Manager) -> Result<(), String> {
    m.uninstall().map_err(|e| words(&e))
}

/// `dyno-agent service uninstall` and Launch at login switched off: stops, disables and removes the unit. Removing what is
/// not there succeeds.
pub fn uninstall(m: &mut Manager) -> Result<(), String> {
    m.uninstall_patient(PATIENCE.0, PATIENCE.1).map_err(|e| words(&e))
}

/// What a start's repair tells the caller: `Ok(true)` the unit was registered again, `Ok(false)` nothing was needed. With
/// `waited` the registration was given the time a start takes, so a service that never answered is an error; without it a
/// registration the OS accepted is all there is to wait for.
pub fn ensure_result(started: Started, waited: bool) -> Result<bool, String> {
    match started {
        Started::Repaired(_, outcome) => match outcome {
            RepairOutcome::Recovered => Ok(true),
            RepairOutcome::Registered if !waited => Ok(true),
            RepairOutcome::Registered => {
                Err("the agent's unit was registered again but did not start; it is tried again at the next start".into())
            }
            RepairOutcome::Cancelled => Ok(false),
            RepairOutcome::Failed(why) | RepairOutcome::Unavailable(why) => Err(why),
            RepairOutcome::NeedsApproval | RepairOutcome::NeedsElevation | RepairOutcome::Declined => {
                Err("the agent's unit could not be registered again".into())
            }
        },
        Started::Busy => Err("another registration or repair kept running; it is tried again at the next start".into()),
        Started::Left(_) | Started::Marked | Started::Offered(_) | Started::Removed => Ok(false),
    }
}

/// `dyno-agent service ensure` and the app's repair at start: registers the unit again when it is stale (another program, a
/// definition from an older version, a program that is gone). Not installed stays not installed, and what the user switched
/// off stays off.
pub fn ensure(m: &mut Manager, waited: bool) -> Result<bool, String> {
    let started = m.on_start_patient(never(), PATIENCE.0, PATIENCE.1);
    if let Started::Repaired(reason, outcome) = &started {
        log::info!(reason:? = reason, outcome:? = outcome; "agent service: replaced the registration");
    }
    ensure_result(started, waited)
}

/// `dyno-agent service status`.
pub fn status_line(m: &mut Manager) -> String {
    let status = m.status();
    if !status.installed {
        return "not installed".into();
    }
    let moved = matches!(m.check(), Plan::Replace(Reason::Moved | Reason::ProgramMissing));
    format!(
        "installed, {}{}",
        if status.running { "running" } else { "not running" },
        if moved { ", registered for another executable" } else { "" }
    )
}

/// The `dyno-agent service` verbs on Linux: thin callers of the manager, with the words and exit codes the packages and docs know.
pub struct Unit(Manager);

impl Unit {
    /// The verbs for the unit of the current user, starting `exe`.
    pub fn for_current_user(exe: &Path) -> Result<Unit, String> {
        real_manager(exe, env!("CARGO_PKG_VERSION"), install_timings()).map(Unit)
    }

    /// The verbs of a manager built elsewhere (tests).
    pub fn new(manager: Manager) -> Unit {
        Unit(manager)
    }
}

impl Verbs for Unit {
    fn install(&mut self) -> Result<(), String> {
        install(&mut self.0)
    }

    fn uninstall(&mut self) -> Result<(), String> {
        uninstall(&mut self.0)
    }

    fn ensure(&mut self) -> Result<bool, String> {
        ensure(&mut self.0, false)
    }

    fn status(&mut self) -> String {
        status_line(&mut self.0)
    }
}

#[cfg(test)]
mod tests;
