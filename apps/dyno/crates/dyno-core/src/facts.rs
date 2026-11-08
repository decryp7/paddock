//! Slow-changing facts about the machine that health and tuning reason about. The platform gathers them (every few
//! minutes, off the sampling path); the core only reads them.

use crate::metrics::Reading;
use serde::{Deserialize, Serialize};

/// Health of one physical disk as the OS reports it (SMART / storage health).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskHealth {
    /// Model or friendly name.
    pub name: String,
    /// The OS's verdict.
    pub status: DiskHealthStatus,
    /// Wear as a percentage of rated endurance (SSDs), when reported.
    pub wear: Option<f32>,
    /// Temperature in °C, when reported.
    pub temperature: Option<f32>,
}

/// A disk's health verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiskHealthStatus {
    /// Healthy.
    Healthy,
    /// The OS warns (predicted failure, degraded).
    Warning,
    /// Failed or failing.
    Unhealthy,
    /// Not reported.
    Unknown,
}

/// The active power plan / energy mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerPlan {
    /// Platform id (Windows scheme GUID; macOS "lowpower" / "normal" / "high").
    pub id: String,
    /// Display name.
    pub name: String,
    /// Whether it favours saving power over speed.
    pub power_saver: bool,
}

/// A folder developers build in (detected from project markers) and whether the OS indexes / scans it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevFolder {
    /// The folder.
    pub path: String,
    /// Whether the search indexer includes it, when known.
    pub indexed: Option<bool>,
    /// Whether the antivirus excludes it, when known (reading Defender's exclusions needs administrator rights).
    pub av_excluded: Option<bool>,
}

/// A fact in [`SystemFacts`] that a source may declare it never reads on its OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FactKind {
    /// `disks`.
    Disks,
    /// `restart_pending`.
    RestartPending,
    /// `pending_updates`.
    PendingUpdates,
    /// `power_plan`.
    PowerPlan,
    /// `visual_effects_full`.
    VisualEffects,
    /// `background_apps`.
    BackgroundApps,
    /// `last_boot_secs`.
    LastBoot,
}

impl FactKind {
    /// Every fact, for a source that reads none of them.
    pub const ALL: [FactKind; 7] = [
        FactKind::Disks,
        FactKind::RestartPending,
        FactKind::PendingUpdates,
        FactKind::PowerPlan,
        FactKind::VisualEffects,
        FactKind::BackgroundApps,
        FactKind::LastBoot,
    ];
}

/// Everything slow-changing that health and tuning look at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemFacts {
    /// OS name and version, e.g. "Windows 11 Pro 24H2".
    pub os: String,
    /// Disk health per physical disk, or why it can't be read.
    pub disks: Reading<Vec<DiskHealth>>,
    /// Whether an OS update waits for a restart.
    pub restart_pending: Reading<bool>,
    /// Number of OS or driver updates waiting to install, when known.
    pub pending_updates: Reading<u32>,
    /// The active power plan.
    pub power_plan: Reading<PowerPlan>,
    /// Whether animations and transparency are on (visual effects).
    pub visual_effects_full: Reading<bool>,
    /// Number of apps allowed to run in the background (Windows), when known.
    pub background_apps: Reading<u32>,
    /// Developer folders found in the user's profile.
    pub dev_folders: Vec<DevFolder>,
    /// Seconds the last boot took (from firmware hand-off to the desktop), when the OS reports it.
    pub last_boot_secs: Reading<f32>,
    /// Facts this OS's source never reads, by design (as opposed to one that failed this time). Health tells the two apart.
    pub not_read_here: Vec<FactKind>,
}

impl SystemFacts {
    /// Facts with nothing known yet (before the first gathering finishes).
    pub fn unknown(os: impl Into<String>) -> SystemFacts {
        fn r<T>() -> Reading<T> {
            Reading::unavailable("Not checked yet")
        }
        SystemFacts {
            os: os.into(),
            disks: r(),
            restart_pending: r(),
            pending_updates: r(),
            power_plan: r(),
            visual_effects_full: r(),
            background_apps: r(),
            dev_folders: Vec::new(),
            last_boot_secs: r(),
            not_read_here: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_facts_say_not_checked() {
        let f = SystemFacts::unknown("Windows 11");
        assert_eq!(f.restart_pending, Reading::Unavailable("Not checked yet".into()));
        assert!(f.dev_folders.is_empty());
        assert_eq!(f.os, "Windows 11");
    }
}
