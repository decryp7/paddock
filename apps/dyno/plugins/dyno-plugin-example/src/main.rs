//! An example Dyno plugin, and the reference for writing one (see docs/plugins.md): one sensor, "Dyno example:
//! seconds since start". Plugins contribute sensor readings only; they never change the machine through Dyno.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use dyno_plugin_api::{Contribution, Manifest, Plugin, SensorProvider, SensorReading};
use std::time::Instant;

struct Example {
    started: Instant,
}

impl Plugin for Example {
    fn manifest(&self) -> Manifest {
        Manifest::new("example", "Dyno example", env!("CARGO_PKG_VERSION")).contributing(Contribution::Sensors)
    }

    fn as_sensors(&self) -> Option<&dyn SensorProvider> {
        Some(self)
    }
}

impl SensorProvider for Example {
    fn sensors(&self) -> Vec<SensorReading> {
        vec![SensorReading::new(
            "uptime",
            "Dyno example: seconds since start",
            self.started.elapsed().as_secs_f64().floor(),
            "s",
        )]
    }
}

fn main() -> std::process::ExitCode {
    dyno_plugin_api::serve(Example { started: Instant::now() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_describes_itself_and_reports_its_uptime() {
        let e = Example { started: Instant::now() };
        let m = e.manifest();
        assert_eq!(m.id, "example");
        assert!(m.contributes(Contribution::Sensors));
        let s = e.sensors();
        assert_eq!(s[0].id, "uptime");
        assert!(s[0].value >= 0.0);
        assert!(e.as_sensors().is_some());
    }
}
