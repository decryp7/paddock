//! Registering the Pitcrew service with the OS from the app (ADR 0003). Elevation and approval are always the OS's
//! own: UAC on Windows (the installer does this once; the app can repeat it), the Login Items approval on macOS;
//! on Linux nothing is elevated (a systemd user service).
//! Pitcrew never asks for or handles a password.

use crate::dto::{CommandError, CommandErrorCode, ServiceInstall};

/// What to do about the per-user engine when the app looks for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineStart {
    /// It answers: nothing to do.
    Running,
    /// Start it, as this user, hidden.
    Start,
    /// This app runs elevated: starting it from here would start it elevated, and it must never run so (it takes requests from the
    /// user's own programs). Not started; the window says to start Pitcrew normally.
    SkipElevated,
}

/// Decides. Pure; the engine's own start refuses an elevated token too, so this is the first of two checks.
pub fn engine_start_decision(running: bool, app_elevated: bool) -> EngineStart {
    if running {
        EngineStart::Running
    } else if app_elevated {
        EngineStart::SkipElevated
    } else {
        EngineStart::Start
    }
}

/// What the window says when the app skipped the engine because it runs elevated.
pub const ELEVATED_SKIP_NOTICE: &str =
    "Pitcrew is running as administrator, so it does not start the engine for your own runners (that engine must never run elevated). Close Pitcrew and start it normally to use them.";

/// The installed service binary next to the app's executable (Tauri puts sidecars there).
pub fn service_exe() -> Result<std::path::PathBuf, CommandError> {
    let exe = std::env::current_exe().map_err(|e| CommandError::internal(format!("the app's folder cannot be found: {e}")))?;
    let dir = exe
        .parent()
        .ok_or_else(|| CommandError::internal("the app's folder cannot be found"))?;
    let name = if cfg!(windows) { "pitcrew-service.exe" } else { "pitcrew-service" };
    let p = dir.join(name);
    if p.metadata().is_ok_and(|m| m.len() > 0) {
        Ok(p)
    } else {
        Err(CommandError::new(
            CommandErrorCode::NotImplemented,
            format!(
                "{} is missing; reinstall Pitcrew (development builds: build pitcrew-service and copy it there)",
                p.display()
            ),
        ))
    }
}

/// What the OS says about the registration (macOS).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OsState {
    /// Registered.
    pub installed: bool,
    /// What to do next, in words.
    pub message: Option<String>,
    /// The repair's progress, if one ran.
    pub repair: Option<crate::service_repair::RepairNotice>,
    /// The user switched the item off in Login Items.
    pub disabled_in_login_items: bool,
    /// launchd holds a job it cannot start ([`crate::service_repair::JobReport::stale`]).
    pub job_stale: bool,
    /// launchd has the job, `SMAppService` does not know the registration.
    pub job_unlisted: bool,
}

/// Puts the OS state and the service's answer together (macOS).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn compose(os: OsState, running: bool) -> ServiceInstall {
    let repairing = os
        .repair
        .as_ref()
        .is_some_and(|r| r.state == crate::service_repair::RepairState::Working);
    let broken = !running && !os.disabled_in_login_items && !repairing;
    ServiceInstall {
        platform: "macos",
        installed: os.installed,
        running,
        can_install: true,
        message: os.message,
        repair: os.repair,
        disabled_in_login_items: os.disabled_in_login_items,
        start_failed: broken && os.job_stale,
        not_in_login_items: broken && os.job_unlisted,
    }
}

#[cfg(windows)]
mod imp {
    //! SCM queries are safe wrappers; elevation is `ShellExecuteExW` with the "runas" verb, the one unsafe call.
    #![allow(unsafe_code)]

    use super::*;
    use pitcrew_core::service_spec::WINDOWS_SERVICE_NAME;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    use windows_service::service::{ServiceAccess, ServiceState};
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    /// Whether the service is registered and running.
    pub fn state() -> ServiceInstall {
        let base = ServiceInstall {
            platform: "windows",
            installed: false,
            running: false,
            can_install: true,
            message: None,
            repair: None,
            disabled_in_login_items: false,
            start_failed: false,
            not_in_login_items: false,
        };
        let Ok(manager) = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT) else {
            return ServiceInstall {
                message: Some("the Service Control Manager cannot be asked".into()),
                ..base
            };
        };
        match manager.open_service(WINDOWS_SERVICE_NAME, ServiceAccess::QUERY_STATUS) {
            Ok(s) => {
                let running = s
                    .query_status()
                    .map(|st| st.current_state == ServiceState::Running)
                    .unwrap_or(false);
                ServiceInstall {
                    installed: true,
                    running,
                    message: (!running).then(|| "installed but not running; Windows restarts it, or start it from Services".to_string()),
                    ..base
                }
            }
            Err(_) => base,
        }
    }

    /// Runs `pitcrew-service <verb>` with Windows' elevation prompt and waits for it.
    pub fn elevated(verb: &str) -> Result<(), CommandError> {
        run_elevated(&service_exe()?, verb, std::time::Duration::from_secs(120))
    }

    /// Runs `exe` with `params` through Windows' elevation prompt and waits at most `wait` for it to end; exit code 0 is success.
    pub fn run_elevated(exe: &std::path::Path, params: &str, wait: std::time::Duration) -> Result<(), CommandError> {
        const ERROR_CANCELLED: i32 = 1223;
        let verb = params.split_whitespace().next().unwrap_or("");
        let op = HSTRING::from("runas");
        let file = HSTRING::from(exe.as_os_str());
        let params = HSTRING::from(params);
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
                return Err(CommandError::new(
                    CommandErrorCode::Cancelled,
                    "Windows' permission prompt was declined",
                ))
            }
            Err(e) => return Err(CommandError::internal(format!("the service program could not be started: {e}"))),
        }
        if info.hProcess.is_invalid() {
            return Ok(());
        }
        // SAFETY: a valid process handle we own; waited on (2 minutes at most) and closed once.
        let code = unsafe {
            let waited = WaitForSingleObject(info.hProcess, u32::try_from(wait.as_millis()).unwrap_or(u32::MAX));
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
            Err(CommandError::internal(format!(
                "{} {verb} failed (exit code {code}); the service log has details",
                exe.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())
            )))
        }
    }

    /// Whether this app runs with an elevated token.
    pub fn app_is_elevated() -> bool {
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        let mut token = HANDLE::default();
        // SAFETY: the pseudo handle of this process and a valid out pointer; the token is closed below.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.is_err() {
            return true; // not knowing is not "not elevated"
        }
        let mut info = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        // SAFETY: `info` is a TOKEN_ELEVATION of the size passed; `token` is valid.
        let read = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                Some((&mut info as *mut TOKEN_ELEVATION).cast()),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut returned,
            )
        };
        // SAFETY: the handle opened above, closed once.
        unsafe {
            let _ = CloseHandle(token);
        }
        read.is_err() || info.TokenIsElevated != 0
    }

    /// Starts the **per-user engine** (`pitcrew-service run`, hidden, as this user, no prompt) unless it already answers. It keeps
    /// the runners in folders the user can change (ADR 0004, second amendment); the app starts it whenever it finds it not
    /// running, and the app itself starts at log-on.
    pub fn start_user_engine() -> Result<(), CommandError> {
        use crate::service_client::{ServiceClient, UserEngineClient};
        let running = UserEngineClient.request(pitcrew_core::protocol::Request::Status).is_ok();
        match engine_start_decision(running, app_is_elevated()) {
            EngineStart::Running => return Ok(()),
            EngineStart::SkipElevated => return Err(CommandError::new(CommandErrorCode::PermissionDenied, ELEVATED_SKIP_NOTICE)),
            EngineStart::Start => {}
        }
        let exe = service_exe()?;
        let op = HSTRING::from("open");
        let file = HSTRING::from(exe.as_os_str());
        let params = HSTRING::from("run");
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOASYNC,
            lpVerb: windows::core::PCWSTR(op.as_ptr()),
            lpFile: windows::core::PCWSTR(file.as_ptr()),
            lpParameters: windows::core::PCWSTR(params.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        // SAFETY: every pointer in `info` refers to an HSTRING that outlives the call; no process handle is requested.
        unsafe { ShellExecuteExW(&mut info) }
            .map_err(|e| CommandError::internal(format!("the engine for your own runners could not be started: {e}")))
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
    //! `SMAppService` (macOS 13+). The plist is `Contents/Library/LaunchAgents/net.decryptology.pitcrew.service.plist`
    //! in the app bundle (bundle.macOS.files in tauri.macos.conf.json), with `BundleProgram` pointing at the bundled
    //! service. The Objective-C calls are the only unsafe code; each is a plain message send on objects we own.
    #![allow(unsafe_code, unused_unsafe)]

    use super::*;
    use crate::app_state::AppState;
    use crate::applog;
    use crate::service_client::ServiceClient;
    use crate::service_repair::{
        disabled_in_login_items, parse_job, parse_print_disabled, plan, run_repair, JobReport, Observed, Plan, RegisterError,
        RegistrationStatus, RepairOps, RepairOutcome, REPAIR,
    };
    use objc2_foundation::NSString;
    use objc2_service_management::{SMAppService, SMAppServiceStatus};
    use std::path::Path;
    use std::time::{Duration, Instant};

    const PLIST: &str = "net.decryptology.pitcrew.service.plist";

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

    /// The service answers.
    fn answers() -> bool {
        crate::service_client::LocalServiceClient
            .request(pitcrew_core::protocol::Request::Status)
            .is_ok()
    }

    /// Whether the service is registered and running.
    pub fn state() -> ServiceInstall {
        let status = registration_status();
        let running = answers();
        let (installed, message) = match status {
            RegistrationStatus::Enabled => (true, None),
            RegistrationStatus::RequiresApproval => (true, Some("allow Pitcrew in System Settings ▸ General ▸ Login Items".to_string())),
            RegistrationStatus::NotFound => (
                false,
                Some("run Pitcrew from /Applications (the launch agent is part of the app bundle)".to_string()),
            ),
            RegistrationStatus::NotRegistered => (false, None),
        };
        let job = job_report();
        let message = message.or_else(|| {
            job.stale().then(|| {
                "launchd refused to start the service (error 78, bad configuration): the app was updated and macOS still holds the previous version's signature. Repair replaces it."
                    .to_string()
            })
        });
        compose(
            OsState {
                installed,
                message,
                repair: REPAIR.current(Instant::now()),
                disabled_in_login_items: disabled_in_login_items(status, launchd_disabled()),
                job_stale: job.stale(),
                job_unlisted: job.found && status == RegistrationStatus::NotRegistered,
            },
            running,
        )
    }

    /// Opens System Settings ▸ General ▸ Login Items. The user switches the item; Pitcrew never does it for them.
    pub fn open_login_items() -> Result<(), CommandError> {
        // SAFETY: a class method without arguments.
        unsafe { SMAppService::openSystemSettingsLoginItems() };
        Ok(())
    }

    /// Registers and starts the service (the OS asks the user).
    pub fn install() -> Result<(), CommandError> {
        register()
    }

    fn register() -> Result<(), CommandError> {
        let s = service();
        // SAFETY: a method call on a valid object; the error is returned as a Retained NSError.
        match unsafe { s.registerAndReturnError() } {
            Ok(()) => Ok(()),
            Err(e) => {
                // SAFETY: as above.
                if unsafe { s.status() } == SMAppServiceStatus::RequiresApproval {
                    // SAFETY: a class method without arguments.
                    unsafe { SMAppService::openSystemSettingsLoginItems() };
                    return Err(CommandError::new(
                        CommandErrorCode::PermissionDenied,
                        "macOS needs your approval: allow Pitcrew in System Settings ▸ General ▸ Login Items (opened for you)",
                    ));
                }
                Err(CommandError::internal(format!(
                    "the launch agent could not be registered: {}",
                    e.localizedDescription()
                )))
            }
        }
    }

    /// The user registered the service from the app: this version has looked after it.
    pub fn note_registered(dir: &Path) {
        record_version(dir);
    }

    /// Stops and removes the service.
    pub fn uninstall() -> Result<(), CommandError> {
        // SAFETY: a method call on a valid object.
        unsafe { service().unregisterAndReturnError() }
            .map_err(|e| CommandError::internal(format!("the launch agent could not be removed: {}", e.localizedDescription())))
    }

    /// launchd's report of the job (nothing found when it cannot be read).
    fn job_report() -> JobReport {
        target()
            .and_then(|t| launchctl(&["print", &t]))
            .map(|r| parse_job(&r))
            .unwrap_or_default()
    }

    /// Remembers in `dir` that this version looked after the registration.
    fn record_version(dir: &Path) {
        let mut s = AppState::load(dir);
        s.service_registered_version = Some(env!("CARGO_PKG_VERSION").to_string());
        if let Err(e) = s.save(dir) {
            applog::write(&format!("could not remember the registered version: {e}"));
        }
    }

    fn observe(dir: &Path, up: bool, requested: bool) -> Plan {
        let registered = AppState::load(dir).service_registered_version;
        plan(&Observed {
            status: registration_status(),
            launchd_disabled: launchd_disabled(),
            answers: up,
            job: job_report(),
            registered_version: registered.as_deref(),
            current_version: env!("CARGO_PKG_VERSION"),
            requested,
        })
    }

    /// After the app was updated launchd refuses the registered service (its stored launch constraint is the previous
    /// build's cdhash, see [`JobReport::stale`]), and the job can be missing from Login Items. This registers the
    /// service again ([`run_repair`]) when launchd reports a stale job while the service does not answer, and once after
    /// every version change; progress goes to [`REPAIR`] and every step to the app log. `None`: nothing to repair
    /// (including the user having switched the item off: that is their choice and is left alone).
    pub fn repair_registration(dir: &Path) -> Option<RepairOutcome> {
        match observe(dir, answers(), false) {
            Plan::Nothing => None,
            Plan::MarkCurrent => {
                record_version(dir);
                None
            }
            Plan::Repair(_) => REPAIR.start(Instant::now()).then(|| perform(dir)),
        }
    }

    /// The user pressed Repair: runs the repair in the background (progress in [`REPAIR`]). Refused when the user
    /// switched the item off in Login Items or the launch agent is missing from the app.
    pub fn request_repair(dir: &Path) -> Result<(), CommandError> {
        if observe(dir, false, true) == Plan::Nothing {
            return Err(CommandError::new(
                CommandErrorCode::PermissionDenied,
                "Pitcrew's background item is switched off in System Settings ▸ General ▸ Login Items, or the app is not in /Applications. Switch it on there; Pitcrew never does it for you.",
            ));
        }
        if REPAIR.start(Instant::now()) {
            let dir = dir.to_path_buf();
            std::thread::spawn(move || {
                perform(&dir);
            });
        }
        Ok(())
    }

    /// Runs the repair (the tracker is already working) and records the result.
    fn perform(dir: &Path) -> RepairOutcome {
        applog::write("the background service cannot start (stale registration or new version): repairing");
        let outcome = run_repair(&mut MacOps, &mut |step| applog::write(&format!("repair: {step}")));
        REPAIR.finish(&outcome, Instant::now());
        if outcome == RepairOutcome::Recovered {
            record_version(dir);
        }
        applog::write(&format!("repair finished: {outcome:?}"));
        outcome
    }

    /// [`RepairOps`] over `SMAppService` and `launchctl`.
    struct MacOps;

    impl RepairOps for MacOps {
        fn register(&mut self) -> Result<(), RegisterError> {
            register().map_err(|e| {
                if e.code == CommandErrorCode::PermissionDenied {
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
            launchctl(&["bootout", &t])
                .map(|_| ())
                .ok_or_else(|| "launchctl bootout could not be run".to_string())
        }
        fn job_gone(&mut self) -> bool {
            !job_report().found
        }
        fn registration_stale(&mut self) -> bool {
            job_report().stale()
        }
        fn service_answers(&mut self) -> bool {
            answers()
        }
        fn pause(&mut self, how_long: Duration) {
            std::thread::sleep(how_long);
        }
    }

    /// `gui/<uid>/<label>`.
    fn target() -> Option<String> {
        Some(format!("{}/{}", gui_domain()?, pitcrew_core::service_spec::LAUNCHD_LABEL))
    }

    fn gui_domain() -> Option<String> {
        use std::os::unix::fs::MetadataExt;
        let home = std::env::var_os("HOME")?;
        Some(format!("gui/{}", std::fs::metadata(home).ok()?.uid()))
    }

    /// Whether launchd's override database marks the service disabled (`launchctl print-disabled gui/<uid>`).
    fn launchd_disabled() -> Option<bool> {
        let report = launchctl(&["print-disabled", &gui_domain()?])?;
        parse_print_disabled(&report, pitcrew_core::service_spec::LAUNCHD_LABEL)
    }

    /// `launchctl <args>`, bounded to a few seconds; `None` when it cannot be run or read.
    fn launchctl(args: &[&str]) -> Option<String> {
        use std::process::{Command, Stdio};
        // macOS only: no console window to hide, and the wait below is bounded.
        #[allow(clippy::disallowed_methods)]
        let mut child = Command::new("/bin/launchctl")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
            }
        }
        let mut out = String::new();
        std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
        Some(out)
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    //! A systemd **user** service (ADR 0009): the app runs `pitcrew-service install` / `uninstall` (as the user, no
    //! elevation), which writes `~/.config/systemd/user/pitcrew.service` and runs `systemctl --user`. The service
    //! program has the timeouts; this side only bounds the wait for it.
    use super::*;
    use crate::service_client::ServiceClient;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const UNIT: &str = pitcrew_core::service_spec::SYSTEMD_UNIT;

    fn unit_file() -> Option<std::path::PathBuf> {
        let env = |k: &str| std::env::var_os(k).map(std::path::PathBuf::from);
        env("XDG_CONFIG_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| env("HOME").map(|h| h.join(".config")))
            .map(|c| c.join("systemd").join("user").join(UNIT))
    }

    /// Whether the service is registered and running.
    pub fn state() -> ServiceInstall {
        let installed = unit_file().is_some_and(|p| p.exists());
        let running = crate::service_client::LocalServiceClient
            .request(pitcrew_core::protocol::Request::Status)
            .is_ok();
        let message = match (installed, running) {
            (true, false) => Some(format!(
                "installed but not running: run `systemctl --user status {UNIT}` for the reason"
            )),
            (false, true) => Some("running outside systemd (started by hand)".to_string()),
            _ => None,
        };
        ServiceInstall {
            platform: "linux",
            installed,
            running,
            can_install: true,
            message,
            repair: None,
            disabled_in_login_items: false,
            start_failed: false,
            not_in_login_items: false,
        }
    }

    /// Runs `pitcrew-service <verb>`, waiting at most 90 s; its standard error becomes the message.
    fn run_verb(verb: &str) -> Result<(), CommandError> {
        let exe = service_exe()?;
        // The one place the app starts a program: it is our own service, bounded by the wait loop below (no console
        // window exists on Linux).
        #[allow(clippy::disallowed_methods)]
        let mut child = Command::new(&exe)
            .arg(verb)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| CommandError::internal(format!("the service program could not be started: {e}")))?;
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if started.elapsed() > Duration::from_secs(90) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CommandError::internal(format!(
                        "pitcrew-service {verb} took too long and was stopped"
                    )));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => return Err(CommandError::internal(format!("waiting for pitcrew-service {verb} failed: {e}"))),
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
        Err(CommandError::internal(if reason.is_empty() {
            format!("pitcrew-service {verb} failed")
        } else {
            reason.to_string()
        }))
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

#[cfg(windows)]
pub use imp::{app_is_elevated, run_elevated, start_user_engine};
pub use imp::{install, state, uninstall};

#[cfg(target_os = "macos")]
pub use imp::{note_registered, open_login_items, repair_registration, request_repair};

/// Opens System Settings ▸ General ▸ Login Items, where the user switches the background item on. Only macOS has one.
#[cfg(not(target_os = "macos"))]
pub fn open_login_items() -> Result<(), CommandError> {
    Err(CommandError::new(
        CommandErrorCode::NotImplemented,
        "Login Items exist on macOS only",
    ))
}

/// Remembers which version registered the service (macOS only; nothing elsewhere).
#[cfg(not(target_os = "macos"))]
pub fn note_registered(_dir: &std::path::Path) {}

/// Repair exists on macOS only; elsewhere the service is installed or removed from the app.
#[cfg(not(target_os = "macos"))]
pub fn request_repair(_dir: &std::path::Path) -> Result<(), CommandError> {
    Err(CommandError::new(CommandErrorCode::NotImplemented, "Repair exists on macOS only"))
}

/// How many times a run of the app checks for a stale service registration (macOS).
pub const REPAIR_ATTEMPTS: u32 = 8;

/// The wait before repair check `attempt`: 3 s first, then a minute apart (the service may restart onto a new binary
/// only once its runners are idle).
pub fn repair_delay(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_secs(if attempt == 0 { 3 } else { 60 })
}

#[cfg(test)]
mod stale_tests {
    use super::*;
    use crate::service_repair::{RepairNotice, RepairState};

    fn notice(state: RepairState) -> Option<RepairNotice> {
        Some(RepairNotice {
            state,
            message: "m".into(),
        })
    }

    #[test]
    fn repair_is_offered_when_the_job_cannot_start_or_is_missing_from_login_items() {
        let stale = OsState {
            job_stale: true,
            job_unlisted: true,
            ..OsState::default()
        };
        let s = compose(stale.clone(), false);
        assert!(s.start_failed && s.not_in_login_items);
        // Running, repairing or switched off by the user: no Repair offer.
        assert!(!compose(stale.clone(), true).start_failed);
        let working = OsState {
            repair: notice(RepairState::Working),
            ..stale.clone()
        };
        assert!(!compose(working, false).start_failed);
        let off = OsState {
            disabled_in_login_items: true,
            ..stale
        };
        let s = compose(off, false);
        assert!(!s.start_failed && !s.not_in_login_items);
        let json = serde_json::to_value(compose(OsState::default(), false)).unwrap();
        assert_eq!(
            (json["startFailed"].clone(), json["notInLoginItems"].clone()),
            (false.into(), false.into())
        );
    }

    #[test]
    fn the_repair_and_the_login_item_switch_reach_the_ui() {
        let os = OsState {
            installed: true,
            repair: notice(RepairState::Working),
            disabled_in_login_items: true,
            ..OsState::default()
        };
        let json = serde_json::to_value(compose(os, false)).unwrap();
        assert_eq!(json["disabledInLoginItems"], true);
        assert_eq!(json["repair"]["state"], "working");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn repair_exists_on_macos_only() {
        let e = request_repair(std::path::Path::new("/nowhere")).unwrap_err();
        assert_eq!(e.code, CommandErrorCode::NotImplemented);
    }

    #[test]
    fn repair_checks_soon_then_every_minute() {
        assert_eq!(super::repair_delay(0).as_secs(), 3);
        assert_eq!(super::repair_delay(1).as_secs(), 60);
        const { assert!(super::REPAIR_ATTEMPTS >= 5) };
    }
}

#[cfg(test)]
mod engine_start_tests {
    use super::*;

    #[test]
    fn an_elevated_app_never_starts_the_users_engine() {
        assert_eq!(engine_start_decision(true, false), EngineStart::Running);
        assert_eq!(
            engine_start_decision(true, true),
            EngineStart::Running,
            "an engine that already runs is left alone"
        );
        assert_eq!(engine_start_decision(false, false), EngineStart::Start);
        assert_eq!(engine_start_decision(false, true), EngineStart::SkipElevated);
        assert!(ELEVATED_SKIP_NOTICE.contains("administrator") && ELEVATED_SKIP_NOTICE.contains("normally"));
    }

    /// On the Windows test VM, run elevated: the app does not start the engine, says why, and no engine answers afterwards.
    #[cfg(windows)]
    #[test]
    #[ignore = "VM: run elevated (PITCREW_TEST_EXPECT=elevated) on the Windows test VM"]
    fn an_elevated_app_leaves_the_user_engine_not_running_and_says_why() {
        use crate::service_client::{ServiceClient, UserEngineClient};
        assert_eq!(
            std::env::var("PITCREW_TEST_EXPECT").as_deref(),
            Ok("elevated"),
            "this row runs elevated"
        );
        assert!(app_is_elevated(), "the VM run is not elevated: the row proves nothing");
        let err = start_user_engine().unwrap_err();
        assert_eq!(err.code, CommandErrorCode::PermissionDenied);
        assert!(err.message.contains("administrator"), "{}", err.message);
        assert!(
            UserEngineClient.request(pitcrew_core::protocol::Request::Status).is_err(),
            "an engine is running after an elevated app looked"
        );
    }
}
