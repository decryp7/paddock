//! What the app shows for a plugin.

use std::time::Duration;

/// The state of one plugin, for the Plugins view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Installed and fine, not started yet (plugins start on first use).
    Stopped,
    /// Its process is starting.
    Starting,
    /// Its process is up and answered `Describe`.
    Running,
    /// Its process ended unexpectedly; it starts again on the next use, after the back-off.
    Restarting {
        /// The failure that ended it.
        why: String,
        /// How long the next start waits after the failure (the back-off).
        delay: Duration,
    },
    /// Too many failures in a short time: the app does not call it until the user re-enables it.
    Quarantined {
        /// The last failure.
        why: String,
    },
    /// Cannot be served, and the text says who must update or what to fix.
    Refused(String),
    /// Switched off by the user.
    Disabled,
}

impl Status {
    /// A stable code for the UI (`stopped`, `starting`, `running`, `restarting`, `quarantined`, `refused`, `disabled`).
    pub fn code(&self) -> &'static str {
        match self {
            Status::Stopped => "stopped",
            Status::Starting => "starting",
            Status::Running => "running",
            Status::Restarting { .. } => "restarting",
            Status::Quarantined { .. } => "quarantined",
            Status::Refused(_) => "refused",
            Status::Disabled => "disabled",
        }
    }

    /// The text the UI shows next to the status, when there is one.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Status::Restarting { why, .. } | Status::Quarantined { why } | Status::Refused(why) => Some(why),
            _ => None,
        }
    }
}
