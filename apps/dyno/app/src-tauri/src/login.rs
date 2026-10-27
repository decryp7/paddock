//! Starting at login: the one Settings switch `launchAtLogin` means "Dyno opens at login (in the tray) and the agent
//! keeps measuring while the window is closed" (ADR 0010). Two registrations back it:
//!
//! - the app itself, with `tauri-plugin-autostart` (started with `--background`: tray only, no window);
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
    /// Registers it again when the OS cannot start what is registered, and on macOS once after every version change
    /// (an update). `agent_answers` asks whether the agent answers now. Returns whether it registered again.
    fn repair(&mut self, agent_answers: &dyn Fn() -> bool) -> Result<bool, String>;
}

/// The app's own start-at-login entry.
pub trait Autostart: Send {
    /// Turns it on or off.
    fn set(&mut self, on: bool) -> Result<(), String>;
}

/// Applies the `launchAtLogin` setting to both registrations. Each is attempted even if the other fails; the message
/// names what did not work and what to do.
pub fn apply_launch_at_login(on: bool, service: &mut dyn AgentService, autostart: &mut dyn Autostart) -> Result<(), String> {
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

/// The agent program next to the app (or `DYNO_AGENT_BIN`).
pub fn agent_program() -> Result<PathBuf, String> {
    let app = std::env::current_exe().map_err(|e| format!("the app's folder cannot be found: {e}"))?;
    crate::link::agent_exe(&app, std::env::var_os("DYNO_AGENT_BIN").as_deref())
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

#[cfg(not(target_os = "macos"))]
impl AgentService for CliService {
    fn installed(&mut self) -> bool {
        self.service()
            .is_ok_and(|mut s| !matches!(s.status(), dyno_agent::service::Status::NotInstalled))
    }

    fn install(&mut self) -> Result<(), String> {
        self.service()?.install()
    }

    fn uninstall(&mut self) -> Result<(), String> {
        self.service()?.uninstall()
    }

    fn repair(&mut self, _agent_answers: &dyn Fn() -> bool) -> Result<bool, String> {
        self.service()?.ensure()
    }
}

// ---------------------------------------------------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
pub use mac::SmService;

#[cfg(target_os = "macos")]
mod mac {
    //! `SMAppService` (macOS 13+). The plist is `Contents/Library/LaunchAgents/net.decryptology.dyno.agent.plist` in the
    //! bundle (`bundle.macOS.files` in tauri.macos.conf.json) with `BundleProgram` pointing at the bundled agent. The
    //! Objective-C calls are the only unsafe code; each is a plain message send on objects we own.
    #![allow(unsafe_code, unused_unsafe)]

    use super::*;
    use crate::service_repair::{
        keep_registered, parse_job, parse_print_disabled, plan, record_registered_version, registered_version, JobReport, Observed,
        RegisterError, RegistrationStatus, RepairOps, REGISTERED_VERSION_FILE,
    };
    use dyno_agent::service::LABEL;
    use objc2_foundation::NSString;
    use objc2_service_management::{SMAppService, SMAppServiceStatus};
    use std::time::{Duration, Instant};

    const PLIST: &str = "net.decryptology.dyno.agent.plist";

    /// The bundled launch agent.
    pub struct SmService;

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
            .and_then(|t| launchctl(&["print", &t]))
            .map(|r| parse_job(&r))
            .unwrap_or_default()
    }

    /// Whether launchd's override database marks the agent disabled (`launchctl print-disabled gui/<uid>`).
    fn launchd_disabled() -> Option<bool> {
        let report = launchctl(&["print-disabled", &gui_domain()?])?;
        parse_print_disabled(&report, LABEL)
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
            let registered = registered_version(&version_file());
            let observed = Observed {
                status: registration_status(),
                launchd_disabled: launchd_disabled(),
                answers: agent_answers(),
                job: job_report(),
                registered_version: registered.as_deref(),
                current_version: env!("CARGO_PKG_VERSION"),
                requested: false,
            };
            keep_registered(
                plan(&observed),
                &mut MacOps { answers: agent_answers },
                &mut record_version,
                &mut |step| crate::logging::info(&format!("agent service: {step}")),
            )
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

/// The app's own login item (`tauri-plugin-autostart`).
pub struct AppAutostart<R: tauri::Runtime>(pub tauri::AppHandle<R>);

impl<R: tauri::Runtime> Autostart for AppAutostart<R> {
    fn set(&mut self, on: bool) -> Result<(), String> {
        use tauri_plugin_autostart::ManagerExt;
        let manager = self.0.autolaunch();
        let result = if on { manager.enable() } else { manager.disable() };
        result.map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
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
        let r = apply_launch_at_login(on, &mut s, &mut a);
        let log = calls.0.lock().unwrap().clone();
        (r, log)
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
}
