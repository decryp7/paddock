//! How often to sample: fast when someone is looking or rules need it, slower otherwise, and slower still when Dyno's
//! own CPU goes over budget (ADR 0002: Dyno stays under ~1 % CPU).

use crate::protocol::LiveDetail;
use crate::settings::Sampling;
use serde::{Deserialize, Serialize};

/// Dyno's CPU budget, percent of the machine.
pub const CPU_BUDGET: f32 = 1.0;
/// The system interval when nobody watches: history needs a point every 5 s.
pub const UNWATCHED_SYSTEM_MS: u32 = 2_000;
/// The process table's interval when no window shows it (nobody watching, or only the menu bar icons).
pub const UNWATCHED_PROCESS_MS: u32 = 10_000;

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
        Some(LiveDetail::Summary | LiveDetail::Processes) => {}
        // Only the menu bar icons: they draw the system sample at the user's pace, and nothing shows the process table.
        Some(LiveDetail::Tray) => process = process.max(UNWATCHED_PROCESS_MS),
        // Nobody watching: history needs a point every 5 s and the process table every 10 s.
        None => {
            system = system.max(UNWATCHED_SYSTEM_MS);
            process = process.max(UNWATCHED_PROCESS_MS);
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

    // With the window closed the app still subscribed at the summary level (the tray needs numbers), so the agent read
    // the whole process table every 2 s for nobody: 0.72-0.74% of one core against 0.38-0.39% unwatched (VM bench,
    // docs/benchmarks.md 2026-11-03). The tray level keeps the system pace and reads processes at the unwatched pace.
    #[test]
    fn the_tray_alone_keeps_the_system_pace_and_reads_processes_unwatched() {
        let tray = intervals(
            Sampling::default(),
            Load {
                watching: Some(LiveDetail::Tray),
                ..Default::default()
            },
        );
        assert_eq!(
            tray,
            Intervals {
                system_ms: Sampling::default().interval_ms,
                process_ms: UNWATCHED_PROCESS_MS
            }
        );
        let summary = intervals(
            Sampling::default(),
            Load {
                watching: Some(LiveDetail::Summary),
                ..Default::default()
            },
        );
        assert!(summary.process_ms < tray.process_ms);
    }

    #[test]
    fn ranks_order_the_details() {
        assert!(LiveDetail::Tray.rank() < LiveDetail::Summary.rank());
        assert!(LiveDetail::Summary.rank() < LiveDetail::Processes.rank());
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
                helper_sweep_ms: None,
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
