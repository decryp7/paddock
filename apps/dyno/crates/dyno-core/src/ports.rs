//! The core's ports to the operating system. `dyno-platform` implements them per OS (Windows, macOS) and as fakes
//! for tests; the agent depends on these traits only (ADR 0001).
//!
//! Contract for every implementation: no panics; anything the OS refuses or lacks is a [`PlatformError`] or an
//! unavailable [`crate::metrics::Reading`] with a reason the user can read; calls that may block (external tools,
//! WMI) are bounded by a timeout. An action on a [`ProcessRef`] whose start time no longer matches the live process
//! fails with [`PlatformError::NotFound`] (the pid was reused).

use crate::facts::SystemFacts;
use crate::metrics::{Reading, SystemSample};
use crate::process::{Locker, ProcessDetails, ProcessRef, ProcessSample};
use crate::startup::StartupEntry;
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

/// "What is locking this file?"
pub trait FileLocks: Send {
    /// The processes that hold `path` open (Restart Manager on Windows, `lsof` on macOS).
    fn lockers(&mut self, path: &Path) -> Result<Vec<Locker>, PlatformError>;
}

/// The startup manager's view of the OS.
pub trait StartupItems: Send {
    /// Everything that starts at login (impact is filled in by the agent from its measurements).
    fn list(&mut self) -> Result<Vec<StartupEntry>, PlatformError>;
    /// Forgets anything cached, so the next [`StartupItems::list`] reads the OS again (an explicit refresh; sources that are slow
    /// to read keep their answer between refreshes).
    fn refresh(&mut self) {}
    /// What the last [`StartupItems::list`] could not read, one sentence each, for the page to show next to a partial list.
    fn unavailable(&self) -> Vec<String> {
        Vec::new()
    }
    /// Gives the list the administrator helper's login items (macOS reads them only through it); other systems ignore it. The
    /// agent calls it once it exists, because the platform is built before it.
    fn use_login_items(&mut self, _link: Box<dyn crate::btm::LoginItemsLink>) {}
    /// Who read the login items and background tasks of the last [`StartupItems::list`] (macOS); everything else needs no helper.
    fn btm_via(&self) -> crate::read_via::ReadVia {
        crate::read_via::ReadVia::Local
    }
}

/// Slow-changing facts (disk health, pending updates, power plan…). Called every few minutes off the sampling path.
pub trait FactsSource: Send {
    /// Gathers the facts; each one that fails is an unavailable reading, never an error for the whole.
    fn facts(&mut self) -> SystemFacts;
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

/// Puts the computer to sleep, hibernates it or shuts it down, always by the OS's normal path (never forced: an app with unsaved
/// work may stop a shutdown). Tests never use a real one (ER-14): they use the fake, which records the request.
pub trait PowerActions: Send + Sync {
    /// Whether this action can be asked for here: `Available(())`, or why not (shown as is).
    fn can(&self, action: crate::power::idle::IdleAction) -> Reading<()>;
    /// Asks the OS to do it. `Ok` means the OS took the request, not that it happened.
    fn request(&self, action: crate::power::idle::IdleAction) -> Result<(), PlatformError>;
}

/// What the idle rule's exceptions read from the OS: each is a value or "cannot tell" with the reason.
#[derive(Debug, Clone, PartialEq)]
pub struct IdleSignalsReading {
    /// Whether audio is playing.
    pub audio: Reading<bool>,
    /// Whether a full-screen app or a presentation shows.
    pub full_screen: Reading<bool>,
    /// The names of the other programs that asked the computer to stay awake (Dyno's own hold is left out).
    pub stay_awake: Reading<Vec<String>>,
}

/// The exceptions of the idle rule. One call may run an external tool, so the agent reads it only when it matters.
pub trait IdleSignals: Send + Sync {
    /// Reads every signal now.
    fn read(&self) -> IdleSignalsReading;
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
