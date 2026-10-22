//! macOS: the service is the user's launch agent (ADR 0003). The property list is rendered by
//! `pitcrew_core::service_spec`; registering it (`SMAppService` from the app bundle on macOS 13+, `launchctl
//! bootstrap gui/<uid>` otherwise) is not built yet.

use crate::error::Error;
use pitcrew_core::service_spec::{launch_agent_plist, LaunchAgentSpec, LAUNCHD_LABEL};
use std::path::Path;

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

/// Registers the launch agent.
pub fn install() -> Result<(), Error> {
    Err(Error::NotImplemented("Registering the launch agent"))
}

/// Removes the launch agent.
pub fn uninstall() -> Result<(), Error> {
    Err(Error::NotImplemented("Removing the launch agent"))
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
        assert!(install().is_err());
    }
}
