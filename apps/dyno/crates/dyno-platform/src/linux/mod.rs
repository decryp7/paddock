//! Linux adapters: sensors beyond sysinfo (PSI, diskstats, hwmon, GPU), process details from `/proc`, file locks,
//! autostart and systemd user units, facts, user activity, and process control on top of the shared Unix control.
//!
//! Everything reads `/proc` and `/sys` through [`paths::HostPaths`] and runs tools through [`tools::Tools`], so the
//! adapters are tested against temporary trees and scripted tools, and the real thing in a Linux container
//! (`scripts/linux-container.sh`).

mod activity;
mod details;
mod facts;
mod gpu;
mod hwmon;
mod locks;
mod os_control;
mod packages;
pub mod paths;
mod sensors;
mod startup;
mod sys;
mod tools;
pub mod traffic;

use crate::processes::SysinfoProcesses;
use crate::sensors::SysinfoSensors;
use crate::unix_control::UnixControl;
use crate::Platform;
use dyno_core::ports::PlatformError;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// How long "what is locking this file?" may scan.
const LOCK_SCAN_BUDGET: Duration = Duration::from_secs(5);

/// The ports for Linux.
pub fn platform(state_file: PathBuf) -> Platform {
    let tools: Arc<dyn tools::Tools> = Arc::new(tools::SystemTools);
    let paths = paths::HostPaths::host();
    Platform {
        sensors: Box::new(SysinfoSensors::new(Box::new(sensors::LinuxProbe::new(
            paths.clone(),
            tools.clone(),
        )))),
        processes: Box::new(SysinfoProcesses::new(Box::new(details::LinuxDetails::new(
            paths.clone(),
            details::LinuxDetails::default_autostart_dirs(),
            tools.clone(),
        )))),
        control: Box::new(UnixControl::new(state_file, Box::new(os_control::LinuxControl::new(paths.clone())))),
        locks: Box::new(locks::LinuxLocks::new(paths.clone(), LOCK_SCAN_BUDGET)),
        startup: Box::new(startup::LinuxStartup::for_this_user(tools.clone())),
        facts: Box::new(facts::LinuxFacts::new(paths, facts::FactPaths::host(), tools.clone())),
        activity: Box::new(activity::LinuxActivity::from_env(tools)),
    }
}

/// Opens the folder containing `path` with `xdg-open`.
pub fn reveal(path: &Path) -> Result<(), PlatformError> {
    let dir = path.parent().unwrap_or(path);
    let mut cmd = crate::process::command("xdg-open");
    cmd.arg(dir);
    crate::process::run_with_timeout(cmd, Duration::from_secs(10)).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::time::Timestamp;

    #[test]
    fn the_linux_platform_runs_end_to_end_on_this_machine() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = platform(dir.path().join("state.json"));
        let _ = p.sensors.sample(Timestamp(1));
        let s = p.sensors.sample(Timestamp(2));
        assert!(s.memory.total > 0);
        assert!(s.network.iter().all(|n| n.name != "lo"));
        let procs = p.processes.processes();
        let me = procs.iter().find(|x| x.pid == std::process::id()).expect("own process listed");
        let d = p.processes.details(me.reference()).unwrap();
        assert!(d.priority.is_some() && d.affinity.is_some() && d.threads.is_some());
        assert!(matches!(d.modules, dyno_core::process::ListOrReason::Items { .. }));
        // sysinfo knows threads as processes; the list must not (a TID is never a target).
        let again = p.processes.processes();
        let tids: Vec<u32> = std::fs::read_dir("/proc/self/task")
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.parse().ok())
            .collect();
        assert!(tids.len() > 1, "the test runner has threads");
        assert!(
            again.iter().all(|x| x.pid == std::process::id() || !tids.contains(&x.pid)),
            "a thread was listed as a process"
        );
        let tid = *tids.iter().find(|t| **t != std::process::id()).unwrap();
        let as_thread = dyno_core::process::ProcessRef {
            pid: tid,
            started: me.started,
        };
        assert!(p.processes.details(as_thread).is_err(), "details of a thread id must be refused");
        let facts = p.facts.facts();
        assert!(!facts.os.is_empty());
        let _ = p.activity.idle_secs();
        let _ = p.startup.list();
        p.control.release_all();
    }
}
