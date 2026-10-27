//! The traits a plugin implements. Keep them small: a plugin implements only the providers it contributes.

use crate::manifest::Manifest;
use crate::model::{ActionInfo, Candidate, CategoryInfo, RuleEvent, ScanLimits, SensorReading};

/// A plugin: its manifest and the providers it contributes. Calls may arrive on several threads at once.
pub trait Plugin: Send + Sync + 'static {
    /// Who it is and what it contributes; must agree with its `plugin.toml`.
    fn manifest(&self) -> Manifest;

    /// Its sensors, if it contributes any.
    fn as_sensors(&self) -> Option<&dyn SensorProvider> {
        None
    }

    /// Its cleanup categories, if it contributes any.
    fn as_cleanup(&self) -> Option<&dyn CleanupProvider> {
        None
    }

    /// Its actions, if it contributes any.
    fn as_actions(&self) -> Option<&dyn ActionProvider> {
        None
    }
}

/// Extra sensor readings. Called about once a second with a 2 s timeout: be quick.
pub trait SensorProvider: Send + Sync {
    /// The current readings.
    fn sensors(&self) -> Vec<SensorReading>;
}

/// Cleanup categories. The plugin only proposes; the host checks and deletes.
pub trait CleanupProvider: Send + Sync {
    /// The categories offered.
    fn categories(&self) -> Vec<CategoryInfo>;

    /// The candidates of `category`, within `limits` (60 s timeout). An unknown category is an error.
    fn scan(&self, category: &str, limits: ScanLimits) -> Result<Vec<Candidate>, PluginError>;
}

/// Actions rules can trigger.
pub trait ActionProvider: Send + Sync {
    /// The actions offered.
    fn actions(&self) -> Vec<ActionInfo>;

    /// Runs `action_id` for `event` (10 s timeout); the text is shown in the action log.
    fn run(&self, action_id: &str, event: RuleEvent) -> Result<String, PluginError>;
}

/// A plugin's own, expected failure (a folder that is missing, a webhook that refused). Not counted as a crash.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct PluginError {
    /// What went wrong, for people.
    pub message: String,
}

impl PluginError {
    /// An error saying `message`.
    pub fn new(message: impl Into<String>) -> Self {
        PluginError { message: message.into() }
    }
}
