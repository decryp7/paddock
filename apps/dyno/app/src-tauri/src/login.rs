//! Starting at login: the one Settings switch `launchAtLogin` means "Dyno opens at login (in the tray) and the agent
//! keeps measuring while the window is closed" (ADR 0010). Two registrations back it:
//!
//! - the app itself: `SMAppService.mainApp` on macOS (one row in Login Items, nothing under `~/Library/LaunchAgents`;
//!   started at login, it hides in the tray because macOS says so, [`mac::open_event`]) and
//!   an XDG autostart file with `--hidden` on Linux and the per-user `Run` value on Windows ([`paddock_service::login::UserRunEntry`], [`crate::login_item::route_for`]);
//! - the agent as a per-user service: `SMAppService` on macOS (the plist inside the bundle), a systemd user unit on
//!   Linux and the per-user `Run` entry on Windows (`dyno_agent::service`).
//!
//! The OS parts sit behind two small traits so the orchestration is unit-tested with fakes; the macOS re-registration
//! after an update is decided and run by [`crate::service_repair`] (also over fakes). Nothing in the tests touches a
//! real launch agent, unit or registry key.

use std::path::PathBuf;

/// The agent as a per-user background service.
pub trait AgentService: Send {
    /// Whether it is registered.
    fn installed(&mut self) -> bool;
    /// Registers (and starts) it.
    fn install(&mut self) -> Result<(), String>;
    /// Stops and removes it.
    fn uninstall(&mut self) -> Result<(), String>;
    /// Whether the OS starts the agent now and restarts it when it exits (launchd, systemd), so the app must hand over
    /// to it instead of running its own; a Windows `Run` entry only starts it at the next logon.
    fn supervises(&mut self) -> bool {
        false
    }
    /// Registers it again when the OS cannot start what is registered, and on macOS once after every version change
    /// (an update). `agent_answers` asks whether the agent answers now. Returns whether it registered again.
    fn repair(&mut self, agent_answers: &dyn Fn() -> bool) -> Result<bool, String>;
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

/// Applies the `launchAtLogin` setting to both registrations. Each is attempted even if the other fails; the message
/// names what did not work and what to do.
///
/// Turning it on from a place that will not exist after the next login (`location`, [`crate::login_item::refuse_unstable_location`])
/// registers neither and says where to move Dyno; turning it off always runs.
pub fn apply_launch_at_login(
    on: bool,
    location: Result<(), String>,
    service: &mut dyn AgentService,
    autostart: &mut dyn Autostart,
) -> Result<(), String> {
    if let (true, Err(e)) = (on, location) {
        return Err(format!("Your settings are saved, but start at login was not turned on: {e}"));
    }
    let mut problems = Vec::new();
    let service_result = if on { service.install() } else { service.uninstall() };
    if let Err(e) = service_result {
        problems.push(format!(
            "the background agent could not be {}: {e}",
            if on { "registered" } else { "removed" }
        ));
    }
    if let Err(e) = autostart.set(on) {
        problems.push(format!(
            "Dyno could not be {} the login items: {e}",
            if on { "added to" } else { "removed from" }
        ));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!("Your settings are saved, but {}.", problems.join("; and ")))
    }
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
    let mut service = platform_service();
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
        Ok(exe) => crate::login_item::refuse_unstable_location(&exe),
        Err(e) => Err(format!("the app's folder cannot be found: {e}")),
    }
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
impl AgentService for CliService {
    fn installed(&mut self) -> bool {
        self.service()
            .is_ok_and(|mut s| !s.app_is_logon_entry() && !matches!(s.status(), dyno_agent::service::Status::NotInstalled))
    }

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

    fn supervises(&mut self) -> bool {
        self.service().is_ok_and(|mut s| s.supervised())
    }

    fn repair(&mut self, _agent_answers: &dyn Fn() -> bool) -> Result<bool, String> {
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
    let cleaned = service.repair(&|| true)?;
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

// ---------------------------------------------------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
pub use mac::{migrate_legacy_login_at_start, open_event, MainAppLogin, SmService};

#[cfg(target_os = "macos")]
mod mac {
    //! `SMAppService` (macOS 13+). The plist is `Contents/Library/LaunchAgents/net.decryptology.dyno.agent.plist` in the
    //! bundle (`bundle.macOS.files` in tauri.macos.conf.json) with `BundleProgram` pointing at the bundled agent. The
    //! Objective-C calls are the only unsafe code; each is a plain message send on objects we own.
    #![allow(unsafe_code, unused_unsafe)]

    use super::*;
    use crate::login_item::OpenEvent;
    use crate::service_repair::{
        keep_registered, launchctl_output, parse_print_disabled, plan, record_registered_version, registered_version, service_supervises,
        JobReport, Observed, RegisterError, RegistrationStatus, RepairOps, REGISTERED_VERSION_FILE,
    };
    use dyno_agent::service::LABEL;
    use objc2_foundation::NSString;
    use objc2_service_management::{SMAppService, SMAppServiceStatus};
    use paddock_service::system::RealSystem;
    use std::time::Duration;

    const PLIST: &str = "net.decryptology.dyno.agent.plist";

    /// The bundled launch agent.
    pub struct SmService;

    /// The app itself as a login item: `SMAppService.mainApp` (one row in Login Items, macOS 13+).
    pub struct MainAppLogin;

    impl Autostart for MainAppLogin {
        fn set(&mut self, on: bool) -> Result<(), String> {
            paddock_service::login::AppServiceAutostart::new(Box::new(paddock_service::native::SmAppService::main_app())).set(on)
        }
    }

    /// Moves a start-at-login the old route (`tauri-plugin-autostart`) wrote to `SMAppService.mainApp` ([`super::migrate_legacy_login`]).
    pub fn migrate_legacy_login_at_start() {
        let Some(home) = std::env::var_os("HOME") else { return };
        let dir = PathBuf::from(home).join("Library/LaunchAgents");
        match super::migrate_legacy_login(&dir, super::current_location(), &mut MainAppLogin) {
            Ok(crate::login_item::LegacyPlist::Removed) => {
                crate::logging::info("moved start at login from the old Dyno.plist to a Login Items entry")
            }
            Ok(_) => {}
            Err(e) => crate::logging::info(&format!("the old start-at-login file could not be moved: {e}")),
        }
    }

    /// What the open-application Apple Event says about this start: `keyAELaunchedAsLogInItem` (`'lgit'`) - Apple's
    /// `AERegistry.h` (Xcode SDK, read 2026-10-31): "application was launched as a login item and probably shouldn't open
    /// up untitled documents" - as a parameter of the event or the value of its `keyAEPropData` (`'prdt'`); and
    /// `keyAERestoreAppState` (`'rsto'`) with the value yes (`AppleEvents.h`), which loginwindow sends when it reopens
    /// the apps that were running at logout ("Reopen windows"), together with whether the sender (`keySenderPIDAttr`,
    /// `'spid'`) is the login window, judged by the sender pid's executable path read right here (rule 53: not a pid
    /// remembered from earlier). Only valid while the event is being handled: call it from `applicationDidFinishLaunching`,
    /// which is where Tauri runs its setup (`RuntimeRunEvent::Ready`). Any miss gives the default (all false): the window shows.
    pub fn open_event() -> OpenEvent {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        const OAPP: u32 = u32::from_be_bytes(*b"oapp");
        const PROP_DATA: u32 = u32::from_be_bytes(*b"prdt");
        const LOGIN_ITEM: u32 = u32::from_be_bytes(*b"lgit");
        const RESTORE: u32 = u32::from_be_bytes(*b"rsto");
        const YES: u32 = u32::from_be_bytes(*b"yes ");
        const SENDER_PID: u32 = u32::from_be_bytes(*b"spid");
        // SAFETY: plain message sends on Foundation objects; the types of the replies match the documented
        // NSAppleEventManager / NSAppleEventDescriptor signatures (FourCharCode is u32, a pid is int32).
        unsafe {
            let manager: Option<objc2::rc::Retained<AnyObject>> = msg_send![class!(NSAppleEventManager), sharedAppleEventManager];
            let Some(manager) = manager else { return OpenEvent::default() };
            let event: Option<objc2::rc::Retained<AnyObject>> = msg_send![&*manager, currentAppleEvent];
            let Some(event) = event else { return OpenEvent::default() };
            let id: u32 = msg_send![&*event, eventID];
            if id != OAPP {
                return OpenEvent::default();
            }
            let direct: Option<objc2::rc::Retained<AnyObject>> = msg_send![&*event, paramDescriptorForKeyword: LOGIN_ITEM];
            let props: Option<objc2::rc::Retained<AnyObject>> = msg_send![&*event, paramDescriptorForKeyword: PROP_DATA];
            let by_props = match props {
                Some(p) => {
                    let code: u32 = msg_send![&*p, enumCodeValue];
                    code == LOGIN_ITEM
                }
                None => false,
            };
            let restore: Option<objc2::rc::Retained<AnyObject>> = msg_send![&*event, paramDescriptorForKeyword: RESTORE];
            let restore = match restore {
                Some(r) => {
                    let code: u32 = msg_send![&*r, enumCodeValue];
                    code == YES
                }
                None => false,
            };
            let sender_is_loginwindow = restore && {
                let sender: Option<objc2::rc::Retained<AnyObject>> = msg_send![&*event, attributeDescriptorForKeyword: SENDER_PID];
                match sender {
                    Some(d) => {
                        let pid: i32 = msg_send![&*d, int32Value];
                        pid_path(pid).is_some_and(|p| crate::login_item::is_loginwindow(&p))
                    }
                    None => false,
                }
            };
            OpenEvent {
                login_item: direct.is_some() || by_props,
                restore,
                sender_is_loginwindow,
            }
        }
    }

    /// The executable path of process `pid` now (`proc_pidpath`), `None` when it is gone or unreadable.
    fn pid_path(pid: i32) -> Option<String> {
        unsafe extern "C" {
            fn proc_pidpath(pid: i32, buffer: *mut std::ffi::c_void, size: u32) -> i32;
        }
        if pid <= 0 {
            return None;
        }
        let mut buf = vec![0u8; 4096];
        // SAFETY: `buf` is valid for `size` bytes; the call writes at most that many and returns the length written.
        let n = unsafe { proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
        if n <= 0 {
            return None;
        }
        buf.truncate(n as usize);
        String::from_utf8(buf).ok()
    }

    fn service() -> objc2::rc::Retained<SMAppService> {
        let name = NSString::from_str(PLIST);
        // SAFETY: a class method taking a valid NSString.
        unsafe { SMAppService::agentServiceWithPlistName(&name) }
    }

    fn status() -> SMAppServiceStatus {
        // SAFETY: a property read on a valid object.
        unsafe { service().status() }
    }

    fn registration_status() -> RegistrationStatus {
        match status() {
            SMAppServiceStatus::Enabled => RegistrationStatus::Enabled,
            SMAppServiceStatus::RequiresApproval => RegistrationStatus::RequiresApproval,
            SMAppServiceStatus::NotFound => RegistrationStatus::NotFound,
            _ => RegistrationStatus::NotRegistered,
        }
    }

    /// Registers the launch agent; with `open_settings` a missing approval opens Login Items for the user (only when
    /// the user asked: the background repair never opens a window).
    fn register(open_settings: bool) -> Result<(), RegisterError> {
        let s = service();
        // SAFETY: a method call on a valid object; the error is returned as a Retained NSError.
        match unsafe { s.registerAndReturnError() } {
            Ok(()) => {
                record_version();
                Ok(())
            }
            Err(e) => {
                if status() == SMAppServiceStatus::RequiresApproval {
                    if open_settings {
                        // SAFETY: a class method without arguments.
                        unsafe { SMAppService::openSystemSettingsLoginItems() };
                    }
                    return Err(RegisterError::NeedsApproval);
                }
                if status() == SMAppServiceStatus::NotFound {
                    return Err(RegisterError::Other(
                        "run Dyno from the Applications folder (the launch agent is part of the app bundle)".into(),
                    ));
                }
                Err(RegisterError::Other(e.localizedDescription().to_string()))
            }
        }
    }

    fn version_file() -> PathBuf {
        dyno_agent::paths::Dirs::current().root().join(REGISTERED_VERSION_FILE)
    }

    /// Remembers that this version registered the agent (a failure only costs one more re-registration).
    fn record_version() {
        if let Err(e) = record_registered_version(&version_file(), env!("CARGO_PKG_VERSION")) {
            crate::logging::info(&format!("could not remember the registered agent version: {e}"));
        }
    }

    /// `gui/<uid>`.
    fn gui_domain() -> Option<String> {
        use std::os::unix::fs::MetadataExt;
        let home = std::env::var_os("HOME")?;
        Some(format!("gui/{}", std::fs::metadata(home).ok()?.uid()))
    }

    /// `gui/<uid>/<label>`.
    fn target() -> Option<String> {
        Some(format!("{}/{LABEL}", gui_domain()?))
    }

    /// launchd's report of the job (nothing found when it cannot be read).
    fn job_report() -> JobReport {
        target()
            .and_then(|t| paddock_service::launchctl::job_report(&mut system(), &t))
            .unwrap_or_default()
    }

    /// Whether launchd's override database marks the agent disabled (`launchctl print-disabled gui/<uid>`).
    fn launchd_disabled() -> Option<bool> {
        let report = launchctl(&["print-disabled", &gui_domain()?])?;
        parse_print_disabled(&report, LABEL)
    }

    /// The shared bounded tool runner: output read while the tool runs, the tree ended at the timeout.
    fn system() -> RealSystem {
        RealSystem::new().with_timeout(Duration::from_secs(5))
    }

    /// `launchctl <args>` through it; `None` when it cannot be run, times out or fails.
    fn launchctl(args: &[&str]) -> Option<String> {
        launchctl_output(&mut system(), args)
    }

    /// The repair plan for the observed state, run; `requested` makes it replace the registration whatever launchd reports.
    fn repair_as(agent_answers: &dyn Fn() -> bool, requested: bool) -> Result<bool, String> {
        let registered = registered_version(&version_file());
        let observed = Observed {
            status: registration_status(),
            launchd_disabled: launchd_disabled(),
            answers: agent_answers(),
            job: job_report(),
            registered_version: registered.as_deref(),
            current_version: env!("CARGO_PKG_VERSION"),
            requested,
        };
        keep_registered(
            plan(&observed),
            &mut MacOps { answers: agent_answers },
            &mut record_version,
            &mut |step| crate::logging::info(&format!("agent service: {step}")),
        )
    }

    /// [`RepairOps`] over `SMAppService` and `launchctl`.
    struct MacOps<'a> {
        answers: &'a dyn Fn() -> bool,
    }

    impl RepairOps for MacOps<'_> {
        fn register(&mut self) -> Result<(), RegisterError> {
            register(false)
        }
        fn unregister(&mut self) -> Result<(), String> {
            SmService.uninstall()
        }
        fn bootout(&mut self) -> Result<(), String> {
            let t = target().ok_or("the user's launchd domain is unknown")?;
            paddock_service::launchctl::bootout(&mut system(), &t)
        }
        fn job_gone(&mut self) -> bool {
            !job_report().found
        }
        fn job(&mut self) -> JobReport {
            job_report()
        }
        fn service_answers(&mut self) -> bool {
            // The app may have started an agent of its own meanwhile; launchd running the job counts too.
            (self.answers)() || job_report().running
        }
        fn pause(&mut self, how_long: Duration) {
            std::thread::sleep(how_long);
        }
    }

    impl AgentService for SmService {
        fn installed(&mut self) -> bool {
            matches!(status(), SMAppServiceStatus::Enabled | SMAppServiceStatus::RequiresApproval)
        }

        /// Enabled, not switched off in Login Items and loaded in launchd: it runs the job (`RunAtLoad`, `KeepAlive`).
        /// Waiting for approval, switched off or dropped by launchd, it runs nothing.
        fn supervises(&mut self) -> bool {
            service_supervises(registration_status(), launchd_disabled(), job_report())
        }

        fn install(&mut self) -> Result<(), String> {
            register(true).map_err(|e| match e {
                RegisterError::NeedsApproval => {
                    "macOS needs your approval: allow Dyno in System Settings > General > Login Items (opened for you)".into()
                }
                RegisterError::Other(e) => e,
            })
        }

        fn uninstall(&mut self) -> Result<(), String> {
            if status() == SMAppServiceStatus::NotRegistered || status() == SMAppServiceStatus::NotFound {
                return Ok(());
            }
            // SAFETY: a method call on a valid object.
            unsafe { service().unregisterAndReturnError() }.map_err(|e| e.localizedDescription().to_string())
        }

        /// After an update launchd refuses the registered agent (its stored launch constraint is the previous build's
        /// cdhash) or keeps the old job: replace the registration when launchd reports it stale while the agent does
        /// not answer, and once after every version change ([`crate::service_repair`]). What the user switched off in
        /// Login Items is left alone, and nothing is registered that the user never turned on.
        fn repair(&mut self, agent_answers: &dyn Fn() -> bool) -> Result<bool, String> {
            repair_as(agent_answers, false)
        }

        /// The launchd job `gui/<uid>/net.decryptology.dyno.agent`, while launchd holds it. The `SMAppService` registration is
        /// not touched: `launchctl bootout` takes the job out of launchd (so `KeepAlive` cannot restart it) and the registration
        /// loads it again at the next login.
        fn stop_mechanism(&mut self) -> Option<paddock_service::stop::StopMechanism> {
            let target = target()?;
            job_report()
                .found
                .then_some(paddock_service::stop::StopMechanism::Launchd { target })
        }

        /// After a stop on request launchd holds no job and `RunAtLoad` fires only when a job is loaded, so the registration is
        /// replaced the way a repair does (remove the job, wait until it is gone, register, check): the same proven path.
        /// What the user switched off in Login Items stays off ([`plan`]).
        fn resume(&mut self) -> Result<bool, String> {
            repair_as(&|| false, true)
        }
    }
}

/// The platform's agent service.
#[cfg(target_os = "macos")]
pub fn platform_service() -> Box<dyn AgentService> {
    Box::new(SmService)
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
    Box::new(MainAppLogin)
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

    impl AgentService for FakeService {
        fn installed(&mut self) -> bool {
            false
        }
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
        fn repair(&mut self, _: &dyn Fn() -> bool) -> Result<bool, String> {
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

    #[test]
    fn one_failing_does_not_stop_the_other_and_the_message_says_what_to_do() {
        let (r, log) = apply(true, true, false);
        assert_eq!(log.len(), 2, "autostart is still attempted");
        let m = r.unwrap_err();
        assert!(
            m.starts_with("Your settings are saved") && m.contains("background agent") && m.contains("no"),
            "{m}"
        );
        let (r, _) = apply(true, true, true);
        assert!(r.unwrap_err().contains("; and "));
        assert!(apply(false, false, true).0.unwrap_err().contains("login items"));
    }

    #[test]
    fn an_unstable_location_registers_neither_but_turning_off_always_runs() {
        let calls = Calls::default();
        let mut s = FakeService {
            calls: calls.0.clone(),
            fail: false,
        };
        let mut a = FakeAutostart {
            calls: calls.0.clone(),
            fail: false,
        };
        let bad = || Err("move Dyno to Applications".to_string());
        let m = apply_launch_at_login(true, bad(), &mut s, &mut a).unwrap_err();
        assert!(
            m.contains("move Dyno to Applications") && m.starts_with("Your settings are saved"),
            "{m}"
        );
        assert!(calls.0.lock().unwrap().is_empty(), "nothing was registered");
        assert_eq!(apply_launch_at_login(false, bad(), &mut s, &mut a), Ok(()));
        assert_eq!(calls.0.lock().unwrap().len(), 2, "both were removed");
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

    impl AgentService for StaleService {
        fn installed(&mut self) -> bool {
            false
        }
        fn install(&mut self) -> Result<(), String> {
            self.calls.lock().unwrap().push("service install".into());
            Ok(())
        }
        fn uninstall(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn repair(&mut self, _: &dyn Fn() -> bool) -> Result<bool, String> {
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
