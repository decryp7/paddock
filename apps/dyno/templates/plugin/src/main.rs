//! A Dyno plugin. Implement the sensor provider and list it in the manifest and in plugin.toml.

use dyno_plugin_api::{Contribution, Manifest, Plugin, SensorProvider, SensorReading};

struct Mine;

impl Plugin for Mine {
    fn manifest(&self) -> Manifest {
        // The id must match plugin.toml and the plugin's folder name.
        Manifest::new("mine", "My plugin", env!("CARGO_PKG_VERSION")).contributing(Contribution::Sensors)
    }

    fn as_sensors(&self) -> Option<&dyn SensorProvider> {
        Some(self)
    }
}

impl SensorProvider for Mine {
    /// Called about once a second with a 2 s timeout: return quickly, never block.
    fn sensors(&self) -> Vec<SensorReading> {
        vec![SensorReading::new("answer", "My plugin: the answer", 42.0, "")]
    }
}

fn main() -> std::process::ExitCode {
    dyno_plugin_api::serve(Mine)
}
