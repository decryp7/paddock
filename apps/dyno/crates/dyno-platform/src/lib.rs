//! Dyno's OS adapters behind `dyno_core::ports`: sysinfo-based sensors and process table for every OS, plus Windows
//! (performance counters, Win32 process control, Restart Manager, registry, WMI) and macOS (libc signals, `lsof`,
//! launch agents, `codesign`) specifics, and in-memory [`fake`]s for tests. [`platform`] wires the ports for the
//! current OS.
//!
//! Unsafe code is allowed only in the small OS modules that call the OS (`windows`, `macos`), each unsafe block with
//! a `// SAFETY:` comment.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod access;
pub mod battery;
pub mod btm_reader;
pub mod counters;
pub mod dev_folders;
pub mod extras;
pub mod fake;
pub mod fileid;
pub mod firewall_readers;
pub mod hardware;
pub mod helper_sweep;
pub mod inventory;
pub mod issues;
pub mod links;
pub mod network;
pub mod nt;
pub mod parse;
pub mod priority;
pub mod process;
pub mod processes;
pub mod readability;
pub mod runner;
pub mod sensors;
pub mod smart_reader;
pub mod tool_install;
pub mod toolpath;
pub mod wakes;
pub mod windows_firewall;
#[cfg(unix)]
#[allow(unsafe_code)]
pub mod unix_proc;
pub mod volumes;
pub mod zone;

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

use dyno_core::ports::{FactsSource, FileLocks, PlatformError, ProcessSource, StartupItems, SystemSensors, UserActivity};
use std::path::Path;

/// One implementation of every port.
pub struct Platform {
    /// System-wide sensors.
    pub sensors: Box<dyn SystemSensors>,
    /// The process table.
    pub processes: Box<dyn ProcessSource>,
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
    /// The network interfaces and the Wi-Fi network joined (network change events).
    pub links: Box<dyn dyno_core::ports::NetworkLinks>,
    /// The user's local time zone (local days, quiet hours, maintenance windows).
    pub zone: dyno_core::time::Zone,
    /// The OS's performance counters.
    pub counters: std::sync::Arc<dyn dyno_core::ports::CounterSource>,
    /// The sockets, reverse DNS and per-process traffic.
    pub network: NetworkPorts,
    /// The throttle, SSD wear and battery readers.
    pub hardware: HardwareReaders,
}

/// The network ports of a [`Platform`]: what the Network page reads.
pub struct NetworkPorts {
    /// The machine's sockets with their owners.
    pub sockets: Box<dyn dyno_core::ports::NetworkSockets>,
    /// Reverse DNS through the OS resolver.
    pub dns: std::sync::Arc<dyn dyno_core::ports::ReverseDns>,
    /// Cumulative bytes per process.
    pub traffic: Box<dyn dyno_core::ports::ProcessTraffic>,
}

impl NetworkPorts {
    /// This machine's own readers. Only the real wiring of each OS uses it, never a test (ER-14).
    pub fn os() -> NetworkPorts {
        NetworkPorts {
            sockets: Box::new(network::OsSockets),
            dns: std::sync::Arc::new(network::OsReverseDns),
            traffic: Box::new(network::OsTraffic),
        }
    }
}

/// The hardware readers of a [`Platform`]: the throttle signals, SSD wear and battery capacity.
pub struct HardwareReaders {
    /// What the OS says about limits on the CPU's speed.
    pub signals: Box<dyn dyno_core::ports::ThrottleSignals>,
    /// SSD wear counters.
    pub wear: Box<dyn dyno_core::ports::DiskWearSource>,
    /// Battery capacity and cycles.
    pub battery: Box<dyn dyno_core::ports::BatteryCapacitySource>,
}

impl HardwareReaders {
    /// This machine's own readers. Only the real wiring of each OS uses it, never a test (ER-14).
    pub fn os() -> HardwareReaders {
        HardwareReaders {
            signals: hardware::throttle_signals(),
            wear: hardware::disk_wear(),
            battery: hardware::battery_capacity(),
        }
    }
}

/// The ports for the current OS.
pub fn platform() -> Platform {
    #[cfg(windows)]
    {
        windows::platform()
    }
    #[cfg(target_os = "macos")]
    {
        macos::platform()
    }
    #[cfg(target_os = "linux")]
    {
        linux::platform()
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        other::platform()
    }
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
    fn revealing_a_missing_path_is_not_found() {
        assert!(matches!(reveal(Path::new("/dyno/no/such/path")), Err(PlatformError::NotFound(_))));
    }
}
