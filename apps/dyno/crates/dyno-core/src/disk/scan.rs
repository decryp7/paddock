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

/// Live numbers. All of them only grow.
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
    /// Folders the OS refused to open (counted, not fatal).
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
