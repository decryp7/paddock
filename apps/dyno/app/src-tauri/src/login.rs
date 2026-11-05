//! Starting at login: the one Settings switch `launchAtLogin` means "Dyno opens at login (in the tray) and the agent
//! keeps measuring while the window is closed" (ADR 0010). Two registrations back it:
//!
//! - the app itself: `SMAppService.mainApp` on macOS (one row in Login Items, nothing under `~/Library/LaunchAgents`;
//!   started at login, it hides in the tray because macOS says so, [`paddock_service::apple_event::open_event`]) and
//!   an XDG autostart file with `--hidden` on Linux and the per-user `Run` value on Windows ([`paddock_service::login::UserRunEntry`], [`paddock_service::login::XdgAutostart`]);
//! - the agent as a per-user service: `SMAppService` on macOS (the plist inside the bundle), a systemd user unit on
//!   Linux and the per-user `Run` entry on Windows (`dyno_agent::service`).
//!
//! The OS parts sit behind two small traits so the orchestration is unit-tested with fakes. On macOS the agent service is
//! `paddock_service::Manager` over `SMAppService` (registration, the re-registration after an update, stop and resume are
//! the shared crate's, tested there over fakes); this file only wires it to Dyno's names. Nothing in the tests touches a
//! real launch agent, unit or registry key.

use std::path::PathBuf;

/// Whether the agent answers now, asked from another thread.
pub type AnswerFn = std::sync::Arc<dyn Fn() -> bool + Send + Sync>;

/// The agent as a per-user background service. Registering and removing it are the shared
/// [`paddock_service::login::LoginService`].
pub trait AgentService: paddock_service::login::LoginService + Send {
    /// Whether it is registered.
    fn installed(&mut self) -> bool;
    /// Whether the OS starts the agent now and restarts it when it exits (launchd, systemd), so the app must hand over
    /// to it instead of running its own; a Windows `Run` entry only starts it at the next logon.
    fn supervises(&mut self) -> bool {
        false
    }
    /// Registers it again when the OS cannot start what is registered, and on macOS once after every version change
    /// (an update). `agent_answers` asks whether the agent answers now. Returns whether it registered again.
    fn repair(&mut self, agent_answers: AnswerFn) -> Result<bool, String>;
    /// The service job a stop can name (launchd job, systemd user unit), when the OS supervises the agent now; `None` where
    /// there is none to stop (Windows, or nothing supervises). Quit and stop measuring stops it without removing the registration.
    fn stop_mechanism(&mut self) -> Option<paddock_service::stop::StopMechanism> {
        None
    }
    /// Starts the service again after it was stopped on request (the next start). Returns whether it started something.
    fn resume(&mut self) -> Result<bool, String> {
        Ok(false)
    }
}

pub use paddock_service::login::Autostart;

/// The words of the message [`apply_launch_at_login`] returns.
fn words() -> paddock_service::login::LoginWords {
    paddock_service::login::LoginWords {
        saved: "Your settings are saved".into(),
        service: "the background agent".into(),
        app: "Dyno".into(),
    }
}

/// Applies the `launchAtLogin` setting to both registrations ([`paddock_service::login::apply_launch_at_login_with`]). Each
/// is attempted even if the other fails; the message names what did not work and what to do.
///
/// Turning it on from a place that will not exist after the next login (`location`, [`current_location`]) registers neither
/// and says where to move Dyno; turning it off always runs.
pub fn apply_launch_at_login(
    on: bool,
    location: Result<(), String>,
    service: &mut dyn AgentService,
    autostart: &mut dyn Autostart,
) -> Result<(), String> {
    paddock_service::login::apply_launch_at_login_with(on, location, Some(service), autostart, &words())
}

/// What the first sight of the settings did about start at login.
#[derive(Debug, PartialEq, Eq)]
pub enum FirstRun {
    /// The default was applied now (the switch was on and the marker was missing).
    Applied,
    /// Nothing to do: the marker is there, or the switch is off.
    Skipped,
}

/// The marker's file name inside the user's own app data folder.
pub const FIRST_RUN_MARKER: &str = "login-default-applied";

/// Start at login is on by default, so a fresh install registers itself once without Settings being opened. The first
/// sight of the settings with the switch on and no per-user `marker` registers both entries and writes the marker, so a user who
/// later turns the switch off is never overridden; a switch that is off (a file that says `false`) is respected and only
/// leaves the marker. A failed registration leaves no marker (it is tried again at the next start) and returns the message.
pub fn apply_first_run(
    marker: &paddock_service::login::FirstRunMarker,
    sys: &mut dyn paddock_service::System,
    wanted: bool,
    location: Result<(), String>,
    service: &mut dyn AgentService,
    autostart: &mut dyn Autostart,
) -> Result<FirstRun, String> {
    if marker.is_set(sys) {
        return Ok(FirstRun::Skipped);
    }
    if !wanted {
        marker.record(sys, "off")?;
        return Ok(FirstRun::Skipped);
    }
    apply_launch_at_login(true, location, service, autostart)?;
    marker.record(sys, "applied")?;
    Ok(FirstRun::Applied)
}

/// Whether the first run registers start at login, read from the settings file the app already has before any agent runs: a
/// missing file is a fresh install (the default, on); a file that says `false` is the user's choice (an upgrade from 1.0.0).
pub fn first_run_wanted(settings_file: &std::path::Path) -> bool {
    let (settings, warning) = dyno_agent::files::load(settings_file, dyno_core::settings::parse);
    if let Some(w) = warning {
        // ER-27: a damaged settings file decides this by its defaults; the log says so.
        log::warn!(Warning = w.as_str(); "First run: the settings file was not read cleanly; start at login follows what could be read");
    }
    settings.unwrap_or_default().launch_at_login
}

/// Whether the first-run registration has been tried in this process. A failed attempt is not repeated in the same run
/// (each try may open System Settings or wait on `systemctl`); the missing marker makes the next start try again.
pub struct FirstRunAttempt(std::sync::atomic::AtomicBool);

impl FirstRunAttempt {
    /// Nothing tried yet.
    pub const fn new() -> FirstRunAttempt {
        FirstRunAttempt(std::sync::atomic::AtomicBool::new(false))
    }

    /// True once, for the first caller.
    pub fn claim(&self) -> bool {
        !self.0.swap(true, std::sync::atomic::Ordering::SeqCst)
    }
}

impl Default for FirstRunAttempt {
    fn default() -> Self {
        FirstRunAttempt::new()
    }
}

/// This process's attempt.
pub static FIRST_RUN_ATTEMPT: FirstRunAttempt = FirstRunAttempt::new();

/// [`apply_first_run_at_start`], at most once per `attempt`: later calls do nothing and say [`FirstRun::Skipped`].
pub fn apply_first_run_once(
    attempt: &FirstRunAttempt,
    settings_file: &std::path::Path,
    marker: &paddock_service::login::FirstRunMarker,
    sys: &mut dyn paddock_service::System,
    location: Result<(), String>,
    service: &mut dyn AgentService,
    autostart: &mut dyn Autostart,
) -> Result<FirstRun, String> {
    if !attempt.claim() {
        return Ok(FirstRun::Skipped);
    }
    apply_first_run_at_start(settings_file, marker, sys, location, service, autostart)
}

/// The first run for the installed app, the one place both callers use (the link's start, and the first sight of the settings
/// for an app that registered nothing yet): the real service, login item and marker, once per process. `handover` is true when
/// an agent of the app may already run, so the link is told to look at the service again.
pub fn run_first_run<R: tauri::Runtime>(app: &tauri::AppHandle<R>, link: &crate::link::AgentLink, handover: bool) {
    use tauri::Manager;
    let Ok(data) = app.path().app_data_dir() else {
        crate::logging::info("first run: no data folder, start at login not applied");
        return;
    };
    let mut service = first_run_service();
    let mut autostart = platform_autostart(app);
    let marker = paddock_service::login::FirstRunMarker::new(data.join(FIRST_RUN_MARKER));
    let mut sys = paddock_service::RealSystem::new();
    let began = std::time::Instant::now();
    match apply_first_run_once(
        &FIRST_RUN_ATTEMPT,
        &link.dirs.settings(),
        &marker,
        &mut sys,
        current_location(),
        service.as_mut(),
        autostart.as_mut(),
    ) {
        Ok(FirstRun::Applied) => {
            link.set_service_installed(service.installed());
            if handover {
                link.refresh_service();
            }
            log::info!(Ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX), HandOver = handover; "First run: start at login (the default) was registered");
        }
        Ok(FirstRun::Skipped) => {}
        Err(e) => {
            log::warn!(target: "dyno", Error = e.as_str(), Ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX); "First run: start at login (the default) could not be registered; it is tried again at the next start, or turn it off and on in Settings.")
        }
    }
}

/// The first run, decided before the link starts any agent (bug 7538-1): registering the agent service after the app has
/// started and connected to an agent of its own ends that agent and waits for the service's (launchd's or systemd's
/// restart throttle), so a fresh install showed three agent processes in its first 20 s. Registered first, the link sees a
/// supervising service and starts none of its own.
pub fn apply_first_run_at_start(
    settings_file: &std::path::Path,
    marker: &paddock_service::login::FirstRunMarker,
    sys: &mut dyn paddock_service::System,
    location: Result<(), String>,
    service: &mut dyn AgentService,
    autostart: &mut dyn Autostart,
) -> Result<FirstRun, String> {
    apply_first_run(marker, sys, first_run_wanted(settings_file), location, service, autostart)
}

/// Removes the old route's `Dyno.plist` from `launch_agents` (only a file of exactly that shape) and, because its
/// existence means the switch was on, registers the app's new entry through `autostart` so the switch never says on with
/// nothing registered. An unstable `location` removes the file but registers nothing (the user is told at the next toggle).
pub fn migrate_legacy_login(
    launch_agents: &std::path::Path,
    location: Result<(), String>,
    autostart: &mut dyn Autostart,
) -> Result<crate::login_item::LegacyPlist, String> {
    let result = crate::login_item::remove_legacy_plist(launch_agents).map_err(|e| e.to_string())?;
    if result == crate::login_item::LegacyPlist::Removed && location.is_ok() {
        autostart.set(true)?;
    }
    Ok(result)
}

/// Whether the running executable may be registered to start at login.
pub fn current_location() -> Result<(), String> {
    match std::env::current_exe() {
        Ok(exe) => current_location_for(&exe.to_string_lossy()),
        Err(e) => Err(format!("the app's folder cannot be found: {e}")),
    }
}

/// [`current_location`] for the executable at `exe`.
fn current_location_for(exe: &str) -> Result<(), String> {
    paddock_service::launch::refuse_unstable_location(std::path::Path::new(exe), "Dyno")
}

/// The agent program next to the app (or `DYNO_AGENT_BIN` in a development build).
pub fn agent_program() -> Result<PathBuf, String> {
    let app = std::env::current_exe().map_err(|e| format!("the app's folder cannot be found: {e}"))?;
    let env = std::env::var_os("DYNO_AGENT_BIN");
    crate::link::agent_exe(&app, agent_override(cfg!(debug_assertions), env.as_deref()))
}

/// The program `DYNO_AGENT_BIN` names, only in a development build: in a release build the environment must not decide
/// which program the app runs (any process of the user could set it).
pub fn agent_override(debug_build: bool, value: Option<&std::ffi::OsStr>) -> Option<&std::ffi::OsStr> {
    value.filter(|_| debug_build)
}

// ---------------------------------------------------------------------------------------------------- Linux, Windows

/// The agent's own service registration (`dyno_agent::service`): a systemd user unit or the per-user `Run` entry.
#[cfg(not(target_os = "macos"))]
pub struct CliService;

#[cfg(not(target_os = "macos"))]
impl CliService {
    fn service(&self) -> Result<dyno_agent::service::Service, String> {
        dyno_agent::service::Service::for_current_user(agent_program()?)
    }
}

/// On Windows the app's own `Run` value is the logon entry and the app starts the agent hidden: the agent is not registered
/// separately (an entry that started only the agent opened a console and left no tray icon, Dyno 1.0.0), so `install` only
/// removes the stale agent entry 1.0.0 wrote, and `repair` does the same and says whether it was there.
#[cfg(not(target_os = "macos"))]
impl paddock_service::login::LoginService for CliService {
    fn install(&mut self) -> Result<(), String> {
        let mut s = self.service()?;
        if s.app_is_logon_entry() {
            return s.clear_stale_run_entry().map(|_| ());
        }
        s.install()
    }

    fn uninstall(&mut self) -> Result<(), String> {
        self.service()?.uninstall()
    }
}

#[cfg(not(target_os = "macos"))]
impl AgentService for CliService {
    fn installed(&mut self) -> bool {
        self.service()
            .is_ok_and(|mut s| !s.app_is_logon_entry() && !matches!(s.status(), dyno_agent::service::Status::NotInstalled))
    }

    fn supervises(&mut self) -> bool {
        self.service().is_ok_and(|mut s| s.supervised())
    }

    fn repair(&mut self, _agent_answers: AnswerFn) -> Result<bool, String> {
        let mut s = self.service()?;
        if s.app_is_logon_entry() {
            return s.clear_stale_run_entry();
        }
        s.ensure()
    }

    /// Linux: the systemd user unit, while it is supervising. Windows has no service job (the app's `Run` entry starts the agent).
    fn stop_mechanism(&mut self) -> Option<paddock_service::stop::StopMechanism> {
        if cfg!(target_os = "linux") && self.supervises() {
            Some(paddock_service::stop::StopMechanism::SystemdUser {
                unit: "dyno-agent.service".into(),
            })
        } else {
            None
        }
    }

    /// Linux: `systemctl --user start` of the unit that was stopped (never `enable`: Launch at login was not changed).
    fn resume(&mut self) -> Result<bool, String> {
        if !cfg!(target_os = "linux") || !self.installed() {
            return Ok(false);
        }
        let mut sys = paddock_service::RealSystem::new().with_timeout(std::time::Duration::from_secs(10));
        let out = paddock_service::System::run(&mut sys, "systemctl", &["--user", "start", "dyno-agent.service"])
            .map_err(|e| format!("systemctl start {e}"))?;
        if out.success() {
            Ok(true)
        } else {
            Err(format!("systemctl --user start dyno-agent.service: {}", out.reason()))
        }
    }
}

/// Dyno 1.0.0 on Windows replaced the app's logon entry (`Dyno` = `dyno-app.exe --hidden`) with the agent's (`Dyno` =
/// `dyno-agent.exe run`), five seconds after every start, so the next logon started only the agent. At the first sight of the
/// switch after an update: the stale entry is removed, and the app's own entry is registered again when start at login is on
/// (when it is off, removing it is all). Returns whether a stale entry was there.
pub fn repair_stale_agent_entry(
    start_at_login: bool,
    service: &mut dyn AgentService,
    autostart: &mut dyn Autostart,
) -> Result<bool, String> {
    let cleaned = service.repair(std::sync::Arc::new(|| true))?;
    if cleaned && start_at_login {
        autostart.set(true)?;
    }
    Ok(cleaned)
}

/// Windows, at every start: when start at login is on, the app's own `Run` value is written again (idempotent), so a switch
/// that says on while nothing is registered (an entry written to the wrong hive, deleted by a cleaner, or never written) heals
/// itself. A switch that is off is never touched here. Returns whether it wrote.
pub fn ensure_app_entry(start_at_login: bool, autostart: &mut dyn Autostart) -> Result<bool, String> {
    if !start_at_login {
        return Ok(false);
    }
    autostart.set(true)?;
    Ok(true)
}

// ------------------------------------------------------------------------------- the macOS service's decisions (pure)

/// What a failed registration tells the user. `opened`: System Settings was opened for them.
#[cfg(any(target_os = "macos", test))]
fn registration_words(e: &paddock_service::ServiceError, opened: bool) -> String {
    use paddock_service::ServiceError;
    let shown = if opened { " (opened for you)" } else { "" };
    match e {
        ServiceError::NeedsApproval => {
            format!("macOS needs your approval: allow Dyno in System Settings > General > Login Items{shown}")
        }
        ServiceError::SwitchedOff => {
            format!("Dyno is switched off in System Settings > General > Login Items{shown}: switch it on there")
        }
        other => other.to_string(),
    }
}

/// Whether a registration is made without opening System Settings: the first run (nobody asked), once Login Items has been
/// opened for it before.
#[cfg(any(target_os = "macos", test))]
fn install_is_quiet(first_run: bool, marker_exists: bool) -> bool {
    first_run && marker_exists
}

/// Whether the first run records that it opened Login Items: it did (not quiet) and there was something for the user to do.
#[cfg(any(target_os = "macos", test))]
fn marker_wanted(first_run: bool, quiet: bool, result: &Result<paddock_service::RepairOutcome, paddock_service::ServiceError>) -> bool {
    use paddock_service::ServiceError;
    first_run && !quiet && matches!(result, Err(ServiceError::NeedsApproval | ServiceError::SwitchedOff))
}

/// The waits of the Settings switch's registration: it returns at once (the link waits for the agent and falls back to one of
/// its own), so it does not wait for the service to answer.
#[cfg(any(target_os = "macos", test))]
fn install_timings() -> paddock_service::Timings {
    paddock_service::Timings::default().without_start_wait()
}

/// What the repair at start tells the caller: `Ok(true)` the registration was replaced and the service runs, `Ok(false)` nothing
/// was needed, an error when it was registered but never answered, another repair kept the lock, or it failed.
#[cfg(any(target_os = "macos", test))]
fn repair_result(started: paddock_service::manager::Started) -> Result<bool, String> {
    use paddock_service::manager::Started;
    use paddock_service::{RepairOutcome, ServiceError};
    match started {
        Started::Repaired(_, outcome) => match outcome {
            RepairOutcome::Recovered => Ok(true),
            RepairOutcome::Registered => {
                Err("the agent service was registered again but did not answer; it is tried again at the next start".into())
            }
            RepairOutcome::Cancelled => Ok(false),
            RepairOutcome::NeedsApproval => Err(registration_words(&ServiceError::NeedsApproval, false)),
            RepairOutcome::Failed(why) | RepairOutcome::Unavailable(why) => Err(why),
            RepairOutcome::NeedsElevation | RepairOutcome::Declined => Err("the agent service could not be registered again".into()),
        },
        Started::Busy => Err("another registration or repair kept running; it is tried again at the next start".into()),
        Started::Left(_) | Started::Marked | Started::Offered(_) | Started::Removed => Ok(false),
    }
}

// ---------------------------------------------------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
pub use mac::{migrate_legacy_login_at_start, SmService};

#[cfg(target_os = "macos")]
mod mac {
    //! The bundled launch agent through the shared service manager: `Contents/Library/LaunchAgents/net.decryptology.dyno.agent.plist`
    //! in the bundle (`bundle.macOS.files` in tauri.macos.conf.json) with `BundleProgram` pointing at the bundled agent, registered
    //! with `SMAppService` by `paddock_service::backend::LaunchdBundled`. Registration, the re-registration after an update,
    //! stop and resume are `paddock_service::Manager`'s; this file names Dyno's service and maps the results to Dyno's messages.

    use super::*;
    use dyno_agent::service::LABEL;
    use paddock_service::login::{LoginService, MainAppItem};
    use paddock_service::manager::Started;
    use paddock_service::repair::Timings;
    use paddock_service::{Manager, ServiceSpec};
    use std::sync::atomic::AtomicBool;

    /// The bundled launch agent. `first_run` is the unasked registration at the first sight of the settings: it opens System
    /// Settings (Login Items) for an approval or a switch at most once, where the Settings switch always does.
    pub struct SmService {
        first_run: bool,
    }

    impl SmService {
        /// The service for what the user asks (Settings switch, repair at start, resume, stop).
        pub const fn asked() -> SmService {
            SmService { first_run: false }
        }

        /// The service for the first run (no one asked): Login Items is opened at most once.
        pub const fn first_run() -> SmService {
            SmService { first_run: true }
        }
    }

    /// The login items have no cancel button; a repair runs to its bounds.
    fn never() -> AtomicBool {
        AtomicBool::new(false)
    }

    /// The marker that Login Items has been opened for the first run once.
    fn opened_marker() -> PathBuf {
        dyno_agent::paths::Dirs::current().root().join("login-items-opened")
    }

    /// Dyno 1.1.0 records the registered version with the shared service record; the 1.0.0 file is of no use any more.
    fn forget_old_version_file(root: &std::path::Path) {
        let old = root.join("service-registered-version");
        match std::fs::remove_file(&old) {
            Ok(()) => crate::logging::info("removed the 1.0.0 registered-version file (the service record replaces it)"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => crate::logging::warn(&format!("the 1.0.0 registered-version file could not be removed: {e}")),
        }
    }

    /// The service of the agent: its spec, the real `SMAppService` and `launchctl` (5 s per call), Dyno's data folder.
    /// `agent_answers` says whether the agent answers; `timings` are the waits of a repair.
    fn manager(agent_answers: AnswerFn, timings: Timings) -> Result<Manager, String> {
        let spec = ServiceSpec::builder(LABEL, "Dyno", "dyno-agent")
            .build()
            .map_err(|e| format!("the agent's service definition is wrong: {e}"))?;
        let uid = paddock_service::native::current_uid();
        let target = paddock_service::paths::launchd_target(&spec, uid);
        let system = || paddock_service::system::RealSystem::new().with_timeout(std::time::Duration::from_secs(5));
        let backend = paddock_service::backend::LaunchdBundled::new(
            spec.clone(),
            uid,
            Box::new(paddock_service::native::SmAppService::new(&spec)),
            Box::new(system()),
        );
        let dirs = dyno_agent::paths::Dirs::current();
        Ok(Manager::new(
            "Dyno",
            Box::new(backend),
            dirs.root(),
            LABEL,
            env!("CARGO_PKG_VERSION"),
            // The service's own signal only: launchd running the job. The app's own agent, which the link may have started
            // meanwhile, is `cover`: it keeps a refused job from being replaced, but never settles a registration.
            move || paddock_service::launchctl::job_running(&mut system(), &target),
        )
        .with_cover(move || agent_answers())
        .with_timings(timings))
    }

    /// A manager that is only asked what it sees or told to stop or resume: nothing waits for an agent.
    fn quiet_manager() -> Result<Manager, String> {
        manager(std::sync::Arc::new(|| false), Timings::default())
    }

    /// Moves a start-at-login the old route (`tauri-plugin-autostart`) wrote to `SMAppService.mainApp` ([`super::migrate_legacy_login`]).
    pub fn migrate_legacy_login_at_start() {
        let Some(home) = std::env::var_os("HOME") else { return };
        let dir = PathBuf::from(home).join("Library/LaunchAgents");
        match super::migrate_legacy_login(&dir, super::current_location(), &mut MainAppItem::new()) {
            Ok(crate::login_item::LegacyPlist::Removed) => {
                crate::logging::info("moved start at login from the old Dyno.plist to a Login Items entry")
            }
            Ok(_) => {}
            Err(e) => crate::logging::warn(&format!("the old start-at-login file could not be moved: {e}")),
        }
    }

    impl LoginService for SmService {
        /// Registers the agent without waiting for it to answer (`install_timings`): the link waits for it and falls back to an
        /// agent of its own. This call itself can still take long (a lost lock is waited for up to 30 s, and each registration
        /// cycle about 24 s), so the Settings switch must show it as busy.
        fn install(&mut self) -> Result<(), String> {
            let mut m = manager(std::sync::Arc::new(|| false), install_timings())?;
            let quiet = install_is_quiet(self.first_run, opened_marker().exists());
            // A first run that lost the lock to another registration waits for it instead of leaving nothing registered.
            let result = m.install_patient(&never(), quiet, 6, std::time::Duration::from_secs(5));
            if marker_wanted(self.first_run, quiet, &result) {
                let marker = opened_marker();
                let written = marker
                    .parent()
                    .map_or(Ok(()), std::fs::create_dir_all)
                    .and_then(|()| std::fs::write(&marker, "opened\n"));
                if let Err(e) = written {
                    crate::logging::warn(&format!("the Login Items marker could not be written: {e}"));
                }
            }
            result.map(drop).map_err(|e| registration_words(&e, !quiet))
        }

        fn uninstall(&mut self) -> Result<(), String> {
            // Another registration or repair may hold the lock: wait for it as the install does.
            quiet_manager()?
                .uninstall_patient(6, std::time::Duration::from_secs(5))
                .map_err(|e| e.to_string())
        }
    }

    impl AgentService for SmService {
        fn installed(&mut self) -> bool {
            quiet_manager().is_ok_and(|mut m| m.installed())
        }

        /// Enabled, not switched off in Login Items and loaded in launchd: it runs the job (`RunAtLoad`, `KeepAlive`).
        /// Waiting for approval, switched off or dropped by launchd, it runs nothing.
        fn supervises(&mut self) -> bool {
            quiet_manager().is_ok_and(|mut m| m.supervises())
        }

        /// After an update launchd refuses the registered agent (its stored launch constraint is the previous build's
        /// cdhash) or keeps the old job: replace the registration when launchd reports it stale while the agent does
        /// not answer, and once after every version change. What the user switched off in Login Items is left alone, and
        /// nothing is registered that the user never turned on.
        fn repair(&mut self, agent_answers: AnswerFn) -> Result<bool, String> {
            // From a disk image or a translocated path the bundle cannot register the agent: replacing would remove a job
            // that runs and fail to put it back.
            super::current_location()?;
            forget_old_version_file(dyno_agent::paths::Dirs::current().root());
            let mut m = manager(agent_answers, Timings::default())?;
            // Another registration or repair (the first run, a second copy) may hold the lock: wait for it a while.
            let started = m.on_start_patient(&never(), 6, std::time::Duration::from_secs(5));
            if let Started::Repaired(reason, outcome) = &started {
                crate::logging::info(&format!("agent service: replaced the registration ({reason:?}): {outcome:?}"));
            }
            repair_result(started)
        }

        /// The launchd job `gui/<uid>/net.decryptology.dyno.agent`, while launchd holds it. The `SMAppService` registration is
        /// not touched: `launchctl bootout` takes the job out of launchd (so `KeepAlive` cannot restart it) and the registration
        /// loads it again at the next login.
        fn stop_mechanism(&mut self) -> Option<paddock_service::stop::StopMechanism> {
            quiet_manager().ok()?.stop_mechanism()
        }

        /// After a stop on request launchd holds no job and `RunAtLoad` fires only when a job is loaded: the shared resume decides and
        /// runs it (an item switched off or a list that cannot be read is left alone; a version change goes through the full repair;
        /// an enabled item of this version is registered again without removal, which macOS would announce as a new item).
        /// Returns whether launchd holds the job afterwards (found, not necessarily running yet).
        fn resume(&mut self) -> Result<bool, String> {
            quiet_manager()?
                .resume(&never(), &mut |step| crate::logging::info(&format!("agent service: {step}")))
                .map_err(|e| registration_words(&e, false))
        }
    }
}

/// The platform's agent service.
#[cfg(target_os = "macos")]
pub fn platform_service() -> Box<dyn AgentService> {
    Box::new(SmService::asked())
}

/// The agent service for the first run, which nobody asked for: System Settings is opened at most once.
#[cfg(target_os = "macos")]
pub fn first_run_service() -> Box<dyn AgentService> {
    Box::new(SmService::first_run())
}

/// The agent service for the first run (the same as [`platform_service`] off macOS).
#[cfg(not(target_os = "macos"))]
pub fn first_run_service() -> Box<dyn AgentService> {
    platform_service()
}

/// The platform's agent service.
#[cfg(not(target_os = "macos"))]
pub fn platform_service() -> Box<dyn AgentService> {
    Box::new(CliService)
}

/// The probe the agent link asks before it starts an agent of its own (see [`crate::link::ServiceState`]).
pub fn service_probe() -> crate::link::ServiceProbe {
    std::sync::Arc::new(|| {
        let mut service = platform_service();
        crate::link::ServiceState {
            installed: service.installed(),
            supervises: service.supervises(),
        }
    })
}

/// The app's own login item: `SMAppService.mainApp` on macOS, `tauri-plugin-autostart` elsewhere.
#[cfg(target_os = "macos")]
pub fn platform_autostart<R: tauri::Runtime>(_app: &tauri::AppHandle<R>) -> Box<dyn Autostart> {
    Box::new(paddock_service::login::MainAppItem::new())
}

/// The app's own login item on Windows: always the per-user `Run` value, never written while elevated
/// ([`paddock_service::login::UserRunEntry`]).
#[cfg(windows)]
pub fn platform_autostart<R: tauri::Runtime>(_app: &tauri::AppHandle<R>) -> Box<dyn Autostart> {
    match std::env::current_exe() {
        Ok(exe) => Box::new(paddock_service::login::UserRunEntry::new(
            dyno_agent::service::spec::WINDOWS_APP_VALUE,
            exe,
            &[paddock_window::launch::HIDDEN_FLAG],
            Box::new(paddock_service::RealSystem::new()),
            Box::new(paddock_service::login::ProcessElevation),
        )),
        Err(e) => Box::new(NoAutostart(format!("the app's folder cannot be found: {e}"))),
    }
}

/// The app's own login item on Linux: an XDG autostart file that starts the app hidden in the tray
/// ([`paddock_service::login::XdgAutostart`]).
#[cfg(not(any(target_os = "macos", windows)))]
pub fn platform_autostart<R: tauri::Runtime>(_app: &tauri::AppHandle<R>) -> Box<dyn Autostart> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    match (config, std::env::current_exe()) {
        (Some(config), Ok(exe)) => Box::new(paddock_service::login::XdgAutostart::new(
            &config,
            // The file name the plugin used (the app name), so an upgrade rewrites it instead of adding a second entry.
            "Dyno",
            "Dyno",
            exe,
            &[paddock_window::launch::HIDDEN_FLAG],
            Box::new(paddock_service::RealSystem::new()),
        )),
        _ => Box::new(NoAutostart("the home folder or the app's folder cannot be found".into())),
    }
}

/// An entry that cannot be written, with the reason.
#[cfg(not(target_os = "macos"))]
struct NoAutostart(String);

#[cfg(not(target_os = "macos"))]
impl Autostart for NoAutostart {
    fn set(&mut self, _: bool) -> Result<(), String> {
        Err(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_first_run_is_quiet_only_once_login_items_has_been_opened_for_it() {
        assert!(!install_is_quiet(false, true), "the Settings switch always opens it");
        assert!(!install_is_quiet(true, false));
        assert!(install_is_quiet(true, true));
    }

    #[test]
    fn the_marker_is_written_only_by_a_first_run_that_opened_login_items_for_the_user() {
        use paddock_service::{RepairOutcome, ServiceError};
        let needs = Err(ServiceError::SwitchedOff);
        assert!(marker_wanted(true, false, &needs));
        assert!(marker_wanted(true, false, &Err(ServiceError::NeedsApproval)));
        assert!(!marker_wanted(false, false, &needs), "the Settings switch does not mark");
        assert!(!marker_wanted(true, true, &needs), "a quiet install opened nothing");
        assert!(!marker_wanted(true, false, &Err(ServiceError::Busy)));
        assert!(!marker_wanted(true, false, &Ok(RepairOutcome::Recovered)));
    }

    #[test]
    fn the_settings_switch_registers_without_waiting_for_the_service_to_answer() {
        assert_eq!(install_timings().start_wait, std::time::Duration::ZERO);
        assert!(
            install_timings().gone_wait > std::time::Duration::ZERO,
            "the removal is still waited for"
        );
    }

    #[test]
    fn the_repair_at_start_reports_what_happened_to_the_caller() {
        use paddock_service::manager::Started;
        use paddock_service::{Hold, Reason, RepairOutcome};
        let repaired = |o| repair_result(Started::Repaired(Reason::VersionChanged, o));
        assert_eq!(repaired(RepairOutcome::Recovered), Ok(true));
        assert!(repaired(RepairOutcome::Registered).unwrap_err().contains("did not answer"));
        assert!(repaired(RepairOutcome::NeedsApproval).unwrap_err().contains("Login Items"));
        assert_eq!(repaired(RepairOutcome::Failed("why".into())), Err("why".into()));
        assert_eq!(repaired(RepairOutcome::Cancelled), Ok(false));
        assert!(repair_result(Started::Busy).unwrap_err().contains("next start"));
        assert_eq!(repair_result(Started::Left(Hold::Healthy)), Ok(false));
        assert_eq!(repair_result(Started::Marked), Ok(false));
    }

    #[test]
    fn a_failed_registration_names_the_login_items_page_and_says_whether_it_was_opened() {
        use paddock_service::ServiceError;
        assert!(registration_words(&ServiceError::NeedsApproval, true).ends_with("(opened for you)"));
        assert!(!registration_words(&ServiceError::SwitchedOff, false).contains("opened for you"));
    }

    #[test]
    fn the_agent_override_from_the_environment_counts_only_in_a_development_build() {
        let v = std::ffi::OsString::from("/tmp/other-agent");
        assert_eq!(super::agent_override(true, Some(v.as_os_str())), Some(v.as_os_str()));
        assert_eq!(
            super::agent_override(false, Some(v.as_os_str())),
            None,
            "a release build ignores it"
        );
        assert_eq!(super::agent_override(true, None), None);
    }

    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Calls(Arc<Mutex<Vec<String>>>);

    struct FakeService {
        calls: Arc<Mutex<Vec<String>>>,
        fail: bool,
    }

    impl paddock_service::login::LoginService for FakeService {
        fn install(&mut self) -> Result<(), String> {
            self.calls.lock().unwrap().push("service install".into());
            if self.fail {
                Err("no".into())
            } else {
                Ok(())
            }
        }
        fn uninstall(&mut self) -> Result<(), String> {
            self.calls.lock().unwrap().push("service uninstall".into());
            Ok(())
        }
    }

    impl AgentService for FakeService {
        fn installed(&mut self) -> bool {
            false
        }
        fn repair(&mut self, _: AnswerFn) -> Result<bool, String> {
            Ok(false)
        }
    }

    struct FakeAutostart {
        calls: Arc<Mutex<Vec<String>>>,
        fail: bool,
    }

    impl Autostart for FakeAutostart {
        fn set(&mut self, on: bool) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("autostart {on}"));
            if self.fail {
                Err("denied".into())
            } else {
                Ok(())
            }
        }
    }

    fn apply(on: bool, service_fails: bool, autostart_fails: bool) -> (Result<(), String>, Vec<String>) {
        let calls = Calls::default();
        let mut s = FakeService {
            calls: calls.0.clone(),
            fail: service_fails,
        };
        let mut a = FakeAutostart {
            calls: calls.0.clone(),
            fail: autostart_fails,
        };
        let r = apply_launch_at_login(on, Ok(()), &mut s, &mut a);
        let log = calls.0.lock().unwrap().clone();
        (r, log)
    }

    fn first_run(
        marker: &paddock_service::login::FirstRunMarker,
        sys: &paddock_service::testing::FakeSystem,
        wanted: bool,
        autostart_fails: bool,
    ) -> (Result<FirstRun, String>, Vec<String>) {
        let calls = Calls::default();
        let mut s = FakeService {
            calls: calls.0.clone(),
            fail: false,
        };
        let mut a = FakeAutostart {
            calls: calls.0.clone(),
            fail: autostart_fails,
        };
        let r = apply_first_run(marker, &mut sys.clone(), wanted, Ok(()), &mut s, &mut a);
        let log = calls.0.lock().unwrap().clone();
        (r, log)
    }

    fn marker() -> paddock_service::login::FirstRunMarker {
        paddock_service::login::FirstRunMarker::new(format!("/data/{FIRST_RUN_MARKER}"))
    }

    #[test]
    fn a_first_run_with_the_default_registers_once_and_never_again() {
        let sys = paddock_service::testing::FakeSystem::new();
        let m = marker();
        let (r, log) = first_run(&m, &sys, true, false);
        assert_eq!(r, Ok(FirstRun::Applied));
        assert_eq!(log, vec!["service install".to_string(), "autostart true".to_string()]);
        // The user turns it off; the next start sees the marker and leaves it off.
        let (r, log) = first_run(&m, &sys, true, false);
        assert_eq!(r, Ok(FirstRun::Skipped));
        assert!(log.is_empty());
    }

    fn at_start(settings_text: Option<&str>) -> (Result<FirstRun, String>, Vec<String>, bool) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        if let Some(t) = settings_text {
            std::fs::write(&file, t).unwrap();
        }
        let sys = paddock_service::testing::FakeSystem::new();
        let m = marker();
        let calls = Calls::default();
        let mut s = FakeService {
            calls: calls.0.clone(),
            fail: false,
        };
        let mut a = FakeAutostart {
            calls: calls.0.clone(),
            fail: false,
        };
        let r = apply_first_run_at_start(&file, &m, &mut sys.clone(), Ok(()), &mut s, &mut a);
        let log = calls.0.lock().unwrap().clone();
        (r, log, m.is_set(&mut sys.clone()))
    }

    /// Bug 7538-1: the registration runs before the link starts an agent, from the settings file (no agent to ask yet).
    #[test]
    fn the_first_run_before_any_agent_registers_on_a_fresh_install() {
        let (r, log, set) = at_start(None);
        assert_eq!(r, Ok(FirstRun::Applied));
        assert_eq!(log, vec!["service install".to_string(), "autostart true".to_string()]);
        assert!(set);
    }

    #[test]
    fn the_first_run_before_any_agent_keeps_an_explicit_off_from_an_upgrade() {
        let (r, log, set) = at_start(Some(r#"{"launchAtLogin":false}"#));
        assert_eq!(r, Ok(FirstRun::Skipped));
        assert!(log.is_empty(), "nothing registered: {log:?}");
        assert!(set, "recorded, so the later first sight of the settings is not a first run");
    }

    /// A failed start-time attempt is not repeated by the first sight of the settings in the same run (bug 7538-1, review P2-2):
    /// each repeat opens Login Items on macOS, and a later success would hand the running agent over again.
    #[test]
    fn a_failed_first_run_is_not_tried_again_in_the_same_run() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let sys = paddock_service::testing::FakeSystem::new();
        let m = marker();
        let attempt = FirstRunAttempt::new();
        let calls = Calls::default();
        let try_once = |autostart_fails: bool| {
            let mut s = FakeService {
                calls: calls.0.clone(),
                fail: false,
            };
            let mut a = FakeAutostart {
                calls: calls.0.clone(),
                fail: autostart_fails,
            };
            apply_first_run_once(&attempt, &file, &m, &mut sys.clone(), Ok(()), &mut s, &mut a)
        };
        assert!(try_once(true).is_err(), "the start-time attempt fails");
        let before = calls.0.lock().unwrap().len();
        assert_eq!(try_once(false), Ok(FirstRun::Skipped), "the second caller of this run does nothing");
        assert_eq!(calls.0.lock().unwrap().len(), before, "no second registration in the same run");
        assert!(!m.is_set(&mut sys.clone()), "no marker, so the next start tries again");
        assert!(FirstRunAttempt::new().claim(), "a new process (attempt) tries");
    }

    #[test]
    fn an_explicit_off_is_not_overridden() {
        let sys = paddock_service::testing::FakeSystem::new();
        let m = marker();
        let (r, log) = first_run(&m, &sys, false, false);
        assert_eq!(r, Ok(FirstRun::Skipped));
        assert!(log.is_empty(), "nothing registered: {log:?}");
        assert!(
            m.is_set(&mut sys.clone()),
            "recorded, so a later change is not taken for a first run"
        );
    }

    #[test]
    fn a_failed_first_run_is_tried_again_at_the_next_start() {
        let sys = paddock_service::testing::FakeSystem::new();
        let m = marker();
        assert!(first_run(&m, &sys, true, true).0.is_err());
        assert!(!m.is_set(&mut sys.clone()));
    }

    #[test]
    fn the_switch_registers_or_removes_both() {
        assert_eq!(
            apply(true, false, false),
            (Ok(()), vec!["service install".into(), "autostart true".into()])
        );
        assert_eq!(
            apply(false, false, false),
            (Ok(()), vec!["service uninstall".into(), "autostart false".into()])
        );
    }

    /// The logic (both attempted, one message, the location refusal) is `paddock_service::login`'s and tested there; this
    /// pins Dyno's words and that a refused location registers nothing through Dyno's wiring.
    #[test]
    fn the_wiring_speaks_dynos_words_and_a_refused_location_registers_nothing() {
        let (r, log) = apply(true, true, true);
        assert_eq!(log.len(), 2, "autostart is still attempted");
        let m = r.unwrap_err();
        assert!(
            m.starts_with("Your settings are saved, but the background agent could not be registered: no; and Dyno could not be added to the login items: denied"),
            "{m}"
        );
        let calls = Calls::default();
        let mut s = FakeService {
            calls: calls.0.clone(),
            fail: false,
        };
        let mut a = FakeAutostart {
            calls: calls.0.clone(),
            fail: false,
        };
        let m = apply_launch_at_login(true, Err("move Dyno to Applications".into()), &mut s, &mut a).unwrap_err();
        assert!(
            m.contains("move Dyno to Applications") && m.starts_with("Your settings are saved"),
            "{m}"
        );
        assert!(calls.0.lock().unwrap().is_empty(), "nothing was registered");
        assert_eq!(apply_launch_at_login(false, Err("x".into()), &mut s, &mut a), Ok(()));
        assert_eq!(calls.0.lock().unwrap().len(), 2, "turning off always runs");
    }

    #[test]
    fn the_location_check_names_dyno() {
        let m = current_location_for("/Volumes/Dyno/Dyno.app/Contents/MacOS/dyno-app").unwrap_err();
        assert!(m.starts_with("Dyno is running from a temporary location"), "{m}");
        assert!(current_location_for("/Applications/Dyno.app/Contents/MacOS/dyno-app").is_ok());
    }

    #[test]
    fn a_removed_legacy_file_registers_the_new_entry_once_and_nothing_else_does() {
        let dir = tempfile::tempdir().unwrap();
        let old = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n<key>Label</key>\n<string>Dyno</string>\n\
            <key>AssociatedBundleIdentifiers</key>\n<array></array>\n<key>ProgramArguments</key>\n<array><string>/Applications/Dyno.app/Contents/MacOS/dyno-app</string>\
            <string>--hidden</string></array>\n<key>RunAtLoad</key>\n<true/>\n</dict>\n</plist>";
        let calls = Calls::default();
        let mut a = FakeAutostart {
            calls: calls.0.clone(),
            fail: false,
        };
        // Nothing there: nothing registered.
        assert_eq!(
            migrate_legacy_login(dir.path(), Ok(()), &mut a),
            Ok(crate::login_item::LegacyPlist::Absent)
        );
        // A foreign file is kept and nothing is registered.
        std::fs::write(dir.path().join("Dyno.plist"), "mine").unwrap();
        assert_eq!(
            migrate_legacy_login(dir.path(), Ok(()), &mut a),
            Ok(crate::login_item::LegacyPlist::Foreign)
        );
        assert!(calls.0.lock().unwrap().is_empty());
        // The old file: removed, new entry registered.
        std::fs::write(dir.path().join("Dyno.plist"), old).unwrap();
        assert_eq!(
            migrate_legacy_login(dir.path(), Ok(()), &mut a),
            Ok(crate::login_item::LegacyPlist::Removed)
        );
        assert_eq!(*calls.0.lock().unwrap(), vec!["autostart true".to_string()]);
        // From an unstable place the file goes but nothing is registered.
        std::fs::write(dir.path().join("Dyno.plist"), old).unwrap();
        assert_eq!(
            migrate_legacy_login(dir.path(), Err("x".into()), &mut a),
            Ok(crate::login_item::LegacyPlist::Removed)
        );
        assert_eq!(calls.0.lock().unwrap().len(), 1);
    }

    struct StaleService {
        stale: bool,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl paddock_service::login::LoginService for StaleService {
        fn install(&mut self) -> Result<(), String> {
            self.calls.lock().unwrap().push("service install".into());
            Ok(())
        }
        fn uninstall(&mut self) -> Result<(), String> {
            Ok(())
        }
    }

    impl AgentService for StaleService {
        fn installed(&mut self) -> bool {
            false
        }
        fn repair(&mut self, _: AnswerFn) -> Result<bool, String> {
            self.calls.lock().unwrap().push("repair".into());
            Ok(std::mem::take(&mut self.stale))
        }
    }

    fn upgrade(stale: bool, start_at_login: bool) -> (Result<bool, String>, Vec<String>) {
        let calls = Calls::default();
        let mut s = StaleService {
            stale,
            calls: calls.0.clone(),
        };
        let mut a = FakeAutostart {
            calls: calls.0.clone(),
            fail: false,
        };
        let r = repair_stale_agent_entry(start_at_login, &mut s, &mut a);
        let log = calls.0.lock().unwrap().clone();
        (r, log)
    }

    #[test]
    fn upgrading_from_1_0_0_with_start_at_login_on_puts_the_apps_entry_back() {
        assert_eq!(
            upgrade(true, true),
            (Ok(true), vec!["repair".to_string(), "autostart true".to_string()])
        );
    }

    #[test]
    fn upgrading_from_1_0_0_with_start_at_login_off_only_removes_the_stale_entry() {
        assert_eq!(upgrade(true, false), (Ok(true), vec!["repair".to_string()]));
    }

    #[test]
    fn without_a_stale_entry_the_apps_entry_is_left_alone() {
        assert_eq!(upgrade(false, true), (Ok(false), vec!["repair".to_string()]));
        assert_eq!(upgrade(false, false), (Ok(false), vec!["repair".to_string()]));
    }
}
