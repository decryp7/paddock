//! The traits a plugin implements. Keep them small: a plugin implements only the providers it contributes.

use crate::manifest::Manifest;
use crate::model::SensorReading;

/// A plugin: its manifest and the providers it contributes. Calls may arrive on several threads at once.
pub trait Plugin: Send + Sync + 'static {
    /// Who it is and what it contributes; must agree with its `plugin.toml`.
    fn manifest(&self) -> Manifest;

    /// Its sensors, if it contributes any.
    fn as_sensors(&self) -> Option<&dyn SensorProvider> {
        None
    }
}

/// Extra sensor readings. Called about once a second with a 2 s timeout: be quick.
pub trait SensorProvider: Send + Sync {
    /// The current readings.
    fn sensors(&self) -> Vec<SensorReading>;
}
