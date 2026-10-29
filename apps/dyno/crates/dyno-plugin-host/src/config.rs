//! The host's settings, with Dyno's defaults.

use std::path::PathBuf;
use std::time::Duration;

/// How long each kind of call may take before the plugin counts as hung and is killed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    /// From starting the process to its connecting (5 s).
    pub start: Duration,
    /// `Describe` (2 s).
    pub describe: Duration,
    /// `Sensors` (2 s).
    pub sensors: Duration,
    /// How long a stopped plugin may take to exit before it is killed (1 s).
    pub stop_grace: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts {
            start: Duration::from_secs(5),
            describe: Duration::from_secs(2),
            sensors: Duration::from_secs(2),
            stop_grace: Duration::from_secs(1),
        }
    }
}

/// Restart and quarantine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// This many failures within `window` quarantine the plugin (3).
    pub max_failures: usize,
    /// The window (10 minutes).
    pub window: Duration,
    /// The wait before the first restart; it doubles after each failure in a row (1 s).
    pub backoff_initial: Duration,
    /// The longest wait (60 s).
    pub backoff_max: Duration,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            max_failures: 3,
            window: Duration::from_secs(10 * 60),
            backoff_initial: Duration::from_secs(1),
            backoff_max: Duration::from_secs(60),
        }
    }
}

/// Everything the host needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostConfig {
    /// `<data dir>/plugins`.
    pub plugins_dir: PathBuf,
    /// The cap on each plugin's committed memory (Windows; 256 MiB). `None`: no cap.
    pub memory_limit: Option<u64>,
    /// Call timeouts.
    pub timeouts: Timeouts,
    /// Restart and quarantine.
    pub policy: Policy,
    /// Pass plugins' stderr through (debugging); discarded by default.
    pub inherit_stderr: bool,
}

impl HostConfig {
    /// Dyno's defaults for the plugins in `plugins_dir`.
    pub fn new(plugins_dir: impl Into<PathBuf>) -> Self {
        HostConfig {
            plugins_dir: plugins_dir.into(),
            memory_limit: Some(256 * 1024 * 1024),
            timeouts: Timeouts::default(),
            policy: Policy::default(),
            inherit_stderr: false,
        }
    }
}
