//! Every failure is attributed to a plugin.

use std::fmt;

/// What kind of failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// No plugin with that id, or the plugin does not know the category or action.
    NotFound,
    /// The plugin is stopped or waiting to be restarted.
    NotRunning,
    /// The plugin is quarantined until the user clears it.
    Quarantined,
    /// The plugin does not contribute what was asked.
    Unsupported,
    /// The plugin did not answer in time and was killed.
    Timeout,
    /// The plugin crashed, panicked, exited or could not start.
    Crashed,
    /// The plugin sent something invalid (garbage, a wrong reply, a manifest that disagrees with its plugin.toml).
    Protocol,
    /// The plugin's folder or manifest is unusable.
    Invalid,
    /// The plugin's own, expected failure (not counted towards quarantine).
    Failed,
}

/// A failure of one plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostError {
    /// The plugin's id.
    pub plugin: String,
    /// What kind of failure.
    pub kind: ErrorKind,
    /// What happened, for people.
    pub message: String,
}

impl HostError {
    /// An error of `plugin`.
    pub fn new(plugin: impl Into<String>, kind: ErrorKind, message: impl Into<String>) -> Self {
        HostError {
            plugin: plugin.into(),
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "plugin {}: {}", self.plugin, self.message)
    }
}

impl std::error::Error for HostError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_name_their_plugin() {
        let e = HostError::new("x", ErrorKind::Timeout, "too slow");
        assert_eq!(e.to_string(), "plugin x: too slow");
    }
}
