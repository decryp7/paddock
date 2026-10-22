//! The core's ports to the operating system. `dyno-platform` implements them per OS (Windows, macOS) and as fakes
//! for tests; the agent depends on these traits only (ADR 0001).
//!
//! Contract for every implementation: no panics; anything the OS refuses or lacks is a [`PlatformError`] or an
//! unavailable [`crate::metrics::Reading`] with a reason the user can read; calls that may block (external tools,
//! WMI) are bounded by a timeout. An action on a [`ProcessRef`] whose start time no longer matches the live process
//! fails with [`PlatformError::NotFound`] (the pid was reused).

use crate::facts::SystemFacts;
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
