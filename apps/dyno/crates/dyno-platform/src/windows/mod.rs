//! Windows adapters: performance counters, process control, details, Restart Manager, startup entries, facts and
//! user activity.

pub mod activity;
pub mod control;
#[cfg(test)]
mod control_tests;
pub mod handle;
pub mod net;
pub mod pdh;
pub mod topology;

use crate::processes::{NoDetails, SysinfoProcesses};
use crate::sensors::SysinfoSensors;
use crate::Platform;
use dyno_core::ports::PlatformError;
use std::path::{Path, PathBuf};

/// The ports for Windows.
pub fn platform(state_file: PathBuf) -> Platform {
    let fake = crate::fake::Fake::new();
    let f = fake.platform();
    Platform {
        sensors: Box::new(SysinfoSensors::new(Box::new(pdh::WinProbe::new()))),
        control: Box::new(control::WinControl::new(state_file)),
        processes: Box::new(SysinfoProcesses::new(Box::new(NoDetails))),
        activity: Box::new(activity::WinActivity),
        ..f
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
    fn display_paths_drop_the_verbatim_prefix() {
        assert_eq!(display_path(Path::new(r"\?\C:\a b\c.txt")), r"C:\a b\c.txt");
        assert_eq!(display_path(Path::new(r"\?\UNC\server\share\x")), r"\server\share\x");
        assert_eq!(display_path(Path::new(r"C:\x")), r"C:\x");
    }
}
