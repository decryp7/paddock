//! Windows adapters: performance counters, details, Restart Manager, startup entries, facts and
//! user activity.

pub mod activity;
pub mod counters;
pub mod firewall_com;
pub mod indirect;
pub mod net;
pub mod nt_query;
pub mod pdh;
pub mod power;
pub mod topology;
pub mod traffic;
pub mod wake_timer;

use crate::processes::{NoDetails, SysinfoProcesses};
use crate::sensors::SysinfoSensors;
use crate::unsupported::NotReadHere;
use crate::Platform;
use dyno_core::ports::PlatformError;
use std::path::{Path, PathBuf};

const NO_LOCKS: &str = "Dyno does not read which programs hold a file on Windows yet.";
const NO_STARTUP: &str = "Dyno does not read Windows startup items yet.";
const NO_FACTS: &str = "Dyno does not check this on Windows yet.";

/// The ports for Windows.
pub fn platform(secrets_dir: &std::path::Path) -> Platform {
    Platform {
        // The three ports without a Windows reader yet say so; they are never a fake (ADR 0033). WP-3, WP-4 and WP-6 replace them.
        locks: Box::new(NotReadHere::new("File locks", NO_LOCKS)),
        startup: Box::new(NotReadHere::new("Startup items", NO_STARTUP)),
        facts: Box::new(NotReadHere::new("System facts", NO_FACTS)),
        sensors: Box::new(SysinfoSensors::new(Box::new(pdh::WinProbe::new()))),
        processes: Box::new(SysinfoProcesses::new(Box::new(NoDetails)).with_nt(Box::new(nt_query::NtQuery::load()))),
        activity: Box::new(activity::WinActivity),
        issues: crate::issues::os_sources(),
        links: Box::new(crate::links::OsLinks::new()),
        zone: std::sync::Arc::new(crate::zone::SystemZone),
        counters: std::sync::Arc::new(crate::counters::OsCounters),
        network: crate::NetworkPorts::os(),
        hardware: crate::HardwareReaders::os(),
        power: crate::PowerPorts::os(),
        wol: crate::wol::WolPorts::os(secrets_dir),
    }
}

/// The path without the `\?\` prefix `canonicalize` adds (Explorer does not understand it).
pub fn display_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    if let Some(unc) = s.strip_prefix(r"\?\UNC\") {
        format!(r"\{unc}")
    } else {
        s.strip_prefix(r"\?\").unwrap_or(&s).to_string()
    }
}

/// Opens Explorer with `path` selected.
pub fn reveal(path: &Path) -> Result<(), PlatformError> {
    use std::os::windows::process::CommandExt;
    let shown = display_path(path);
    if shown.contains('"') {
        return Err(PlatformError::Failed("The path contains a quote character.".into()));
    }
    let windir = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let mut cmd = crate::process::command(windir.join("explorer.exe"));
    // Explorer parses its own command line: `/select,"<path>"` must reach it verbatim.
    cmd.raw_arg(format!("/select,\"{shown}\""));
    // Explorer hands the request to the running shell and exits (often with code 1): only a spawn failure counts.
    crate::process::run_with_timeout(cmd, std::time::Duration::from_secs(10)).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_windows_platform_wires_no_fake() {
        let dir = tempfile::tempdir().unwrap();
        let wired = platform(dir.path());
        let found = crate::wiring::fakes_in(&wired.adapters());
        assert!(found.is_empty(), "the Windows platform wires test doubles: {found:?}");
    }

    #[test]
    fn display_paths_drop_the_verbatim_prefix() {
        assert_eq!(display_path(Path::new(r"\?\C:\a b\c.txt")), r"C:\a b\c.txt");
        assert_eq!(display_path(Path::new(r"\?\UNC\server\share\x")), r"\server\share\x");
        assert_eq!(display_path(Path::new(r"C:\x")), r"C:\x");
    }
}
