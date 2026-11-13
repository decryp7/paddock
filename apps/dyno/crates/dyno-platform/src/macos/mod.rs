//! macOS adapters: memory pressure and GPU load for the sensors, volume tidying (APFS system volumes), `libproc` for open files, mapped libraries and "what is locking this file", the kernel socket lists for connections,
//! `codesign` for signers, launch agents for the startup manager, `sw_vers` /
//! `pmset` facts and user activity through CoreGraphics.

pub mod activity;
pub mod background;
pub mod details;
pub mod facts;
pub mod iokit_disk;
pub mod libproc;
pub mod locks;
pub mod pcblist;
pub mod probe;
pub mod procinfo;
pub mod startup;
pub mod volumes;

use crate::processes::SysinfoProcesses;
use crate::sensors::SysinfoSensors;
use crate::Platform;
use dyno_core::ports::PlatformError;
use std::path::Path;
use std::time::Duration;

/// The ports for macOS.
pub fn platform(secrets_dir: &std::path::Path) -> Platform {
    Platform {
        sensors: Box::new(SysinfoSensors::new(Box::new(probe::MacProbe::new()))),
        processes: Box::new(SysinfoProcesses::new(Box::new(details::MacDetails::new()))),
        locks: Box::new(locks::LsofLocks),
        startup: Box::new(startup::LaunchAgents::for_current_user()),
        facts: Box::new(facts::MacFacts::new()),
        activity: Box::new(activity::MacActivity::new()),
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

/// Shows `path` selected in Finder (`open -R`).
pub fn reveal(path: &Path) -> Result<(), PlatformError> {
    let mut cmd = crate::process::command("/usr/bin/open");
    cmd.arg("-R").arg(path);
    crate::process::run_with_timeout(cmd, Duration::from_secs(10)).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_macos_platform_wires_no_fake() {
        let dir = paddock_testkit::private_tempdir().unwrap();
        let found = crate::wiring::fakes_in(&platform(dir.path()).adapters());
        assert!(found.is_empty(), "the macOS platform wires test doubles: {found:?}");
    }
}
