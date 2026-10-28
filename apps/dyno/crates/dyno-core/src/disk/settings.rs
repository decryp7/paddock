//! The disk map's Settings (Performance > Limits): how big an index may grow and which files get a node of their own.

use super::index::{ScanLimits, HARD_MAX_NODES};
use serde::{Deserialize, Serialize};

const MIB: u64 = 1024 * 1024;

/// `Settings.disk`. Every key is optional and merged over the defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DiskSettings {
    /// The most nodes one scan keeps (`disk.scan.folders`); 10,000 to the hard limit of 5,000,000.
    pub scan_folders: u32,
    /// Files of at least this many MiB get a node of their own; smaller ones are folded into their folder (1 to 1,024).
    pub notable_file_mib: u32,
}

impl Default for DiskSettings {
    fn default() -> Self {
        let d = ScanLimits::default();
        DiskSettings {
            scan_folders: d.max_nodes,
            notable_file_mib: u32::try_from(d.notable_bytes / MIB).unwrap_or(10),
        }
    }
}

impl DiskSettings {
    /// The values clamped into their allowed range.
    pub fn clamped(self) -> DiskSettings {
        DiskSettings {
            scan_folders: self.scan_folders.clamp(10_000, HARD_MAX_NODES),
            notable_file_mib: self.notable_file_mib.clamp(1, 1024),
        }
    }

    /// The limits a scan runs with.
    pub fn limits(self) -> ScanLimits {
        let s = self.clamped();
        ScanLimits {
            max_nodes: s.scan_folders,
            notable_bytes: u64::from(s.notable_file_mib) * MIB,
            ..ScanLimits::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_engines_limits() {
        assert_eq!(DiskSettings::default().limits(), ScanLimits::default());
    }

    #[test]
    fn values_are_clamped_and_become_limits() {
        let low = DiskSettings { scan_folders: 0, notable_file_mib: 0 }.clamped();
        assert_eq!((low.scan_folders, low.notable_file_mib), (10_000, 1));
        let high = DiskSettings { scan_folders: u32::MAX, notable_file_mib: u32::MAX }.limits();
        assert_eq!((high.max_nodes, high.notable_bytes), (HARD_MAX_NODES, 1024 * MIB));
    }

    #[test]
    fn keys_are_optional() {
        let s: DiskSettings = serde_json::from_str(r#"{"scanFolders":50000}"#).unwrap();
        assert_eq!((s.scan_folders, s.notable_file_mib), (50_000, 10));
    }
}
