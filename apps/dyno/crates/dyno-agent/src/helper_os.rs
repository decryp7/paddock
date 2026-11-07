//! The helper on this machine (ADR 0027 WP-11): where it lives, who owns it, and the OS adapters for the prompt. Everything that
//! decides something is a pure function with its test; the adapters only run fixed programs (ER-14: tests never run them).

use crate::helper_centre::{HelperCentre, HelperInstaller};
use crate::helper_link::{HelperLink, HelperTransport};
use crate::helper_peer::PeerTransport;
use dyno_core::helper::HelperHost;
use dyno_core::helper_paths::{HelperPaths, WindowsRoots};
use dyno_core::helper_wire::{HelperAction, HelperUser};
use dyno_platform::runner::{tool, SystemRunner, ToolRunner};
use paddock_env::Os;
use paddock_jobs::time::SystemClock;
use paddock_service::{Elevator, RealSystem};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// The system this build runs on.
pub fn host() -> HelperHost {
    if cfg!(windows) {
        HelperHost::Windows
    } else if cfg!(target_os = "macos") {
        HelperHost::Macos
    } else {
        HelperHost::Linux
    }
}

fn os_of(host: HelperHost) -> Os {
    match host {
        HelperHost::Windows => Os::Windows,
        HelperHost::Macos => Os::MacOs,
        HelperHost::Linux => Os::Linux,
    }
}

/// `"HOST\user","S-1-5-21-…"` (the CSV of `whoami /user /fo csv /nh`) to the account's SID.
pub fn owner_from_whoami(csv: &str) -> Option<HelperUser> {
    let sid = csv
        .lines()
        .find(|l| !l.trim().is_empty())?
        .rsplit(',')
        .next()?
        .trim()
        .trim_matches('"');
    HelperUser::parse(&format!("sid:{sid}")).ok()
}

/// `id -u`'s output to the account.
pub fn owner_from_id(text: &str) -> Option<HelperUser> {
    HelperUser::parse(&format!("uid:{}", text.trim())).ok()
}

/// The account that turns the helper on (the owner, ADR 0027 section 2), read from the OS.
pub fn current_owner(host: HelperHost, runner: &dyn ToolRunner) -> Result<HelperUser, String> {
    let out = match host {
        HelperHost::Windows => runner.run_tool(&tool("whoami", &["/user", "/fo", "csv", "/nh"]), Duration::from_secs(10)),
        _ => runner.run_tool(&tool("/usr/bin/id", &["-u"]), Duration::from_secs(10)),
    }
    .map_err(|e| e.to_string())?;
    let owner = if host == HelperHost::Windows {
        owner_from_whoami(&out.stdout)
    } else {
        owner_from_id(&out.stdout)
    };
    owner.ok_or_else(|| "Dyno could not tell which account you are, so it cannot set up the helper for you.".to_string())
}

/// Why the helper cannot exist here, if so: no bundled program next to the agent, or an AppImage.
pub fn unsupported(host: HelperHost, bundled: &Path, bundled_exists: bool, appimage: bool) -> Result<(), String> {
    if host == HelperHost::Linux && appimage {
        return Err("This AppImage has no helper. Install Dyno's .deb package to use administrator readings.".into());
    }
    if !bundled_exists {
        return Err(format!(
            "This build of Dyno has no helper next to it ({} is missing). Reinstall Dyno.",
            bundled.display()
        ));
    }
    Ok(())
}

/// Whether the helper has been turned on: its protected program is there (macOS, Windows), or, on Linux, where the package puts the
/// program in place whether or not anyone enabled it, the service is enabled (`systemctl is-enabled`, which needs no rights).
pub fn is_installed(host: HelperHost, program: &Path, runner: &dyn ToolRunner) -> bool {
    if host != HelperHost::Linux {
        return program.exists();
    }
    runner
        .run_tool(
            &tool("/usr/bin/systemctl", &["is-enabled", "dyno-helper.service"]),
            Duration::from_secs(5),
        )
        .is_ok_and(|o| o.ok)
}

/// The program the elevated `uninstall` runs. Windows cannot delete a program that is running, so the installed copy cannot remove
/// itself (measured in the Windows 11 VM: `RemoveProgram ... Access is denied`); the bundled copy removes it. Elsewhere the
/// installed copy does (the running file can be unlinked).
pub fn uninstall_program<'a>(host: HelperHost, bundled: &'a Path, installed: &'a Path) -> &'a Path {
    if host == HelperHost::Windows {
        bundled
    } else {
        installed
    }
}

/// The helper program that ships with the agent.
pub fn bundled_helper(host: HelperHost, agent_exe: &Path) -> PathBuf {
    match host {
        HelperHost::Linux if Path::new("/usr/bin/dyno-helper").is_file() => PathBuf::from("/usr/bin/dyno-helper"),
        HelperHost::Windows => agent_exe.with_file_name("dyno-helper.exe"),
        _ => agent_exe.with_file_name("dyno-helper"),
    }
}

#[cfg(windows)]
fn elevator() -> Box<dyn Elevator> {
    Box::new(paddock_service::native::RunAsElevator)
}

/// macOS and Linux elevate through `osascript` and `pkexec` run by `paddock-service`; this port is never asked there.
#[cfg(not(windows))]
fn elevator() -> Box<dyn Elevator> {
    use paddock_service::RegisterError;
    struct Unused;
    impl Elevator for Unused {
        fn run_elevated(&mut self, _: &Path, _: &[&str], _: Duration) -> Result<i32, RegisterError> {
            Err(RegisterError::Unavailable(
                "this system elevates through its own prompt program".into(),
            ))
        }
    }
    Box::new(Unused)
}

/// The real install and removal through the OS's one prompt.
struct OsInstaller {
    host: HelperHost,
    bundled: PathBuf,
    installed: PathBuf,
    owner: Result<HelperUser, String>,
}

impl HelperInstaller for OsInstaller {
    fn install(&mut self, action: HelperAction) -> Result<(), String> {
        let owner = self.owner.clone()?;
        let prompt = dyno_helper_service::macos_prompt(action);
        dyno_helper_service::turn_on_with(
            os_of(self.host),
            &self.bundled,
            &owner,
            &mut RealSystem::new(),
            elevator().as_mut(),
            prompt,
        )
        .map_err(|e| e.to_string())
    }

    fn uninstall(&mut self) -> Result<(), String> {
        let program = uninstall_program(self.host, &self.bundled, &self.installed);
        dyno_helper_service::turn_off(os_of(self.host), program, &mut RealSystem::new(), elevator().as_mut()).map_err(|e| e.to_string())
    }
}

/// The folders Windows names, from the OS and not the environment (the one implementation, shared with the helper).
fn windows_roots() -> WindowsRoots {
    dyno_helper_service::windows_roots(&paddock_service::privileged::OsKnownFolders)
}

/// Where this machine's helper keeps its files (the lifecycle file the report reads, the binary, the endpoint).
pub fn helper_paths() -> HelperPaths {
    HelperPaths::for_host(host(), &windows_roots())
}

/// The connection to this machine's helper (also what the VM test sends requests through).
pub fn transport() -> PeerTransport {
    let host = host();
    let paths = HelperPaths::for_host(host, &windows_roots());
    let installed_path = paths.binary.clone();
    PeerTransport::new(paths.endpoint.clone(), move || is_installed(host, &installed_path, &SystemRunner))
        .with_revive(Box::new(ServiceReviver::new(os_of(host), Arc::new(SystemClock))))
}

/// Starts the helper's stopped service through the Service Control Manager with the user's own token (no prompt: the service's
/// security descriptor lets interactive users start it). Other systems have no such start and report that.
pub struct ServiceReviver {
    os: Os,
    clock: Arc<dyn paddock_jobs::time::Clock>,
}

impl ServiceReviver {
    /// A reviver for `os` timed by `clock`.
    pub fn new(os: Os, clock: Arc<dyn paddock_jobs::time::Clock>) -> Self {
        ServiceReviver { os, clock }
    }
}

struct ClockPace {
    clock: Arc<dyn paddock_jobs::time::Clock>,
    began: Duration,
}

impl paddock_service::start::Pace for ClockPace {
    fn elapsed(&mut self) -> Duration {
        self.clock.monotonic().saturating_sub(self.began)
    }
    fn pause(&mut self, how_long: Duration) {
        self.clock.sleep(how_long);
    }
}

impl crate::helper_peer::Revive for ServiceReviver {
    fn revive(&self, budget: Duration, ready: &mut dyn FnMut() -> bool) -> paddock_service::start::Revival {
        let mut pace = ClockPace {
            clock: self.clock.clone(),
            began: self.clock.monotonic(),
        };
        paddock_service::start::revive(
            &mut RealSystem::new(),
            self.os,
            dyno_helper_service::SERVICE_NAME,
            budget,
            &mut pace,
            &dyno_core::helper_wire::start_exit_words,
            ready,
        )
    }
}

/// The helper centre for this machine.
pub fn centre() -> HelperCentre<Box<dyn HelperTransport>> {
    let host = host();
    let paths = HelperPaths::for_host(host, &windows_roots());
    let agent_exe = std::env::current_exe().unwrap_or_default();
    let bundled = bundled_helper(host, &agent_exe);
    let supported = unsupported(host, &bundled, bundled.is_file(), std::env::var_os("APPIMAGE").is_some());
    let installed_path = paths.binary.clone();
    // The bundled helper's build, hashed once (the agent restarts when its app is replaced, so it cannot go stale).
    let bundled_for_build = bundled.clone();
    let bundled_build: crate::helper_link::BundledBuild = Arc::new({
        let cache = std::sync::OnceLock::new();
        move || cache.get_or_init(|| crate::build_id::file_build_id(&bundled_for_build)).clone()
    });
    let transport = transport();
    let owner = current_owner(host, &SystemRunner);
    let installer = OsInstaller {
        host,
        bundled,
        installed: installed_path,
        owner,
    };
    HelperCentre::new(
        HelperLink::new(Box::new(transport) as Box<dyn HelperTransport>, Arc::new(SystemClock)).with_bundled_build(bundled_build),
        host,
        supported,
        env!("CARGO_PKG_VERSION"),
        Box::new(installer),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_platform::runner::{Tool, ToolOutput};

    #[test]
    fn the_owner_is_read_from_whoami_and_id() {
        assert_eq!(
            owner_from_whoami("\"PC\\alex\",\"S-1-5-21-1-2-3-1001\"\r\n"),
            Some(HelperUser::Sid("S-1-5-21-1-2-3-1001".into()))
        );
        assert_eq!(owner_from_whoami("garbage"), None);
        assert_eq!(owner_from_whoami(""), None);
        assert_eq!(owner_from_id("501\n"), Some(HelperUser::Uid(501)));
        assert_eq!(owner_from_id("root"), None);
    }

    struct Says(String);
    impl ToolRunner for Says {
        fn run_tool(&self, t: &Tool, _: Duration) -> Result<ToolOutput, dyno_core::ports::PlatformError> {
            assert!(
                ["whoami", "/usr/bin/id", "/usr/bin/systemctl"].contains(&t.program.as_str()),
                "only fixed programs: {}",
                t.program
            );
            Ok(ToolOutput {
                ok: true,
                stdout: self.0.clone(),
                error: String::new(),
            })
        }
    }

    #[test]
    fn the_current_owner_uses_the_tool_of_the_system() {
        assert_eq!(current_owner(HelperHost::Macos, &Says("501".into())), Ok(HelperUser::Uid(501)));
        assert!(current_owner(HelperHost::Windows, &Says("\"a\",\"S-1-5-21-9\"".into())).is_ok());
        assert!(current_owner(HelperHost::Linux, &Says("nope".into()))
            .unwrap_err()
            .contains("which account"));
    }

    #[test]
    fn on_linux_the_helper_is_installed_when_its_service_is_enabled_not_when_the_package_put_the_program_there() {
        // Measured on Ubuntu (Orb): with the program in /usr/bin and the service not enabled the card said "Not running" instead of "Off".
        let enabled = Says("enabled\n".into());
        let program = Path::new("/usr/bin/dyno-helper");
        assert!(is_installed(HelperHost::Linux, program, &enabled));
        struct Disabled;
        impl ToolRunner for Disabled {
            fn run_tool(&self, _: &Tool, _: Duration) -> Result<ToolOutput, dyno_core::ports::PlatformError> {
                Ok(ToolOutput {
                    ok: false,
                    stdout: "disabled\n".into(),
                    error: String::new(),
                })
            }
        }
        assert!(!is_installed(HelperHost::Linux, program, &Disabled));
        // Elsewhere the protected program's presence decides (a path that is not there: not installed, no tool run).
        assert!(!is_installed(
            HelperHost::Macos,
            Path::new("/definitely/not/here/dyno-helper"),
            &enabled
        ));
    }

    #[test]
    fn windows_removes_the_installed_helper_with_the_bundled_one() {
        let (b, i) = (Path::new("/app/dyno-helper"), Path::new("/lib/dyno-helper"));
        assert_eq!(uninstall_program(HelperHost::Windows, b, i), b);
        assert_eq!(uninstall_program(HelperHost::Macos, b, i), i);
        assert_eq!(uninstall_program(HelperHost::Linux, b, i), i);
    }

    #[test]
    fn unsupported_systems_say_why() {
        let p = Path::new("/x/dyno-helper");
        assert!(unsupported(HelperHost::Linux, p, true, true).unwrap_err().contains("AppImage"));
        assert!(unsupported(HelperHost::Macos, p, false, false).unwrap_err().contains("Reinstall"));
        assert!(unsupported(HelperHost::Windows, p, true, false).is_ok());
        assert_eq!(
            bundled_helper(HelperHost::Windows, Path::new(r"C:\a\dyno-agent.exe"))
                .file_name()
                .unwrap(),
            "dyno-helper.exe"
        );
    }
}
