//! macOS: the service is the user's launch agent (ADR 0003).
//!
//! The app bundle registers the agent with `SMAppService.agent(plistName:)` from
//! `Contents/Library/LaunchAgents/net.decryptology.pitcrew.service.plist` (see `app/src-tauri/src/service_install.rs`);
//! the user approves it in System Settings ▸ General ▸ Login Items. For development builds outside a bundle,
//! `pitcrew-service install` writes `~/Library/LaunchAgents/<label>.plist` for this executable and loads it with
//! `launchctl bootstrap gui/<uid>`; `uninstall` boots it out and deletes the file. No administrator rights are
//! involved either way.

use crate::error::Error;
use crate::process::{quiet_command, run_with_timeout};
use pitcrew_core::service_spec::{launch_agent_plist, LaunchAgentSpec, LAUNCHD_LABEL};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// launchd starts the agent with `run`; there is no separate service entry point on macOS.
pub fn dispatch() -> Result<(), Error> {
    Err(Error::Unsupported("The `service` command (macOS uses `run` under launchd)"))
}

/// The launch agent's property list for this executable and home folder.
pub fn agent_plist(exe: &Path, home: &Path) -> String {
    let support = home.join("Library/Application Support/Pitcrew");
    let log = home.join("Library/Logs/Pitcrew/service.log");
    launch_agent_plist(&LaunchAgentSpec {
        label: LAUNCHD_LABEL.into(),
        program_arguments: vec![exe.display().to_string(), "run".into()],
        working_directory: support.display().to_string(),
        log_path: log.display().to_string(),
    })
}

fn home() -> Result<PathBuf, Error> {
    std::env::var_os("HOME").map(PathBuf::from).ok_or(Error::Os {
        what: "Finding the home folder",
        reason: "HOME is not set".into(),
    })
}

fn plist_path(home: &Path) -> PathBuf {
    home.join("Library/LaunchAgents").join(format!("{LAUNCHD_LABEL}.plist"))
}

fn domain() -> String {
    format!("gui/{}", nix::unistd::getuid().as_raw())
}

fn launchctl(args: &[&str]) -> Result<crate::process::ToolOutput, Error> {
    let mut cmd = quiet_command(Path::new("/bin/launchctl"));
    cmd.args(args);
    run_with_timeout(cmd, Duration::from_secs(30))
}

/// Registers and starts the launch agent (development builds; the app uses `SMAppService`).
pub fn install() -> Result<(), Error> {
    let home = home()?;
    let exe = std::env::current_exe().map_err(|source| Error::Io {
        path: "the service executable".into(),
        source,
    })?;
    for dir in [
        "Library/LaunchAgents",
        "Library/Logs/Pitcrew",
        "Library/Application Support/Pitcrew",
    ] {
        let d = home.join(dir);
        std::fs::create_dir_all(&d).map_err(|source| Error::Io {
            path: d.display().to_string(),
            source,
        })?;
    }
    let path = plist_path(&home);
    crate::store::write_atomic(&path, &agent_plist(&exe, &home))?;
    // Replacing a loaded agent: boot the old one out first (failure means it was not loaded).
    let _ = launchctl(&["bootout", &format!("{}/{LAUNCHD_LABEL}", domain())]);
    let out = launchctl(&["bootstrap", &domain(), &path.display().to_string()])?;
    if out.success {
        Ok(())
    } else {
        Err(Error::Os {
            what: "Loading the launch agent",
            reason: out.stderr.trim().to_string(),
        })
    }
}

/// Stops and removes the launch agent. Removing one that is not installed succeeds.
pub fn uninstall() -> Result<(), Error> {
    let home = home()?;
    let _ = launchctl(&["bootout", &format!("{}/{LAUNCHD_LABEL}", domain())]);
    match std::fs::remove_file(plist_path(&home)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::Io {
            path: plist_path(&home).display().to_string(),
            source,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_agent_runs_the_service_from_the_app_bundle() {
        let plist = agent_plist(
            Path::new("/Applications/Pitcrew.app/Contents/MacOS/pitcrew-service"),
            Path::new("/Users/ci"),
        );
        assert!(plist.contains("<string>run</string>"));
        assert!(plist.contains("/Users/ci/Library/Logs/Pitcrew/service.log"));
        assert_eq!(
            plist_path(Path::new("/Users/ci")),
            PathBuf::from("/Users/ci/Library/LaunchAgents/net.decryptology.pitcrew.service.plist")
        );
        assert!(domain().starts_with("gui/"));
    }
}
