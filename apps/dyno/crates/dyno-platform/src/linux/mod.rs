//! Linux adapters (work in progress): for now the sysinfo sensors and process table with the shared Unix control.
use crate::processes::{NoDetails, SysinfoProcesses};
use crate::sensors::{NoProbe, SysinfoSensors};
use crate::Platform;
use dyno_core::ports::PlatformError;
use std::path::{Path, PathBuf};

/// The ports for Linux.
pub fn platform(_state_file: PathBuf) -> Platform {
    let f = crate::fake::Fake::new().platform();
    Platform {
        sensors: Box::new(SysinfoSensors::new(Box::new(NoProbe))),
        processes: Box::new(SysinfoProcesses::new(Box::new(NoDetails))),
        ..f
    }
}

/// Opens the folder containing `path` with `xdg-open`.
pub fn reveal(path: &Path) -> Result<(), PlatformError> {
    let dir = path.parent().unwrap_or(path);
    let mut cmd = crate::process::command("xdg-open");
    cmd.arg(dir);
    crate::process::run_with_timeout(cmd, std::time::Duration::from_secs(10)).map(|_| ())
}
