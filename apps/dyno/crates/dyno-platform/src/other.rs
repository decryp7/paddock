//! Other OSes (Linux for now): the sysinfo-based sensors and process table; the rest says it isn't supported yet.

use crate::processes::{NoDetails, SysinfoProcesses};
use crate::sensors::{NoProbe, SysinfoSensors};
use crate::unsupported::NotReadHere;
use crate::Platform;
use dyno_core::ports::PlatformError;
use std::path::Path;

const NOT_YET: &str = "Not supported on this OS yet.";

/// The ports for this OS.
pub fn platform(secrets_dir: &std::path::Path) -> Platform {
    Platform {
        sensors: Box::new(SysinfoSensors::new(Box::new(NoProbe))),
        processes: Box::new(SysinfoProcesses::new(Box::new(NoDetails))),
        locks: Box::new(NotReadHere::new("File locks", NOT_YET)),
        startup: Box::new(NotReadHere::new("Startup items", NOT_YET)),
        facts: Box::new(NotReadHere::new("System facts", NOT_YET)),
        activity: Box::new(NotReadHere::new("User activity", NOT_YET)),
        issues: Vec::new(),
        links: Box::new(crate::links::OsLinks::new()),
        zone: std::sync::Arc::new(crate::zone::SystemZone),
        counters: std::sync::Arc::new(crate::counters::OsCounters),
        network: crate::NetworkPorts::os(),
        hardware: crate::HardwareReaders::os(),
        power: crate::PowerPorts::os(),
        wol: crate::wol::WolPorts::os(secrets_dir),
    }
}

/// Opens the folder containing `path` with `xdg-open`.
pub fn reveal(path: &Path) -> Result<(), PlatformError> {
    let dir = path.parent().unwrap_or(path);
    let mut cmd = crate::process::command("xdg-open");
    cmd.arg(dir);
    crate::process::run_with_timeout(cmd, std::time::Duration::from_secs(10)).map(|_| ())
}
