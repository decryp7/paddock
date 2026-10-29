//! How often to sample: fast when someone is looking or rules need it, slower otherwise, and slower still when Dyno's
//! own CPU goes over budget (ADR 0002: Dyno stays under ~1 % CPU).

use crate::protocol::LiveDetail;
use crate::settings::Sampling;
use serde::{Deserialize, Serialize};

/// Dyno's CPU budget, percent of the machine.
pub const CPU_BUDGET: f32 = 1.0;

/// What the sampler should do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Intervals {
    /// System sensors.
    pub system_ms: u32,
    /// The process table.
    pub process_ms: u32,
}

/// The situation the intervals depend on.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Load {
    /// A window shows live data at this detail (`None`: no window open).
    pub watching: Option<LiveDetail>,
    /// On battery.
    pub on_battery: bool,
    /// Dyno's agent CPU over the last minute, percent of the machine.
    pub own_cpu: f32,
}

/// The intervals for the user's settings and the situation.
pub fn intervals(s: Sampling, load: Load) -> Intervals {
    let mut system = s.interval_ms.clamp(500, 10_000);
    let mut process = s.process_interval_ms.clamp(1_000, 30_000).max(system);
    match load.watching {
        Some(_) => {}
        // Nobody watching: history needs a point every 5 s and the process table every 10 s.
        None => {
            system = system.max(2_000);
            process = process.max(10_000);
        }
    }
    if load.on_battery {
        system = (system * 2).min(10_000);
        process = (process * 2).min(30_000);
    }
    if load.own_cpu > CPU_BUDGET {
        // Back off in proportion to the overshoot.
        let factor = (load.own_cpu / CPU_BUDGET).min(4.0);
        process = ((process as f32 * factor) as u32).min(30_000);
        system = ((system as f32 * factor.sqrt()) as u32).min(10_000);
    }
    Intervals {
        system_ms: system,
        process_ms: process.max(system),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watching_uses_the_settings() {
        let i = intervals(
            Sampling::default(),
            Load {
                watching: Some(LiveDetail::Processes),
                ..Default::default()
            },
        );
        assert_eq!(
            i,
            Intervals {
                system_ms: 1_000,
                process_ms: 2_000
            }
        );
    }

    #[test]
    fn idle_battery_and_overshoot_slow_down() {
        let idle = intervals(Sampling::default(), Load::default());
        assert_eq!(
            idle,
            Intervals {
                system_ms: 2_000,
                process_ms: 10_000
            }
        );
        let battery = intervals(
            Sampling::default(),
            Load {
                watching: Some(LiveDetail::Summary),
                on_battery: true,
                ..Default::default()
            },
        );
        assert_eq!(
            battery,
            Intervals {
                system_ms: 2_000,
                process_ms: 4_000
            }
        );
        let hot = intervals(
            Sampling::default(),
            Load {
                watching: Some(LiveDetail::Summary),
                own_cpu: 4.0,
                ..Default::default()
            },
        );
        assert_eq!(
            hot,
            Intervals {
                system_ms: 2_000,
                process_ms: 8_000
            }
        );
        let cap = intervals(
            Sampling {
                interval_ms: 10_000,
                process_interval_ms: 30_000,
            },
            Load {
                own_cpu: 50.0,
                on_battery: true,
                ..Default::default()
            },
        );
        assert_eq!(
            cap,
            Intervals {
                system_ms: 10_000,
                process_ms: 30_000
            }
        );
    }
}
