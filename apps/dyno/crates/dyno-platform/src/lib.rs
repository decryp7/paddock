//! Dyno's OS adapters behind `dyno_core::ports`: sysinfo-based sensors and process table for every OS, plus Windows
//! (performance counters, Win32 process control, Restart Manager, registry, WMI) and macOS (libc signals, `lsof`,
//! launch agents, `codesign`) specifics, and, for tests only (feature `test-support`), in-memory `fake`s. [`platform`] wires the ports for the
//! current OS.
//!
//! Unsafe code is allowed only in the small OS modules that call the OS (`windows`, `macos`), each unsafe block with
//! a `// SAFETY:` comment.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod access;
pub mod battery;
pub mod btm_reader;
pub mod build_id;
pub mod counters;
pub mod dev_folders;
pub mod extras;
#[cfg(any(test, feature = "test-support"))]
pub mod fake;
pub mod fileid;
pub mod firewall_readers;
pub mod hardware;
pub mod helper_counters;
pub mod helper_extras;
#[cfg(unix)]
pub mod helper_holders;
pub mod helper_sweep;
pub mod inventory;
pub mod issues;
pub mod links;
pub mod mount_events;
pub mod network;
pub mod nt;
pub mod parse;
pub mod power_actions;
pub mod priority;
pub mod process;
pub mod processes;
pub mod readability;
pub mod runner;
pub mod sensors;
pub mod smart_reader;
pub mod smartctl_search;
pub mod system_powershell;
pub mod tool_install;
#[cfg(unix)]
#[allow(unsafe_code)]
pub mod unix_proc;
pub mod unsupported;
pub mod volumes;
pub mod wakes;
pub mod windows_firewall;
#[cfg(test)]
mod wiring;
pub mod wol;
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
    /// Keep awake and sleep/wake notifications (ADR 0032).
    pub power: PowerPorts,
    /// Wake-on-LAN: the sockets, neighbour tables, echo and this PC's wake settings (ADR 0032).
    pub wol: wol::WolPorts,
}

/// What a port shows when its trait has no `adapter()` (the family's `paddock_power` and `paddock_secrets` traits, which stay as they
/// are for the sake of a test: ADR 0033 section 2).
pub const UNNAMED_ADAPTER: &str = "(unnamed)";

impl Platform {
    /// The concrete adapter behind every port, as `(port, adapter type)` pairs, in field order. The agent logs it at start (`Platform
    /// wired`), and each OS's wiring test reads it without calling any port. It destructures `Platform` and its sub-structs by
    /// name, so a new port that is not listed here is a compile error.
    pub fn adapters(&self) -> Vec<(String, &'static str)> {
        let Platform {
            sensors,
            processes,
            locks,
            startup,
            facts,
            activity,
            issues,
            links,
            zone,
            counters,
            network,
            hardware,
            power,
            wol,
        } = self;
        let NetworkPorts { sockets, dns, traffic } = network;
        let HardwareReaders { signals, wear, battery } = hardware;
        let PowerPorts {
            os: _,
            actions,
            signals: idle,
            events: _,
        } = power;
        let wol::WolPorts {
            udp,
            interfaces,
            neighbours,
            prober,
            readiness,
            secrets: _,
            clock,
            wake_timer,
        } = wol;
        let mut list: Vec<(String, &'static str)> = vec![
            ("sensors".into(), sensors.adapter()),
            ("processes".into(), processes.adapter()),
            ("locks".into(), locks.adapter()),
            ("startup".into(), startup.adapter()),
            ("facts".into(), facts.adapter()),
            ("activity".into(), activity.adapter()),
        ];
        list.extend(issues.iter().enumerate().map(|(i, s)| (format!("issues[{i}]"), s.adapter())));
        list.extend([
            ("links".into(), links.adapter()),
            ("zone".into(), zone.adapter()),
            ("counters".into(), counters.adapter()),
            ("network.sockets".into(), sockets.adapter()),
            ("network.dns".into(), dns.adapter()),
            ("network.traffic".into(), traffic.adapter()),
            ("hardware.signals".into(), signals.adapter()),
            ("hardware.wear".into(), wear.adapter()),
            ("hardware.battery".into(), battery.adapter()),
            ("power.os".into(), UNNAMED_ADAPTER),
            ("power.actions".into(), actions.adapter()),
            ("power.signals".into(), idle.adapter()),
            ("power.events".into(), UNNAMED_ADAPTER),
            ("wol.udp".into(), udp.adapter()),
            ("wol.interfaces".into(), interfaces.adapter()),
            ("wol.neighbours".into(), neighbours.adapter()),
            ("wol.prober".into(), prober.adapter()),
            ("wol.readiness".into(), readiness.adapter()),
            ("wol.secrets".into(), UNNAMED_ADAPTER),
            ("wol.clock".into(), clock.adapter()),
            ("wol.wake_timer".into(), wake_timer.as_ref().map_or("(none)", |t| t.adapter())),
        ]);
        list
    }

    /// [`Platform::adapters`] as one line, `port=adapter, ...`, for the `Platform wired` log line (ER-27). Type names are not user content. It lists the platform as built, before the agent wraps
    /// a port (a helper-backed reader, for example), so a port may be answered by a wrapper around the adapter named here.
    pub fn wired_line(&self) -> String {
        self.adapters()
            .iter()
            .map(|(port, adapter)| format!("{port}={adapter}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The power ports of a [`Platform`]: the OS's sleep-prevention object and its sleep and wake notifications.
pub struct PowerPorts {
    /// Takes the OS hold that keeps the computer awake.
    pub os: std::sync::Arc<dyn paddock_power::PowerOs>,
    /// Sleep, hibernate and shut down by the OS's normal path.
    pub actions: std::sync::Arc<dyn dyno_core::ports::PowerActions>,
    /// The idle rule's exceptions (audio, full screen, other programs' stay-awake requests).
    pub signals: std::sync::Arc<dyn dyno_core::ports::IdleSignals>,
    /// Sleep and wake notifications.
    pub events: std::sync::Arc<dyn paddock_power::SleepWakeSource>,
}

impl PowerPorts {
    /// This machine's own adapters. Only the real wiring of each OS uses it, never a test (ER-14).
    pub fn os() -> PowerPorts {
        PowerPorts {
            os: std::sync::Arc::from(paddock_power::system_power()),
            actions: std::sync::Arc::new(power_actions::OsPowerActions),
            signals: std::sync::Arc::new(power_actions::OsIdleSignals),
            events: std::sync::Arc::from(paddock_power::system_events()),
        }
    }
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

/// The ports for the current OS. `secrets_dir` is where the macOS build keeps Wake-on-LAN passwords (see [`wol::os_store`]).
pub fn platform(secrets_dir: &std::path::Path) -> Platform {
    #[cfg(windows)]
    {
        windows::platform(secrets_dir)
    }
    #[cfg(target_os = "macos")]
    {
        macos::platform(secrets_dir)
    }
    #[cfg(target_os = "linux")]
    {
        linux::platform(secrets_dir)
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        other::platform(secrets_dir)
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
