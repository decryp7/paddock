//! The disk map's Settings (Performance > Limits): how big an index may grow and which files get a node of their own.

use super::index::{ScanLimits, HARD_MAX_NODES};
use crate::extras::files::{FinderLimits, MAX_DEPTH, MAX_HASH_BYTES, MAX_SCANNED};
use crate::threads::Threads;
use serde::{Deserialize, Serialize};

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

/// The most threads that read folders at once, whatever the setting says.
pub const MAX_READ_THREADS: u8 = 16;

/// Seconds between two readings of the volumes' free space when the Setting is Auto.
pub const SPACE_REFRESH_AUTO_SECS: u32 = 60;
/// The shortest and longest `disk.spaceRefreshSecs`.
pub const SPACE_REFRESH_RANGE_SECS: (u32, u32) = (10, 600);

/// Seconds between two readings of the volumes' free space for the Setting value `secs` (`0` is Auto: 60), kept in 10 to 600.
pub fn space_refresh_secs(secs: u32) -> u32 {
    if secs == 0 {
        SPACE_REFRESH_AUTO_SECS
    } else {
        secs.clamp(SPACE_REFRESH_RANGE_SECS.0, SPACE_REFRESH_RANGE_SECS.1)
    }
}

/// When the disk map refreshes a saved map by itself (`disk.savedMaps.refreshOnOpen`): never by default. Scanning a volume is the
/// heaviest thing Dyno does and it is a monitor; the page shows the net change in used space instead and the person decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefreshOnOpen {
    /// Only when the person presses Rescan.
    #[default]
    Off,
    /// At the first look at the page, when the saved map is more than a day old.
    OlderThanDay,
    /// At the first look at the page, when the saved map is more than seven days old.
    OlderThanWeek,
}

/// `disk.savedMaps`: the disk maps kept in the cache for the next time (ADR 0029).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SavedMapsSettings {
    /// Whether finished scans are kept (`disk.savedMaps.enabled`). Turning it off forgets every saved map.
    pub enabled: bool,
    /// The most all saved maps may take on disk, in MB (`disk.savedMaps.maxMb`); 64 to 8,192.
    pub max_mb: u32,
    /// Whether a saved map is refreshed at the first look at the page (`disk.savedMaps.refreshOnOpen`).
    pub refresh_on_open: RefreshOnOpen,
}

impl Default for SavedMapsSettings {
    fn default() -> Self {
        SavedMapsSettings {
            enabled: true,
            max_mb: 1024,
            refresh_on_open: RefreshOnOpen::Off,
        }
    }
}

impl SavedMapsSettings {
    /// The values clamped into their allowed range.
    pub fn clamped(self) -> SavedMapsSettings {
        SavedMapsSettings {
            max_mb: self.max_mb.clamp(64, 8_192),
            ..self
        }
    }

    /// The cap on the packed size of all saved maps, in bytes.
    pub fn max_bytes(self) -> u64 {
        u64::from(self.clamped().max_mb) * MIB
    }
}

/// `Settings.disk`. Every key is optional and merged over the defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DiskSettings {
    /// The most nodes one scan keeps (`disk.scanFolders`); 10,000 to the hard limit of 5,000,000.
    pub scan_folders: u32,
    /// Files of at least this many MiB get a node of their own; smaller ones are folded into their folder (1 to 1,024).
    pub notable_file_mib: u32,
    /// How many threads read folders at once, for the disk map and the file finder: Auto (by the kind of volume: a few on an SSD, one
    /// on a spinning disk, two per server on the network) or 1 to [`MAX_READ_THREADS`].
    pub read_threads: Threads,
    /// The file finder stops after this many files and folders (`disk.finderFiles`); 1,000 to 5,000,000.
    pub finder_files: u32,
    /// The file finder does not enter folders deeper than this (`disk.finderDepth`); 1 to 256.
    pub finder_depth: u32,
    /// The file finder stops comparing contents after reading this many GiB (`disk.finderHashGib`); 1 to 4,096.
    pub finder_hash_gib: u32,
    /// The disk maps kept for the next time (`disk.savedMaps`).
    pub saved_maps: SavedMapsSettings,
    /// Seconds between two readings of the volumes' free space (`disk.spaceRefreshSecs`): 0 is Auto (60), else 10 to 600. The agent
    /// reads it whether or not any window is open; every fifth reading lists the mounts again.
    pub space_refresh_secs: u32,
}

impl Default for DiskSettings {
    fn default() -> Self {
        let d = ScanLimits::default();
        DiskSettings {
            scan_folders: d.max_nodes,
            notable_file_mib: u32::try_from(d.notable_bytes / MIB).unwrap_or(10),
            read_threads: Threads::Auto,
            finder_files: u32::try_from(MAX_SCANNED).unwrap_or(500_000),
            finder_depth: MAX_DEPTH,
            finder_hash_gib: u32::try_from(MAX_HASH_BYTES / GIB).unwrap_or(64),
            saved_maps: SavedMapsSettings::default(),
            space_refresh_secs: 0,
        }
    }
}

impl DiskSettings {
    /// The values clamped into their allowed range.
    pub fn clamped(self) -> DiskSettings {
        DiskSettings {
            scan_folders: self.scan_folders.clamp(10_000, HARD_MAX_NODES),
            notable_file_mib: self.notable_file_mib.clamp(1, 1024),
            read_threads: self.read_threads.clamped(MAX_READ_THREADS),
            finder_files: self.finder_files.clamp(1_000, 5_000_000),
            finder_depth: self.finder_depth.clamp(1, 256),
            finder_hash_gib: self.finder_hash_gib.clamp(1, 4_096),
            saved_maps: self.saved_maps.clamped(),
            space_refresh_secs: if self.space_refresh_secs == 0 {
                0
            } else {
                space_refresh_secs(self.space_refresh_secs)
            },
        }
    }

    /// What one file finder scan may spend.
    pub fn finder_limits(self) -> FinderLimits {
        let s = self.clamped();
        FinderLimits {
            files: usize::try_from(s.finder_files).unwrap_or(usize::MAX),
            depth: s.finder_depth,
            hash_bytes: u64::from(s.finder_hash_gib) * GIB,
        }
    }

    /// How many threads read folders: the setting, or `auto` (what the kind of volume allows) when it is Auto.
    pub fn read_threads(self, auto: usize) -> usize {
        self.read_threads.resolve(auto, MAX_READ_THREADS)
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
    fn the_interval_setting_auto_is_60s_and_bounds_are_enforced() {
        assert_eq!(DiskSettings::default().space_refresh_secs, 0, "Auto by default");
        for (setting, want) in [(0, 60), (1, 10), (10, 10), (45, 45), (600, 600), (86_400, 600)] {
            assert_eq!(space_refresh_secs(setting), want, "setting {setting}");
        }
        let s = DiskSettings {
            space_refresh_secs: 5,
            ..DiskSettings::default()
        }
        .clamped();
        assert_eq!(s.space_refresh_secs, 10);
        assert_eq!(DiskSettings::default().clamped().space_refresh_secs, 0, "Auto stays Auto");
    }

    #[test]
    fn defaults_match_the_engines_limits() {
        assert_eq!(DiskSettings::default().limits(), ScanLimits::default());
    }

    #[test]
    fn values_are_clamped_and_become_limits() {
        let low = DiskSettings {
            scan_folders: 0,
            notable_file_mib: 0,
            ..DiskSettings::default()
        }
        .clamped();
        assert_eq!((low.scan_folders, low.notable_file_mib), (10_000, 1));
        let high = DiskSettings {
            scan_folders: u32::MAX,
            notable_file_mib: u32::MAX,
            ..DiskSettings::default()
        }
        .limits();
        assert_eq!((high.max_nodes, high.notable_bytes), (HARD_MAX_NODES, 1024 * MIB));
    }

    #[test]
    fn saved_maps_default_on_with_a_gigabyte_and_no_refresh_by_itself() {
        let d = DiskSettings::default().saved_maps;
        assert_eq!((d.enabled, d.max_mb, d.refresh_on_open), (true, 1024, RefreshOnOpen::Off));
        assert_eq!(d.max_bytes(), 1024 * MIB);
        let json = serde_json::to_value(DiskSettings::default()).unwrap();
        assert_eq!(
            json["savedMaps"],
            serde_json::json!({"enabled": true, "maxMb": 1024, "refreshOnOpen": "off"})
        );
        let s: DiskSettings = serde_json::from_str(r#"{"savedMaps":{"refreshOnOpen":"olderThanWeek"}}"#).unwrap();
        assert_eq!(
            (s.saved_maps.enabled, s.saved_maps.max_mb, s.saved_maps.refresh_on_open),
            (true, 1024, RefreshOnOpen::OlderThanWeek)
        );
        let s: DiskSettings = serde_json::from_str(r#"{"savedMaps":{"refreshOnOpen":"olderThanDay","enabled":false}}"#).unwrap();
        assert!(!s.saved_maps.enabled);
    }

    #[test]
    fn the_saved_maps_limit_is_held_to_64_mb_to_8_gb() {
        let wild = |mb| DiskSettings {
            saved_maps: SavedMapsSettings {
                max_mb: mb,
                ..SavedMapsSettings::default()
            },
            ..DiskSettings::default()
        };
        assert_eq!(wild(0).clamped().saved_maps.max_mb, 64);
        assert_eq!(wild(u32::MAX).clamped().saved_maps.max_mb, 8_192);
        assert_eq!(wild(500).clamped().saved_maps.max_mb, 500);
        assert_eq!(wild(u32::MAX).saved_maps.max_bytes(), 8_192 * MIB);
    }

    #[test]
    fn keys_are_optional() {
        let s: DiskSettings = serde_json::from_str(r#"{"scanFolders":50000}"#).unwrap();
        assert_eq!((s.scan_folders, s.notable_file_mib), (50_000, 10));
    }

    #[test]
    fn the_defaults_are_the_products_and_every_new_key_is_optional() {
        let d = DiskSettings::default();
        assert_eq!(
            (d.read_threads, d.finder_files, d.finder_depth, d.finder_hash_gib),
            (Threads::Auto, 500_000, 48, 64)
        );
        assert_eq!(d.finder_limits(), FinderLimits::default());
        let s: DiskSettings = serde_json::from_str(r#"{"readThreads":{"fixed":6},"finderDepth":10}"#).unwrap();
        assert_eq!((s.read_threads, s.finder_depth, s.finder_files), (Threads::Fixed(6), 10, 500_000));
    }

    #[test]
    fn thread_and_finder_values_are_validated_in_the_engine() {
        let wild = DiskSettings {
            read_threads: Threads::Fixed(200),
            finder_files: 0,
            finder_depth: 0,
            finder_hash_gib: u32::MAX,
            ..DiskSettings::default()
        };
        let c = wild.clamped();
        assert_eq!(
            (c.read_threads, c.finder_files, c.finder_depth, c.finder_hash_gib),
            (Threads::Fixed(16), 1_000, 1, 4_096)
        );
        assert_eq!(wild.read_threads(4), 16);
        assert_eq!(DiskSettings::default().read_threads(4), 4);
        assert_eq!(
            DiskSettings {
                read_threads: Threads::Fixed(0),
                ..DiskSettings::default()
            }
            .read_threads(4),
            1
        );
        let l = wild.finder_limits();
        assert_eq!((l.files, l.depth, l.hash_bytes), (1_000, 1, 4_096 * GIB));
    }
}
