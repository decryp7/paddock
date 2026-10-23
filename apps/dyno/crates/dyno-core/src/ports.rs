//! The core's ports to the operating system. `dyno-platform` implements them per OS (Windows, macOS) and as fakes
//! for tests; the agent depends on these traits only (ADR 0001).
//!
//! Contract for every implementation: no panics; anything the OS refuses or lacks is a [`PlatformError`] or an
//! unavailable [`crate::metrics::Reading`] with a reason the user can read; calls that may block (external tools,
//! WMI) are bounded by a timeout. An action on a [`ProcessRef`] whose start time no longer matches the live process
//! fails with [`PlatformError::NotFound`] (the pid was reused).

use crate::facts::SystemFacts;
use crate::helper::{HelperAction, TuningHost};
use crate::metrics::SystemSample;
use crate::process::{Locker, ProcessAction, ProcessDetails, ProcessRef, ProcessSample};
use crate::startup::{StartupEntry, StartupState};
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Why a platform call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlatformError {
    /// The process or file is gone (or the pid was reused).
    #[error("{0}")]
    NotFound(String),
    /// The OS refused; the text says what would help (e.g. "runs as another user: needs the helper").
    #[error("{0}")]
    AccessDenied(String),
    /// Not possible on this OS or version.
    #[error("{0}")]
    Unsupported(String),
    /// Took longer than allowed.
    #[error("{0}")]
    Timeout(String),
    /// Anything else, with the OS's message.
    #[error("{0}")]
    Failed(String),
}

/// The machine's sockets: listening ports and connections with their owning process (observe-only).
pub trait NetworkSockets: Send + Sync {
    /// Every socket the OS shows to this user. A socket whose owner the OS will not name has `pid: None`; a refused read
    /// is an error with what would help.
    fn sockets(&self) -> Result<Vec<crate::network::SocketRow>, PlatformError>;
}

/// The network interfaces and the Wi-Fi network joined, for the timeline's network change events (WP-35).
pub trait NetworkLinks: Send + Sync {
    /// One cheap look. Where the OS hides the Wi-Fi name, `ssid` is `None`; a refused read is an error with what would help.
    fn links(&self) -> Result<crate::netchange::NetState, PlatformError>;
}

/// Reverse DNS through the OS resolver (so the user's resolver and hosts file apply). Blocking: the agent calls it off
/// the request thread with a deadline. Observe-only, and only for addresses the user asked about.
pub trait ReverseDns: Send + Sync {
    /// The name of `ip`, `Ok(None)` when it has none, an error when the resolver failed.
    fn lookup(&self, ip: std::net::IpAddr) -> Result<Option<String>, PlatformError>;
}

/// Cumulative network bytes per process, where the OS makes that cheap (macOS `nettop`). Other systems answer
/// `PlatformError::Unsupported` with the reason.
pub trait ProcessTraffic: Send + Sync {
    /// One read of every process's counters.
    fn counters(&self) -> Result<Vec<crate::network_traffic::TrafficCounter>, PlatformError>;
}

/// The OS's performance counters (WP-12): what exists and what they read now. Observe-only. Reading is bounded by a
/// timeout inside the implementation; a counter that disappeared is an error for that counter only.
pub trait CounterSource: Send + Sync {
    /// Everything the OS offers, as paths are written (`\Object(Instance)\Counter`). The core bounds the result.
    fn catalog(&self) -> Result<crate::counters::CounterCatalog, PlatformError>;
    /// One raw reading per path, in the same order. A path the OS no longer has fails alone, with the reason.
    fn read(&self, paths: &[crate::counters::CounterPath]) -> Vec<crate::counters::RawCounter>;
}

/// System-wide sensors. Called once per tick from the sampling thread.
pub trait SystemSensors: Send {
    /// Measures everything at `now`. Rates (CPU, disk, network) cover the time since the previous call.
    fn sample(&mut self, now: Timestamp) -> SystemSample;
}

/// The process table.
pub trait ProcessSource: Send {
    /// Every process visible to this user, with CPU and disk rates since the previous call.
    fn processes(&mut self) -> Vec<ProcessSample>;
    /// The details of one process (slow: command line, modules, connections, signature).
    fn details(&mut self, target: ProcessRef) -> Result<ProcessDetails, PlatformError>;
}

/// Acting on processes. Every method checks [`ProcessRef::started`] first.
pub trait ProcessControl: Send {
    /// Carries out an action (already validated by the core and checked against the protected list).
    fn apply(&mut self, target: ProcessRef, action: &ProcessAction) -> Result<(), PlatformError>;
    /// Releases everything Dyno applied to processes that is not undone by process exit (CPU-cap jobs, duty
    /// cycles, suspended processes Dyno suspended). Called when the agent stops; must be idempotent.
    fn release_all(&mut self);
}

/// "What is locking this file?"
pub trait FileLocks: Send {
    /// The processes that hold `path` open (Restart Manager on Windows, `lsof` on macOS).
    fn lockers(&mut self, path: &Path) -> Result<Vec<Locker>, PlatformError>;
}

/// The startup manager's view of the OS.
pub trait StartupItems: Send {
    /// Everything that starts at login (impact is filled in by the agent from its measurements).
    fn list(&mut self) -> Result<Vec<StartupEntry>, PlatformError>;
    /// Enables or disables an entry that needs no administrator rights ([`crate::startup::StartupLocation::needs_admin`]
    /// is false). Others go through the privileged helper.
    fn set_state(&mut self, entry: &StartupEntry, state: StartupState) -> Result<(), PlatformError>;
}

/// Slow-changing facts (disk health, pending updates, power plan…). Called every few minutes off the sampling path.
pub trait FactsSource: Send {
    /// Gathers the facts; each one that fails is an unavailable reading, never an error for the whole.
    fn facts(&mut self) -> SystemFacts;
}

/// Reads and changes the few system settings tuning is allowed to touch. The set of changes is closed and typed
/// ([`HelperAction`]); an implementation never runs text it was handed. Settings that need administrator rights fail
/// with [`PlatformError::AccessDenied`]; settings this system lacks with [`PlatformError::Unsupported`].
pub trait TuningSystem: Send {
    /// The operating system the settings belong to.
    fn host(&self) -> TuningHost;
    /// The value of the setting `action` would change, as a short comparable text (for example `15` minutes or
    /// `balanced`). Reading changes nothing.
    fn read(&mut self, action: &HelperAction) -> Result<String, PlatformError>;
    /// Makes the change. The caller has validated it and shown the user its preview.
    fn apply(&mut self, action: &HelperAction) -> Result<(), PlatformError>;
}

// ---- throttling and hardware life (WP-23, WP-24) ----

/// What the OS itself says about limits on the CPU's speed. Cheap enough to call every few seconds; a system that does
/// not say answers an unavailable reading with the reason.
pub trait ThrottleSignals: Send {
    /// The OS's current speed limit and its reasons.
    fn read(&mut self) -> crate::metrics::Reading<crate::hardware::OsThrottle>;
}

/// SSD wear counters (SMART / NVMe health log). Slow and possibly privileged: called every few hours off the sampling
/// path; a refusal is an unavailable reading that says what would help.
pub trait DiskWearSource: Send {
    /// One reading per SSD that reports wear.
    fn read(&mut self) -> crate::metrics::Reading<Vec<crate::hardware::DiskWear>>;
}

/// Battery capacity and cycles. Slow to read on some systems: called every few hours.
pub trait BatteryCapacitySource: Send {
    /// The battery's capacities; unavailable on machines without one.
    fn read(&mut self) -> crate::metrics::Reading<crate::hardware::BatteryCapacity>;
}

/// Whether someone is using the machine.
pub trait UserActivity: Send {
    /// Seconds since the last keyboard or mouse input, when the OS tells.
    fn idle_secs(&mut self) -> Option<u64>;
    /// The process that owns the foreground window, when the OS tells (rules never throttle it).
    fn foreground_pid(&mut self) -> Option<crate::process::Pid>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_show_their_message() {
        assert_eq!(
            PlatformError::AccessDenied("needs the helper".into()).to_string(),
            "needs the helper"
        );
        let j = serde_json::to_value(PlatformError::NotFound("gone".into())).unwrap();
        assert_eq!(j["notFound"], "gone");
    }
}
