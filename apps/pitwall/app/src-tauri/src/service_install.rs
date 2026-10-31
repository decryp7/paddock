//! Registering the Pitwall server with the OS from the app. Elevation and approval are always the OS's own: UAC on
//! Windows (the installer does this once; the app can repeat it), the Login Items approval on macOS (`SMAppService`
//! with the launch agent inside the app bundle); on Linux nothing is elevated (a systemd user unit written by
//! `pitwall-server install`). Pitwall never asks for or handles a password. Adapted from Pitcrew's service_install.

use crate::service_repair::RepairNotice;
#[cfg(not(target_os = "macos"))]
use crate::service_repair::{RepairOutcome, RepairPhase};
use pitwall_core::mgmt::{CommandError, ErrorCode};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// The service's state as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInstall {
    /// `windows`, `macos` or `linux`.
    pub platform: &'static str,
    /// Registered with the OS.
    pub installed: bool,
    /// The server answers.
    pub running: bool,
    /// The OS integration can be switched on from the app.
    pub can_install: bool,
    /// The user has not been asked yet: show the "run Pitwall's service?" question.
    pub ask_consent: bool,
    /// What to do next, when something is off.
    pub message: Option<String>,
    /// The background repair of a registration macOS refused, while it works and after (macOS only).
    pub repair: Option<RepairNotice>,
    /// The user switched the background item off in System Settings > General > Login Items (macOS only).
    pub disabled_in_login_items: bool,
    /// The service is registered or known to launchd but cannot start, and the server does not answer (macOS): offer Repair.
    pub start_failed: bool,
    /// launchd still has the job but System Settings > Login Items does not list Pitwall (macOS): offer Repair.
    pub not_in_login_items: bool,
}

/// The server program's file name.
pub const SERVER_EXE: &str = if cfg!(windows) { "pitwall-server.exe" } else { "pitwall-server" };

/// The server program in `dir` (Tauri puts sidecars next to the app's executable), if it is really there.
pub fn server_exe_in(dir: &Path) -> Result<PathBuf, CommandError> {
    let p = dir.join(SERVER_EXE);
    if p.metadata().is_ok_and(|m| m.len() > 0) {
        Ok(p)
    } else {
        Err(CommandError::new(
            ErrorCode::NotAvailable,
            format!(
                "{} is missing; reinstall Pitwall (development builds: build pitwall-server and copy it there)",
                p.display()
            ),
        ))
    }
}

/// The server program next to this app.
pub fn server_exe() -> Result<PathBuf, CommandError> {
    let exe =
        std::env::current_exe().map_err(|e| CommandError::new(ErrorCode::Internal, format!("the app's folder cannot be found: {e}")))?;
    let dir = exe
        .parent()
        .ok_or_else(|| CommandError::new(ErrorCode::Internal, "the app's folder cannot be found"))?;
    server_exe_in(dir)
}

/// What the OS says about the registration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OsState {
    /// Registered.
    pub installed: bool,
    /// What to do next.
    pub message: Option<String>,
    /// The macOS repair's progress, if one ran.
    pub repair: Option<RepairNotice>,
    /// The user switched the item off in Login Items (macOS).
    pub disabled_in_login_items: bool,
    /// launchd holds a job it cannot start ([`crate::service_repair::JobReport::stale`]).
    pub job_stale: bool,
    /// launchd has the job, SMAppService does not know the registration.
    pub job_unlisted: bool,
}

/// Puts the OS state, the server's answer and the first-run decision together.
pub fn compose(os: OsState, running: bool, ask_consent: bool) -> ServiceInstall {
    let message = os.message.or_else(|| match (os.installed, running) {
        (true, false) => {
            Some("The service is installed but the server does not answer yet; give it a moment, or restart the service.".into())
        }
        (false, true) => Some("The server is running outside the service (started by hand); it stops when you log out.".into()),
        _ => None,
    });
    let repairing = os
        .repair
        .as_ref()
        .is_some_and(|r| r.state == crate::service_repair::RepairState::Working);
    let broken = !running && !os.disabled_in_login_items && !repairing;
    let start_failed = broken && os.job_stale;
    let not_in_login_items = broken && os.job_unlisted;
    ServiceInstall {
        platform: PLATFORM,
        installed: os.installed,
        running,
        can_install: true,
        ask_consent: ask_consent && !os.installed && !running,
        message,
        repair: os.repair,
        disabled_in_login_items: os.disabled_in_login_items,
        start_failed,
        not_in_login_items,
    }
}

/// The platform name the UI sees.
pub const PLATFORM: &str = if cfg!(windows) {
    "windows"
} else if cfg!(target_os = "macos") {
    "macos"
} else {
    "linux"
};

#[cfg(windows)]
mod imp {
    //! SCM queries are safe wrappers; elevation is `ShellExecuteExW` with the "runas" verb, the one unsafe call.
    #![allow(unsafe_code)]

    use super::*;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    use windows_service::service::{ServiceAccess, ServiceState};
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    /// The service's name in the Service Control Manager (`pitwall_server::service::spec::WINDOWS_SERVICE_NAME`).
    const SERVICE_NAME: &str = "Pitwall";

    /// Whether the service is registered.
    pub fn os_state() -> OsState {
        let Ok(manager) = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT) else {
            return OsState {
                installed: false,
                message: Some("the Service Control Manager cannot be asked".into()),
                ..OsState::default()
            };
        };
        match manager.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS) {
            Ok(s) => {
                let running = s
                    .query_status()
                    .map(|st| st.current_state == ServiceState::Running)
                    .unwrap_or(false);
                OsState {
                    installed: true,
                    message: (!running).then(|| "installed but not running; Windows restarts it, or start it from Services".to_string()),
                    ..OsState::default()
                }
            }
            Err(_) => OsState::default(),
        }
    }

    /// Runs `pitwall-server <verb>` with Windows' elevation prompt and waits for it (two minutes at most).
    fn elevated(verb: &str) -> Result<(), CommandError> {
        const ERROR_CANCELLED: i32 = 1223;
        let exe = server_exe()?;
        let op = HSTRING::from("runas");
        let file = HSTRING::from(exe.as_os_str());
        let params = HSTRING::from(verb);
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            lpVerb: windows::core::PCWSTR(op.as_ptr()),
            lpFile: windows::core::PCWSTR(file.as_ptr()),
            lpParameters: windows::core::PCWSTR(params.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        // SAFETY: every pointer in `info` refers to an HSTRING that outlives the call.
        match unsafe { ShellExecuteExW(&mut info) } {
            Ok(()) => {}
            Err(e) if e.code().0 & 0xFFFF == ERROR_CANCELLED => {
                return Err(CommandError::new(ErrorCode::Forbidden, "Windows' permission prompt was declined"));
            }
            Err(e) => {
                return Err(CommandError::new(
                    ErrorCode::Internal,
                    format!("the server program could not be started: {e}"),
                ))
            }
        }
        if info.hProcess.is_invalid() {
            return Ok(());
        }
        // SAFETY: a valid process handle we own; waited on and closed once.
        let code = unsafe {
            let waited = WaitForSingleObject(info.hProcess, 120_000);
            let mut code = 1u32;
            if waited == WAIT_OBJECT_0 {
                let _ = GetExitCodeProcess(info.hProcess, &mut code);
            }
            let _ = CloseHandle(info.hProcess);
            code
        };
        if code == 0 {
            Ok(())
        } else {
            Err(CommandError::new(
                ErrorCode::Internal,
                format!("pitwall-server {verb} failed (exit code {code}); the service log in %ProgramData%\\Pitwall\\logs has details"),
            ))
        }
    }

    /// Registers and starts the service (the OS asks the user).
    pub fn install() -> Result<(), CommandError> {
        elevated("install")
    }

    /// Stops and removes the service.
    pub fn uninstall() -> Result<(), CommandError> {
        elevated("uninstall")
    }
}

#[cfg(target_os = "macos")]
mod imp {
    //! `SMAppService` (macOS 13+). The plist is `Contents/Library/LaunchAgents/net.decryptology.pitwall.service.plist`
    //! in the app bundle (bundle.macOS.files in tauri.macos.conf.json), with `BundleProgram` pointing at the bundled
    //! server. The Objective-C calls are the only unsafe code; each is a plain message send on objects we own.
    #![allow(unsafe_code, unused_unsafe)]

    use super::*;
    use crate::app_state::{self, AppState};
    use crate::service_repair::{
        disabled_in_login_items, launchctl_output, not_found_message, parse_print_disabled, plan, run_repair, JobReport, Observed, Plan,
        RegisterError, RegistrationStatus, RepairOps, RepairOutcome, RepairPhase, REPAIR,
    };
    use objc2_foundation::NSString;
    use objc2_service_management::{SMAppService, SMAppServiceStatus};
    use paddock_service::launchctl::{bootout as launchd_bootout, job_report as launchd_job_report};
    use paddock_service::RealSystem;
    use std::time::{Duration, Instant};

    const PLIST: &str = "net.decryptology.pitwall.service.plist";
    const LABEL: &str = "net.decryptology.pitwall.service";

    fn service() -> objc2::rc::Retained<SMAppService> {
        let name = NSString::from_str(PLIST);
        // SAFETY: a class method taking a valid NSString.
        unsafe { SMAppService::agentServiceWithPlistName(&name) }
    }

    /// What `SMAppService` says, as the pure mapping's type.
    fn registration_status() -> RegistrationStatus {
        // SAFETY: a property read on a valid object.
        let status = unsafe { service().status() };
        if status == SMAppServiceStatus::Enabled {
            RegistrationStatus::Enabled
        } else if status == SMAppServiceStatus::RequiresApproval {
            RegistrationStatus::RequiresApproval
        } else if status == SMAppServiceStatus::NotFound {
            RegistrationStatus::NotFound
        } else {
            RegistrationStatus::NotRegistered
        }
    }

    /// What the OS says about the registration, with the repair's progress and the Login Items switch.
    pub fn os_state() -> OsState {
        let status = registration_status();
        let (installed, message) = match status {
            RegistrationStatus::Enabled => (true, None),
            RegistrationStatus::RequiresApproval => (true, Some("allow Pitwall in System Settings > General > Login Items".to_string())),
            RegistrationStatus::NotFound => (false, Some(not_found_message(registered_version().is_some()).to_string())),
            RegistrationStatus::NotRegistered => (false, None),
        };
        let job = job_report().unwrap_or_default();
        let message = message.or_else(|| {
            job.stale().then(|| {
                format!(
                    "launchd refused to start the service (error 78, bad configuration): the app was updated and macOS still holds the previous version's signature. Repair replaces it. (launchd reports: {}.)",
                    job.describe()
                )
            })
        });
        OsState {
            installed,
            message,
            repair: REPAIR.current(Instant::now()),
            disabled_in_login_items: disabled_in_login_items(status, launchd_disabled()),
            job_stale: job.stale(),
            job_unlisted: job.found && status == RegistrationStatus::NotRegistered,
        }
    }

    /// Opens System Settings > General > Login Items. The user switches the item; Pitwall never does it for them.
    pub fn open_login_items() -> Result<(), CommandError> {
        // SAFETY: a class method without arguments.
        unsafe { SMAppService::openSystemSettingsLoginItems() };
        Ok(())
    }

    /// Registers the launch agent; `open_settings` also opens Login Items when macOS wants the user's approval.
    fn register(open_settings: bool) -> Result<(), CommandError> {
        let s = service();
        // SAFETY: a method call on a valid object; the error is returned as a Retained NSError.
        match unsafe { s.registerAndReturnError() } {
            Ok(()) => Ok(()),
            Err(e) => {
                // SAFETY: as above.
                if unsafe { s.status() } == SMAppServiceStatus::RequiresApproval {
                    if open_settings {
                        // SAFETY: a class method without arguments.
                        unsafe { SMAppService::openSystemSettingsLoginItems() };
                    }
                    return Err(CommandError::new(
                        ErrorCode::Forbidden,
                        "macOS needs your approval: allow Pitwall in System Settings > General > Login Items",
                    ));
                }
                Err(CommandError::new(
                    ErrorCode::Internal,
                    format!("the launch agent could not be registered: {}", e.localizedDescription()),
                ))
            }
        }
    }

    /// Registers and starts the service (the OS asks the user).
    pub fn install() -> Result<(), CommandError> {
        register(true)?;
        record_version();
        Ok(())
    }

    /// Stops and removes the service.
    pub fn uninstall() -> Result<(), CommandError> {
        // SAFETY: a method call on a valid object.
        unsafe { service().unregisterAndReturnError() }.map_err(|e| {
            CommandError::new(
                ErrorCode::Internal,
                format!("the launch agent could not be removed: {}", e.localizedDescription()),
            )
        })
    }

    /// launchd's report of the job: `None` when it could not be asked (a timeout, a permission error), which is not
    /// the same as launchd not knowing the job.
    fn job_report() -> Option<JobReport> {
        launchd_job_report(&mut RealSystem::new(), &target()?)
    }

    fn registered_version() -> Option<String> {
        app_state::data_dir().and_then(|d| AppState::load(&d).service_registered_version)
    }

    /// Remembers that this version looked after the registration.
    fn record_version() {
        if let Some(dir) = app_state::data_dir() {
            let mut s = AppState::load(&dir);
            s.service_registered_version = Some(env!("CARGO_PKG_VERSION").to_string());
            if let Err(e) = s.save(&dir) {
                eprintln!("Pitwall: could not remember the registered version: {e}");
            }
        }
    }

    fn observe(answers: bool, requested: bool, registered: Option<&str>) -> Plan {
        plan(&Observed {
            status: registration_status(),
            launchd_disabled: launchd_disabled(),
            answers,
            job: job_report().unwrap_or_default(),
            registered_version: registered,
            current_version: env!("CARGO_PKG_VERSION"),
            requested,
        })
    }

    /// After the app was updated launchd refuses the registered service (its stored launch constraint is the previous
    /// build's cdhash, see [`JobReport::stale`]), and the job can be missing from Login Items. This registers the
    /// service again ([`run_repair`]) when launchd reports a stale job while the server is unreachable, and once after
    /// every version change; progress goes to [`REPAIR`] and every step to standard error. `None`: nothing to repair
    /// (including the user having switched the item off: that is their choice and is left alone).
    pub fn repair_registration(answers: &dyn Fn() -> bool) -> Option<RepairOutcome> {
        let registered = registered_version();
        // Cheap check first: a healthy, unchanged service needs no launchctl calls.
        let up = answers();
        match observe(up, false, registered.as_deref()) {
            Plan::Nothing => None,
            Plan::MarkCurrent => {
                record_version();
                None
            }
            Plan::Repair(_) => REPAIR.start(Instant::now()).then(|| perform(answers, &|| false, &|_| {})),
        }
    }

    fn refuse_when_not_repairable() -> Result<(), CommandError> {
        if observe(false, true, None) == Plan::Nothing {
            return Err(CommandError::new(
                ErrorCode::Forbidden,
                "Pitwall's background item is switched off in System Settings > General > Login Items, or the app is not in /Applications. Switch it on there; Pitwall never does it for you.",
            ));
        }
        Ok(())
    }

    /// The repair as a job body: runs it on the calling (blocking) thread. `cancel` is asked between steps and while
    /// waiting, `phase` gets each stage. Refused when the user switched the item off in Login Items or the launch agent is missing, and when another repair is working.
    pub fn repair_blocking(
        answers: &dyn Fn() -> bool,
        cancel: &dyn Fn() -> bool,
        phase: &dyn Fn(RepairPhase),
    ) -> Result<RepairOutcome, CommandError> {
        refuse_when_not_repairable()?;
        if !REPAIR.start(Instant::now()) {
            return Err(CommandError::new(ErrorCode::Conflict, "A repair is already working."));
        }
        Ok(perform(answers, cancel, phase))
    }

    /// Runs the repair (the tracker is already working) and records the result.
    fn perform(answers: &dyn Fn() -> bool, cancel: &dyn Fn() -> bool, phase: &dyn Fn(RepairPhase)) -> RepairOutcome {
        eprintln!("Pitwall: the background service cannot start (stale registration or new version): repairing");
        let outcome = run_repair(&mut MacOps { answers, cancel, phase }, &mut |step| {
            eprintln!("Pitwall: repair: {step}")
        });
        REPAIR.finish(&outcome, Instant::now());
        if outcome == RepairOutcome::Recovered {
            record_version();
        }
        eprintln!("Pitwall: repair finished: {outcome:?}");
        outcome
    }

    /// [`RepairOps`] over `SMAppService` and `launchctl`.
    struct MacOps<'a> {
        answers: &'a dyn Fn() -> bool,
        cancel: &'a dyn Fn() -> bool,
        phase: &'a dyn Fn(RepairPhase),
    }

    impl RepairOps for MacOps<'_> {
        fn register(&mut self) -> Result<(), RegisterError> {
            register(false).map_err(|e| {
                if e.code == ErrorCode::Forbidden {
                    RegisterError::NeedsApproval
                } else {
                    RegisterError::Other(e.message)
                }
            })
        }
        fn unregister(&mut self) -> Result<(), String> {
            uninstall().map_err(|e| e.message)
        }
        fn bootout(&mut self) -> Result<(), String> {
            let t = target().ok_or("the user's launchd domain is unknown")?;
            // The shared bootout: a job launchd has already dropped (exit 3 or 113) is success, not a failure to log.
            launchd_bootout(&mut RealSystem::new(), &t)
        }
        fn job_gone(&mut self) -> bool {
            job_report().is_some_and(|job| !job.found)
        }
        fn registration_stale(&mut self) -> bool {
            job_report().unwrap_or_default().stale()
        }
        fn service_answers(&mut self) -> bool {
            (self.answers)()
        }
        fn pause(&mut self, how_long: Duration) {
            std::thread::sleep(how_long);
        }
        fn cancelled(&mut self) -> bool {
            (self.cancel)()
        }
        fn phase(&mut self, phase: RepairPhase) {
            (self.phase)(phase);
        }
    }

    /// `gui/<uid>/<label>`.
    fn target() -> Option<String> {
        Some(format!("{}/{LABEL}", gui_domain()?))
    }

    fn gui_domain() -> Option<String> {
        use std::os::unix::fs::MetadataExt;
        let home = std::env::var_os("HOME")?;
        Some(format!("gui/{}", std::fs::metadata(home).ok()?.uid()))
    }

    /// Whether launchd's override database marks the service disabled (`launchctl print-disabled gui/<uid>`).
    fn launchd_disabled() -> Option<bool> {
        let report = launchctl(&["print-disabled", &gui_domain()?])?;
        parse_print_disabled(&report, LABEL)
    }

    /// `launchctl <args>` through the shared bounded reader (output read while it runs, the tree ended at the timeout);
    /// `None` when it cannot be run, times out or fails.
    fn launchctl(args: &[&str]) -> Option<String> {
        launchctl_output(&mut RealSystem::new(), args)
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    //! A systemd **user** service: the app runs `pitwall-server install` / `uninstall` (as the user, no elevation),
    //! which writes `~/.config/systemd/user/pitwall.service` and runs `systemctl --user`. The server program has the
    //! timeouts; this side only bounds the wait for it.
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const UNIT: &str = "pitwall.service";

    fn unit_file() -> Option<PathBuf> {
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
        env("XDG_CONFIG_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| env("HOME").map(|h| h.join(".config")))
            .map(|c| c.join("systemd").join("user").join(UNIT))
    }

    /// Whether the unit file exists.
    pub fn os_state() -> OsState {
        OsState {
            installed: unit_file().is_some_and(|p| p.exists()),
            message: None,
            ..OsState::default()
        }
    }

    /// Runs `pitwall-server <verb>`, waiting at most 90 s; its standard error becomes the message.
    fn run_verb(verb: &str) -> Result<(), CommandError> {
        let exe = server_exe()?;
        // The one place the app starts a program: it is our own server, bounded by the wait loop below (no console
        // window exists on Linux).
        #[allow(clippy::disallowed_methods)]
        let mut child = Command::new(&exe)
            .arg(verb)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| CommandError::new(ErrorCode::Internal, format!("the server program could not be started: {e}")))?;
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if started.elapsed() > Duration::from_secs(90) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CommandError::new(
                        ErrorCode::Internal,
                        format!("pitwall-server {verb} took too long and was stopped"),
                    ));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => {
                    return Err(CommandError::new(
                        ErrorCode::Internal,
                        format!("waiting for pitwall-server {verb} failed: {e}"),
                    ))
                }
            }
        };
        if status.success() {
            return Ok(());
        }
        let mut message = String::new();
        if let Some(mut err) = child.stderr.take() {
            use std::io::Read;
            let _ = err.by_ref().take(2048).read_to_string(&mut message);
        }
        let reason = message.trim();
        Err(CommandError::new(
            ErrorCode::Internal,
            if reason.is_empty() {
                format!("pitwall-server {verb} failed")
            } else {
                reason.to_string()
            },
        ))
    }

    /// Registers and starts the service (as the user).
    pub fn install() -> Result<(), CommandError> {
        run_verb("install")
    }

    /// Stops and removes the service.
    pub fn uninstall() -> Result<(), CommandError> {
        run_verb("uninstall")
    }
}

pub use imp::{install, os_state, uninstall};

#[cfg(target_os = "macos")]
pub use imp::{open_login_items, repair_blocking, repair_registration};

/// Opens System Settings > General > Login Items, where the user switches the background item on. Only macOS has one.
#[cfg(not(target_os = "macos"))]
pub fn open_login_items() -> Result<(), CommandError> {
    Err(CommandError::new(ErrorCode::NotAvailable, "Login Items exist on macOS only"))
}

/// Repair exists on macOS only; elsewhere the service is installed or removed from the app.
#[cfg(not(target_os = "macos"))]
pub fn repair_blocking(
    _answers: &dyn Fn() -> bool,
    _cancel: &dyn Fn() -> bool,
    _phase: &dyn Fn(RepairPhase),
) -> Result<RepairOutcome, CommandError> {
    Err(CommandError::new(ErrorCode::NotAvailable, "Repair exists on macOS only"))
}

/// How many times a run of the app checks for a stale service registration (macOS).
pub const REPAIR_ATTEMPTS: u32 = 8;

/// The wait before check number `attempt`: soon after start, then every minute (the service may only fail later,
/// when it restarts itself onto a new binary).
pub fn repair_delay(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_secs(if attempt == 0 { 3 } else { 60 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_checks_soon_then_every_minute() {
        assert_eq!(repair_delay(0).as_secs(), 3);
        assert_eq!(repair_delay(1).as_secs(), 60);
        const { assert!(REPAIR_ATTEMPTS >= 5) };
    }

    #[test]
    fn the_repair_and_the_login_item_switch_reach_the_ui() {
        let os = OsState {
            installed: true,
            repair: Some(RepairNotice {
                state: crate::service_repair::RepairState::Working,
                message: "m".into(),
            }),
            disabled_in_login_items: true,
            ..OsState::default()
        };
        let s = compose(os, false, false);
        assert!(s.disabled_in_login_items && s.repair.is_some());
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["disabledInLoginItems"], true);
        assert_eq!(json["repair"]["state"], "working");
    }

    #[test]
    fn repair_is_offered_when_the_job_cannot_start_or_is_missing_from_login_items() {
        let stale = OsState {
            job_stale: true,
            job_unlisted: true,
            ..OsState::default()
        };
        let s = compose(stale.clone(), false, false);
        assert!(s.start_failed && s.not_in_login_items);
        // Running, repairing or switched off by the user: no Repair offer.
        assert!(!compose(stale.clone(), true, false).start_failed);
        let mut working = stale.clone();
        working.repair = Some(RepairNotice {
            state: crate::service_repair::RepairState::Working,
            message: "m".into(),
        });
        assert!(!compose(working, false, false).start_failed);
        let mut off = stale;
        off.disabled_in_login_items = true;
        let s = compose(off, false, false);
        assert!(!s.start_failed && !s.not_in_login_items);
        let json = serde_json::to_value(compose(OsState::default(), false, false)).unwrap();
        assert_eq!(
            (json["startFailed"].clone(), json["notInLoginItems"].clone()),
            (false.into(), false.into())
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn repair_exists_on_macos_only() {
        assert_eq!(
            repair_blocking(&|| false, &|| false, &|_| {}).unwrap_err().code,
            ErrorCode::NotAvailable
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn login_items_exist_on_macos_only() {
        assert_eq!(open_login_items().unwrap_err().code, ErrorCode::NotAvailable);
    }

    #[test]
    fn the_server_program_must_exist_and_not_be_an_empty_placeholder() {
        let d = tempfile::tempdir().unwrap();
        let e = server_exe_in(d.path()).unwrap_err();
        assert_eq!(e.code, ErrorCode::NotAvailable);
        assert!(e.message.contains("reinstall Pitwall"));
        std::fs::write(d.path().join(SERVER_EXE), b"").unwrap();
        assert!(server_exe_in(d.path()).is_err(), "the build placeholder is empty");
        std::fs::write(d.path().join(SERVER_EXE), b"binary").unwrap();
        assert_eq!(server_exe_in(d.path()).unwrap(), d.path().join(SERVER_EXE));
    }

    #[test]
    fn the_state_tells_the_user_what_is_off() {
        let none = OsState::default();
        let yes = OsState {
            installed: true,
            ..OsState::default()
        };
        // Fresh machine: ask.
        let s = compose(none.clone(), false, true);
        assert!(s.ask_consent && !s.installed && !s.running && s.message.is_none());
        // The question is asked once.
        assert!(!compose(none.clone(), false, false).ask_consent);
        // Never asked when something already runs the server.
        assert!(!compose(none.clone(), true, true).ask_consent);
        assert!(!compose(yes.clone(), false, true).ask_consent);
        // Hints.
        assert!(compose(yes.clone(), false, false).message.unwrap().contains("restart the service"));
        assert!(compose(none, true, false).message.unwrap().contains("started by hand"));
        assert!(compose(yes.clone(), true, false).message.is_none());
        // The OS's own message wins.
        let os = OsState {
            installed: true,
            message: Some("allow it".into()),
            ..OsState::default()
        };
        assert_eq!(compose(os, false, false).message.as_deref(), Some("allow it"));
        assert!(["windows", "macos", "linux"].contains(&PLATFORM));
    }
}
