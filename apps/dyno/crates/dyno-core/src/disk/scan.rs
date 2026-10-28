//! The disk scan as a job (ADR 0024): what is scanned, the phases, the live progress, how the state ends, how many
//! threads may walk, and the rule that keeps pushed events to a few per second. Pure; `dyno-agent` runs it.

use super::volume::{Volume, VolumeLocation};
use serde::{Deserialize, Serialize};

/// What a scan covers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DiskTarget {
    /// A volume from the volume list, by id.
    Volume {
        /// The volume's id.
        id: String,
    },
    /// A folder the person chose.
    Folder {
        /// Its full path.
        path: String,
    },
}

/// The phases of a scan, each on its own scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanPhase {
    /// Reading the volume's capacity and free space (instant).
    ReadingVolumeFacts,
    /// Walking the folders (bytes counted, out of the used bytes for a whole volume; a growing count otherwise).
    ReadingFolders,
}

/// Why a scan failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ScanFailure {
    /// The volume stopped answering; what was counted is kept.
    VolumeUnreachable {
        /// What the OS said.
        detail: String,
    },
    /// The target cannot be scanned at all.
    CannotStart {
        /// Why, for the person.
        reason: String,
    },
    /// A bug in the scan.
    Internal {
        /// What went wrong.
        detail: String,
    },
}

/// Where a scan stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ScanState {
    /// Walking.
    Running,
    /// Finished.
    Done,
    /// Stopped by the person; the partial index stays.
    Cancelled,
    /// Stopped by an error; the partial index stays.
    Failed {
        /// The cause.
        failure: ScanFailure,
    },
}

impl ScanState {
    /// Whether the scan has ended.
    pub fn is_final(&self) -> bool {
        !matches!(self, ScanState::Running)
    }
}

/// Live numbers. They only grow, except `unreadable` when a retry reads refused folders again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    /// Bytes counted so far.
    pub bytes: u64,
    /// Files and folders counted so far.
    pub items: u64,
    /// Folders fully listed so far.
    pub folders: u64,
    /// The bytes expected in all (a whole volume's used space), or `None` when the total is unknown and the count just
    /// grows. Never below `bytes`.
    pub total_bytes: Option<u64>,
    /// Folders the OS refused to open (counted, not fatal). A retry takes back the folders it reads again
    /// (a folder refused again is counted anew), so this can fall; every other number only grows.
    pub unreadable: u64,
}

/// One scan's status: the reply to starting it and the pushed event while it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskScanStatus {
    /// The scan's id.
    pub scan: u64,
    /// Rises with every event; a consumer keeps the highest.
    pub seq: u64,
    /// What is scanned, as the person sees it.
    pub name: String,
    /// What was asked for.
    pub target: DiskTarget,
    /// The scan's volume when the target is one.
    pub volume: Option<Volume>,
    /// Running, done, cancelled or failed.
    pub state: ScanState,
    /// The phase now (the last one reached once it ends).
    pub phase: ScanPhase,
    /// The numbers.
    pub progress: ScanProgress,
    /// Whether a limit left folders out (the node limit).
    pub truncated: bool,
    /// The first folders that could not be read.
    pub unreadable_first: Vec<String>,
    /// Folders were refused and this Mac has not given Dyno Full Disk Access, which is the likely reason (macOS only).
    pub needs_full_disk_access: bool,
    /// Files and folders counted per second while the scan runs; `None` until there is enough to say (never a made-up number).
    pub items_per_second: Option<u64>,
    /// Seconds left while a first pass over a whole volume runs, from the bytes counted so far; `None` when the total is
    /// unknown or too little is counted.
    pub eta_seconds: Option<u64>,
    /// Used space the scan did not count (used minus counted, for a whole-volume scan that is done): snapshots, purgeable
    /// space, protected folders. `None` when the scan has no known total or is not done.
    pub hidden_bytes: Option<u64>,
    /// When the latest pass ended (the "scanned 2 min ago" line); `None` while the first pass runs.
    pub scanned_at: Option<crate::time::Timestamp>,
}

/// Rates and estimates are not given before this much of the scan has run.
pub const RATE_AFTER_MS: u64 = 2_000;

/// `count` per second over `elapsed_ms`, or `None` when too little has run or nothing was counted.
pub fn per_second(count: u64, elapsed_ms: u64) -> Option<u64> {
    (elapsed_ms >= RATE_AFTER_MS && count > 0).then(|| count.saturating_mul(1000) / elapsed_ms)
}

/// Seconds until `total` bytes are counted if the pace so far holds, or `None` when it cannot be known.
pub fn eta_seconds(done: u64, total: u64, elapsed_ms: u64) -> Option<u64> {
    if elapsed_ms < RATE_AFTER_MS || done == 0 || total <= done {
        return None;
    }
    let remaining = u128::from(total - done);
    let ms = remaining * u128::from(elapsed_ms) / u128::from(done);
    u64::try_from(ms.div_ceil(1000)).ok()
}

/// Used space that was not counted, for a whole-volume scan that has a total.
pub fn hidden_bytes(total_used: Option<u64>, counted: u64) -> Option<u64> {
    total_used.map(|t| t.saturating_sub(counted))
}

/// Pushed events are at most this far apart.
pub const EVENT_INTERVAL_MS: u64 = 250;
/// How many unreadable folders a status names.
pub const UNREADABLE_LISTED: usize = 10;

/// The most threads that walk at once: a few on an SSD, one or two on a spinning disk, two per server on the network.
pub fn walk_threads(location: &VolumeLocation, rotational: Option<bool>, cores: usize) -> usize {
    let spare = cores.saturating_sub(1).max(1);
    match location {
        VolumeLocation::Network { .. } => 2,
        _ if rotational == Some(true) => 1,
        _ => 4.min(spare),
    }
}

/// Decides when an event may be pushed: not before [`EVENT_INTERVAL_MS`] after the last one.
#[derive(Debug, Default)]
pub struct EventGate {
    last: Option<u64>,
}

impl EventGate {
    /// True (and remembers `now_ms`) when an event may go out.
    pub fn allow(&mut self, now_ms: u64) -> bool {
        let ok = self.last.is_none_or(|l| now_ms >= l.saturating_add(EVENT_INTERVAL_MS));
        if ok {
            self.last = Some(now_ms);
        }
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::volume::NetworkProtocol;

    fn net() -> VolumeLocation {
        VolumeLocation::Network {
            protocol: NetworkProtocol::Smb,
            host: "nas.local".into(),
        }
    }

    #[test]
    fn threads_follow_the_kind_of_storage() {
        assert_eq!(walk_threads(&VolumeLocation::Internal, Some(false), 14), 4);
        assert_eq!(walk_threads(&VolumeLocation::Internal, None, 3), 2);
        assert_eq!(walk_threads(&VolumeLocation::Internal, None, 1), 1);
        assert_eq!(walk_threads(&VolumeLocation::External { bus: None }, Some(true), 14), 1);
        assert_eq!(walk_threads(&net(), None, 14), 2);
        assert_eq!(walk_threads(&net(), Some(true), 1), 2);
    }

    #[test]
    fn rates_and_estimates_are_withheld_until_they_mean_something() {
        assert_eq!(per_second(500, 1_999), None, "too early");
        assert_eq!(per_second(0, 5_000), None, "nothing counted");
        assert_eq!(per_second(5_000, 2_000), Some(2_500));
        assert_eq!(eta_seconds(100, 1_000, 1_000), None, "too early");
        assert_eq!(eta_seconds(0, 1_000, 5_000), None, "no pace yet");
        assert_eq!(eta_seconds(1_000, 1_000, 5_000), None, "already there");
        assert_eq!(eta_seconds(100, 1_000, 10_000), Some(90), "900 more at 10 per second");
        assert_eq!(eta_seconds(1, u64::MAX, u64::MAX), None, "too large to say");
        assert_eq!(hidden_bytes(Some(900), 600), Some(300));
        assert_eq!(hidden_bytes(Some(500), 600), Some(0));
        assert_eq!(hidden_bytes(None, 600), None);
    }

    #[test]
    fn events_are_spaced() {
        let mut g = EventGate::default();
        assert!(g.allow(1_000));
        assert!(!g.allow(1_100));
        assert!(!g.allow(1_249));
        assert!(g.allow(1_250));
        assert!(!g.allow(1_000), "a clock that went back never allows");
    }

    #[test]
    fn only_a_running_scan_is_not_final() {
        assert!(!ScanState::Running.is_final());
        for s in [
            ScanState::Done,
            ScanState::Cancelled,
            ScanState::Failed {
                failure: ScanFailure::Internal { detail: "x".into() },
            },
        ] {
            assert!(s.is_final());
        }
    }

    #[test]
    fn phases_are_ordered() {
        assert!(ScanPhase::ReadingVolumeFacts < ScanPhase::ReadingFolders);
    }
}
