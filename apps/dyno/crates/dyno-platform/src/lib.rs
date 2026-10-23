//! Dyno's OS adapters behind `dyno_core::ports`: sysinfo-based sensors and process table for every OS, plus Windows
//! (performance counters, Win32 process control, Restart Manager, registry, WMI) and macOS (libc signals, `lsof`,
//! launch agents, `codesign`) specifics, and in-memory [`fake`]s for tests. [`platform`] wires the ports for the
//! current OS.
//!
//! Unsafe code is allowed only in the small OS modules that call the OS (`windows`, `macos`), each unsafe block with
//! a `// SAFETY:` comment.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod battery;
pub mod control_state;
pub mod counters;
pub mod dev_folders;
pub mod elevate;
pub mod fake;
pub mod hardware;
pub mod inventory;
pub mod issues;
pub mod network;
pub mod parse;
pub mod process;
pub mod processes;
pub mod sensors;
pub mod tuning;
pub mod tuning_admin;
#[cfg(unix)]
#[allow(unsafe_code)]
pub mod unix_control;

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
pub mod linux;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub mod macos;
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub mod other;
#[cfg(windows)]
#[allow(unsafe_code)]
pub mod windows;

use dyno_core::ports::{FactsSource, FileLocks, PlatformError, ProcessControl, ProcessSource, StartupItems, SystemSensors, UserActivity};
use std::path::{Path, PathBuf};

/// One implementation of every port.
pub struct Platform {
    /// System-wide sensors.
    pub sensors: Box<dyn SystemSensors>,
    /// The process table.
    pub processes: Box<dyn ProcessSource>,
    /// Acting on processes.
    pub control: Box<dyn ProcessControl>,
    /// "What is locking this file?"
    pub locks: Box<dyn FileLocks>,
    /// Startup entries.
    pub startup: Box<dyn StartupItems>,
    /// Slow-changing facts.
    pub facts: Box<dyn FactsSource>,
    /// User activity.
    pub activity: Box<dyn UserActivity>,
    /// The OS problem logs (crashes, hangs, failing services). Empty where the OS has none Dyno can read.
    pub issues: Vec<Box<dyn dyno_core::issues::IssueSource>>,
}

/// The ports for the current OS, with process-control state kept in [`default_state_file`].
pub fn platform() -> Platform {
    platform_with(default_state_file())
}

/// The ports for the current OS, keeping what process control applied (CPU caps, suspensions) in `state_file` so a
/// later `release_all` can undo what a crashed run left behind.
pub fn platform_with(state_file: PathBuf) -> Platform {
    #[cfg(windows)]
    {
        windows::platform(state_file)
    }
    #[cfg(target_os = "macos")]
    {
        macos::platform(state_file)
    }
    #[cfg(target_os = "linux")]
    {
        linux::platform(state_file)
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        other::platform(state_file)
    }
}

/// Where process-control state is kept by default: `%LOCALAPPDATA%\Dyno` on Windows, `~/Library/Application
/// Support/Dyno` on macOS, `$XDG_STATE_HOME/dyno` (or `~/.local/state/dyno`) elsewhere; the temp folder when none is
/// known.
pub fn default_state_file() -> PathBuf {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let dir = if cfg!(windows) {
        env("LOCALAPPDATA").map(|d| d.join("Dyno"))
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library").join("Application Support").join("Dyno"))
    } else {
        env("XDG_STATE_HOME")
            .map(|d| d.join("dyno"))
            .or_else(|| env("HOME").map(|h| h.join(".local").join("state").join("dyno")))
    };
    dir.unwrap_or_else(|| std::env::temp_dir().join("Dyno"))
        .join("process-control.json")
}

/// Shows `path` selected in the file manager (Explorer on Windows, Finder on macOS).
pub fn reveal(path: &Path) -> Result<(), PlatformError> {
    let path = path
        .canonicalize()
        .map_err(|_| PlatformError::NotFound(format!("{} no longer exists.", path.display())))?;
    #[cfg(windows)]
    {
        windows::reveal(&path)
    }
    #[cfg(target_os = "macos")]
    {
        macos::reveal(&path)
    }
    #[cfg(target_os = "linux")]
    {
        linux::reveal(&path)
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        other::reveal(&path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_file_is_named() {
        assert!(default_state_file().ends_with("process-control.json"));
    }

    #[test]
    fn revealing_a_missing_path_is_not_found() {
        assert!(matches!(reveal(Path::new("/dyno/no/such/path")), Err(PlatformError::NotFound(_))));
    }
}
