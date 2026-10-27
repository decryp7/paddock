//! The values plugins build and the host consumes.

use serde::{Deserialize, Serialize};

/// One sensor reading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensorReading {
    /// Stable within the plugin (the host prefixes it with the plugin id).
    pub id: String,
    /// A label for people.
    pub label: String,
    /// The value; non-finite values are dropped by the host.
    pub value: f64,
    /// Its unit (`"°C"`, `"%"`, `"rpm"`, or empty).
    pub unit: String,
}

impl SensorReading {
    /// A reading.
    pub fn new(id: impl Into<String>, label: impl Into<String>, value: f64, unit: impl Into<String>) -> Self {
        SensorReading {
            id: id.into(),
            label: label.into(),
            value,
            unit: unit.into(),
        }
    }
}

/// How careful the user must be with a cleanup category (matches the UI's `CleanupRisk`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    /// Regenerated caches and temporary files.
    Safe,
    /// Probably unneeded, but the user should look first.
    Review,
    /// Could be the user's own files: never pre-selected.
    Personal,
}

/// A cleanup category a plugin offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryInfo {
    /// Stable within the plugin.
    pub id: String,
    /// A name for people.
    pub name: String,
    /// What it removes and why that is safe.
    pub description: String,
    /// How careful to be.
    pub risk: Risk,
}

/// Bounds of one scan, set by the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanLimits {
    /// At most this many candidates (the host drops any beyond).
    pub max_candidates: u32,
    /// At most this deep below a root folder.
    pub max_depth: u32,
}

impl Default for ScanLimits {
    fn default() -> Self {
        ScanLimits {
            max_candidates: 10_000,
            max_depth: 8,
        }
    }
}

/// A file or folder a plugin proposes for deletion. Only a proposal: the host checks it against Dyno's cleanup
/// safety policy, shows it in the preview and deletes it itself, if the user agrees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// An absolute path.
    pub path: String,
    /// Its size in bytes (a folder: its contents).
    pub bytes: u64,
    /// When it was last modified, Unix milliseconds.
    pub modified_ms: i64,
    /// Why it can go, for the preview.
    pub reason: String,
}

/// An action a rule can trigger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionInfo {
    /// Stable within the plugin.
    pub id: String,
    /// A name for people.
    pub name: String,
    /// What it does.
    pub description: String,
}

/// Why a rule triggered an action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleEvent {
    /// The rule's name.
    pub rule: String,
    /// The app it matched.
    pub app: String,
    /// Its CPU use in percent of one core.
    pub cpu: f64,
    /// When, Unix milliseconds.
    pub at: i64,
}
