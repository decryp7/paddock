//! Other OSes (Linux for now): the sysinfo-based sensors and process table; the rest says it isn't supported yet.

use crate::processes::{NoDetails, SysinfoProcesses};
use crate::sensors::{NoProbe, SysinfoSensors};
use crate::Platform;
use dyno_core::facts::SystemFacts;
use dyno_core::ports::{FactsSource, FileLocks, PlatformError, ProcessControl, StartupItems, UserActivity};
use dyno_core::process::{Locker, Pid, ProcessAction, ProcessRef};
use dyno_core::startup::{StartupEntry, StartupState};
use std::path::{Path, PathBuf};

const NOT_YET: &str = "Not supported on this OS yet.";

struct Unsupported;

impl ProcessControl for Unsupported {
    fn apply(&mut self, _target: ProcessRef, _action: &ProcessAction) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported(NOT_YET.into()))
    }
    fn release_all(&mut self) {}
}

impl FileLocks for Unsupported {
    fn lockers(&mut self, _path: &Path) -> Result<Vec<Locker>, PlatformError> {
        Err(PlatformError::Unsupported(NOT_YET.into()))
    }
}

impl StartupItems for Unsupported {
    fn list(&mut self) -> Result<Vec<StartupEntry>, PlatformError> {
        Err(PlatformError::Unsupported(NOT_YET.into()))
    }
}

impl FactsSource for Unsupported {
    fn facts(&mut self) -> SystemFacts {
        SystemFacts::unknown(sysinfo::System::long_os_version().unwrap_or_else(|| "Unknown OS".into()))
    }
}

impl UserActivity for Unsupported {
    fn idle_secs(&mut self) -> Option<u64> {
        None
    }
    fn foreground_pid(&mut self) -> Option<Pid> {
        None
    }
}

/// The ports for this OS.
pub fn platform(_state_file: PathBuf) -> Platform {
    Platform {
        sensors: Box::new(SysinfoSensors::new(Box::new(NoProbe))),
        processes: Box::new(SysinfoProcesses::new(Box::new(NoDetails))),
        control: Box::new(Unsupported),
        locks: Box::new(Unsupported),
        startup: Box::new(Unsupported),
        facts: Box::new(Unsupported),
        activity: Box::new(Unsupported),
        issues: Vec::new(),
        links: Box::new(crate::links::OsLinks::new()),
    }
}

/// Opens the folder containing `path` with `xdg-open`.
pub fn reveal(path: &Path) -> Result<(), PlatformError> {
    let dir = path.parent().unwrap_or(path);
    let mut cmd = crate::process::command("xdg-open");
    cmd.arg(dir);
    crate::process::run_with_timeout(cmd, std::time::Duration::from_secs(10)).map(|_| ())
}
