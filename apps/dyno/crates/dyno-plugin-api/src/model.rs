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
