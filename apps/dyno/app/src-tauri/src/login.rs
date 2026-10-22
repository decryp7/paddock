//! Starting at login: the one Settings switch `launchAtLogin` means "Dyno opens at login (in the tray) and the agent
//! keeps measuring while the window is closed" (ADR 0010). Two registrations back it:
//!
//! - the app itself, with `tauri-plugin-autostart` (started with `--background`: tray only, no window);
//! - the agent as a per-user service: `SMAppService` on macOS (the plist inside the bundle), a systemd user unit on
//!   Linux and the per-user `Run` entry on Windows (`dyno_agent::service`).
//!
//! The OS parts sit behind two small traits so the orchestration and the re-registration decision are unit-tested with
//! fakes; nothing in the tests touches a real launch agent, unit or registry key.

use std::path::PathBuf;

/// The agent as a per-user background service.
pub trait AgentService: Send {
    /// Whether it is registered.
    fn installed(&mut self) -> bool;
    /// Registers (and starts) it.
    fn install(&mut self) -> Result<(), String>;
    /// Stops and removes it.
    fn uninstall(&mut self) -> Result<(), String>;
    /// Registers it again when the OS cannot start what is registered (after an update). Returns whether it did.
    fn repair(&mut self, agent_answers: bool) -> Result<bool, String>;
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

/// Whether `launchctl print` of the launch agent shows a registration launchd can no longer start. macOS keeps the code
/// requirement of the binary that was approved; a rebuilt, ad-hoc signed app has another identity, so launchd refuses
/// the new agent ("needs LWCR update", spawn failed, exit 78 EX_CONFIG) until it is registered again.
pub fn stale_registration(launchctl_print: &str) -> bool {
    launchctl_print.contains("needs LWCR update")
        || launchctl_print.contains("job state = spawn failed")
        || launchctl_print.contains("last exit code = 78")
}

/// Whether a registered macOS agent must be registered again: it is enabled, does not answer, and launchd says its
/// registration is stale.
pub fn needs_reregistration(enabled: bool, agent_answers: bool, launchctl_print: Option<&str>) -> bool {
    enabled && !agent_answers && launchctl_print.is_some_and(stale_registration)
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

    fn repair(&mut self, _agent_answers: bool) -> Result<bool, String> {
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
    use objc2_foundation::NSString;
    use objc2_service_management::{SMAppService, SMAppServiceStatus};

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

    /// `launchctl print gui/<uid>/<label>`, bounded to a few seconds; `None` when it cannot be read.
    fn launchctl_print() -> Option<String> {
        use std::os::unix::fs::MetadataExt;
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        let home = std::env::var_os("HOME")?;
        let uid = std::fs::metadata(home).ok()?.uid();
        let target = format!("gui/{uid}/{}", dyno_agent::service::LABEL);
        // macOS only: no console window to hide, and the wait below is bounded.
        #[allow(clippy::disallowed_methods)]
        let mut child = Command::new("/bin/launchctl")
            .args(["print", &target])
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

    impl AgentService for SmService {
        fn installed(&mut self) -> bool {
            matches!(status(), SMAppServiceStatus::Enabled | SMAppServiceStatus::RequiresApproval)
        }

        fn install(&mut self) -> Result<(), String> {
            let s = service();
            // SAFETY: a method call on a valid object; the error is returned as a Retained NSError.
            match unsafe { s.registerAndReturnError() } {
                Ok(()) => Ok(()),
                Err(e) => {
                    if status() == SMAppServiceStatus::RequiresApproval {
                        // SAFETY: a class method without arguments.
                        unsafe { SMAppService::openSystemSettingsLoginItems() };
                        return Err(
                            "macOS needs your approval: allow Dyno in System Settings > General > Login Items (opened for you)".into(),
                        );
                    }
                    if status() == SMAppServiceStatus::NotFound {
                        return Err("run Dyno from the Applications folder (the launch agent is part of the app bundle)".into());
                    }
                    Err(e.localizedDescription().to_string())
                }
            }
        }

        fn uninstall(&mut self) -> Result<(), String> {
            if status() == SMAppServiceStatus::NotRegistered || status() == SMAppServiceStatus::NotFound {
                return Ok(());
            }
            // SAFETY: a method call on a valid object.
            unsafe { service().unregisterAndReturnError() }.map_err(|e| e.localizedDescription().to_string())
        }

        fn repair(&mut self, agent_answers: bool) -> Result<bool, String> {
            let enabled = status() == SMAppServiceStatus::Enabled;
            let report = if enabled && !agent_answers { launchctl_print() } else { None };
            if !needs_reregistration(enabled, agent_answers, report.as_deref()) {
                return Ok(false);
            }
            // Unregistering a stale job can fail when launchd already dropped it; registering again is what matters.
            let _ = self.uninstall();
            std::thread::sleep(std::time::Duration::from_secs(1));
            self.install().map(|()| true)
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
        fn repair(&mut self, _: bool) -> Result<bool, String> {
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

    #[test]
    fn it_registers_again_only_when_enabled_silent_and_stale() {
        let stale = Some("job state = spawn failed");
        assert!(needs_reregistration(true, false, stale));
        assert!(
            !needs_reregistration(false, false, stale),
            "not registered: Dyno never installs itself unasked"
        );
        assert!(!needs_reregistration(true, true, stale), "the agent answers");
        assert!(!needs_reregistration(true, false, Some("state = running")));
        assert!(!needs_reregistration(true, false, None), "launchctl could not be read");
    }
}
