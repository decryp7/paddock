//! macOS adapters: memory pressure and GPU load for the sensors, volume tidying (APFS system volumes), process
//! control through the shared Unix control (efficiency mode = background QoS), `lsof` for open files, connections
//! and "what is locking this file", `codesign` for signers, launch agents for the startup manager, `sw_vers` /
//! `pmset` facts and user activity through CoreGraphics.

pub mod activity;
pub mod control;
pub mod details;
pub mod facts;
pub mod locks;
pub mod probe;
pub mod startup;
pub mod volumes;

use crate::processes::SysinfoProcesses;
use crate::sensors::SysinfoSensors;
use crate::unix_control::UnixControl;
use crate::Platform;
use dyno_core::ports::PlatformError;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The ports for macOS.
pub fn platform(state_file: PathBuf) -> Platform {
    Platform {
        sensors: Box::new(SysinfoSensors::new(Box::new(probe::MacProbe::new()))),
        processes: Box::new(SysinfoProcesses::new(Box::new(details::MacDetails::new()))),
        control: Box::new(UnixControl::new(state_file, Box::new(control::MacOsControl))),
        locks: Box::new(locks::LsofLocks),
        startup: Box::new(startup::LaunchAgents::for_current_user()),
        facts: Box::new(facts::MacFacts::new()),
        activity: Box::new(activity::MacActivity::new()),
        issues: crate::issues::os_sources(),
        links: Box::new(crate::links::OsLinks::new()),
    }
}

/// Shows `path` selected in Finder (`open -R`).
pub fn reveal(path: &Path) -> Result<(), PlatformError> {
    let mut cmd = crate::process::command("/usr/bin/open");
    cmd.arg("-R").arg(path);
    crate::process::run_with_timeout(cmd, Duration::from_secs(10)).map(|_| ())
}
