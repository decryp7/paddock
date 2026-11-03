//! The helper on this machine (ADR 0027 WP-11): where it lives, who owns it, and the OS adapters for the prompt. Everything that
//! decides something is a pure function with its test; the adapters only run fixed programs (ER-14: tests never run them).

use crate::helper_centre::{HelperCentre, HelperInstaller};
use crate::helper_link::HelperLink;
use crate::helper_peer::PeerTransport;
use dyno_core::helper::HelperHost;
use dyno_core::helper_paths::{HelperPaths, WindowsRoots};
use dyno_core::helper_wire::HelperUser;
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
    fn install(&mut self) -> Result<(), String> {
        let owner = self.owner.clone()?;
        dyno_helper_service::turn_on(os_of(self.host), &self.bundled, &owner, &mut RealSystem::new(), elevator().as_mut())
            .map_err(|e| e.to_string())
    }

    fn uninstall(&mut self) -> Result<(), String> {
        dyno_helper_service::turn_off(os_of(self.host), &self.installed, &mut RealSystem::new(), elevator().as_mut())
            .map_err(|e| e.to_string())
    }
}

fn windows_roots() -> WindowsRoots {
    let var = |k: &str, d: &str| PathBuf::from(std::env::var_os(k).unwrap_or_else(|| d.into()));
    WindowsRoots {
        program_files: var("ProgramFiles", r"C:\Program Files"),
        program_data: var("ProgramData", r"C:\ProgramData"),
        system_root: var("SystemRoot", r"C:\Windows"),
    }
}

/// The helper centre for this machine.
pub fn centre() -> HelperCentre<PeerTransport> {
    let host = host();
    let paths = HelperPaths::for_host(host, &windows_roots());
    let agent_exe = std::env::current_exe().unwrap_or_default();
    let bundled = bundled_helper(host, &agent_exe);
    let supported = unsupported(host, &bundled, bundled.is_file(), std::env::var_os("APPIMAGE").is_some());
    let installed_path = paths.binary.clone();
    let transport = PeerTransport::new(paths.endpoint.clone(), {
        let p = installed_path.clone();
        move || p.exists()
    });
    let owner = current_owner(host, &SystemRunner);
    let installer = OsInstaller {
        host,
        bundled,
        installed: installed_path,
        owner,
    };
    HelperCentre::new(
        HelperLink::new(transport, Arc::new(SystemClock)),
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
                t.program == "whoami" || t.program == "/usr/bin/id",
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
