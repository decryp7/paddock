//! Who a plugin is and what it contributes.

use serde::{Deserialize, Serialize};

/// The version of this contract. The host refuses a plugin built against another one, with a message saying which
/// side to update. Adding a variant at the end of a protocol enum does not change it.
pub const API_VERSION: u32 = 1;

/// What a plugin can contribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Contribution {
    /// Extra sensor readings.
    Sensors,
    /// Cleanup categories.
    Cleanup,
    /// Actions rules can trigger.
    Actions,
}

impl Contribution {
    /// The name used in `plugin.toml` and in the UI (`sensors`, `cleanup`, `actions`).
    pub fn as_str(self) -> &'static str {
        match self {
            Contribution::Sensors => "sensors",
            Contribution::Cleanup => "cleanup",
            Contribution::Actions => "actions",
        }
    }
}

/// A plugin's identity, sent in answer to `Describe`. It must agree with the plugin's `plugin.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// A stable id: lowercase letters, digits, `-` and `_`, at most 64 characters; also the plugin's folder name.
    pub id: String,
    /// A name for people.
    pub name: String,
    /// The plugin's own version.
    pub version: String,
    /// The [`API_VERSION`] it was built against.
    pub api_version: u32,
    /// What it contributes.
    pub contributes: Vec<Contribution>,
}

impl Manifest {
    /// A manifest for this [`API_VERSION`] that contributes nothing yet.
    pub fn new(id: impl Into<String>, name: impl Into<String>, version: impl Into<String>) -> Self {
        Manifest {
            id: id.into(),
            name: name.into(),
            version: version.into(),
            api_version: API_VERSION,
            contributes: Vec::new(),
        }
    }

    /// Adds a contribution.
    pub fn contributing(mut self, c: Contribution) -> Self {
        if !self.contributes.contains(&c) {
            self.contributes.push(c);
        }
        self
    }

    /// Whether it contributes `c`.
    pub fn contributes(&self, c: Contribution) -> bool {
        self.contributes.contains(&c)
    }
}

/// Whether `id` is a valid plugin id: 1 to 64 of `a-z`, `0-9`, `-`, `_`, starting with a letter or digit.
pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    (1..=64).contains(&b.len())
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-' || *c == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_short_lowercase_names() {
        assert!(valid_id("example"));
        assert!(valid_id("my-plugin_2"));
        assert!(!valid_id(""));
        assert!(!valid_id("-x"));
        assert!(!valid_id("Upper"));
        assert!(!valid_id("../x"));
        assert!(!valid_id("a b"));
        assert!(!valid_id(&"a".repeat(65)));
    }

    #[test]
    fn a_manifest_lists_each_contribution_once() {
        let m = Manifest::new("x", "X", "1.0")
            .contributing(Contribution::Sensors)
            .contributing(Contribution::Sensors)
            .contributing(Contribution::Actions);
        assert_eq!(m.contributes, vec![Contribution::Sensors, Contribution::Actions]);
        assert!(m.contributes(Contribution::Actions) && !m.contributes(Contribution::Cleanup));
        assert_eq!(m.api_version, API_VERSION);
        assert_eq!(Contribution::Cleanup.as_str(), "cleanup");
        assert_eq!(Contribution::Sensors.as_str(), "sensors");
        assert_eq!(Contribution::Actions.as_str(), "actions");
    }
}
