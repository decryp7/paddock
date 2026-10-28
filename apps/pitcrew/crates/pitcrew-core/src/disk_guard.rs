//! The low-disk guard (ADR 0013): drain a runner before its disk is full, resume it when space has recovered.
//!
//! Pure rules; the service reads the free space and carries the decision out through [`crate::hold`]. Two thresholds
//! give hysteresis (a disk hovering around one limit does not flap), and a *reserve* below them is space Pitcrew keeps
//! for itself so its own state, logs and cleanup never starve (the lesson of Jenkins' disk monitor, which failed on a
//! full disk): under the reserve the service stops recording metrics and cleans at once.
//!
//! Reading the free space is not free on macOS, so [`SpaceCache`] answers from memory for at least
//! [`MIN_POLL_GAP_MS`] between two reads of the same disk.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One MB (storage steps: 1000 bytes per KB, as the Finder).
const MB: u64 = 1_000_000;
/// The shortest time between two reads of one disk's free space.
pub const MIN_POLL_GAP_MS: u64 = 60_000;
/// Most disks remembered.
const MAX_DISKS: usize = 64;

/// The guard's thresholds (defaults for every runner).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiskGuardPolicy {
    /// Drain runners automatically when the disk is nearly full.
    pub enabled: bool,
    /// Drain below this much free space (MB).
    pub drain_below_mb: u64,
    /// Resume when the free space is back above this (MB); must exceed `drain_below_mb`.
    pub resume_above_mb: u64,
    /// Space kept for Pitcrew itself (MB); under it metrics stop and a cleanup runs at once. Below `drain_below_mb`.
    pub reserve_mb: u64,
}

impl Default for DiskGuardPolicy {
    fn default() -> Self {
        DiskGuardPolicy {
            enabled: true,
            drain_below_mb: 4_000,
            resume_above_mb: 6_000,
            reserve_mb: 1_000,
        }
    }
}

impl DiskGuardPolicy {
    /// Checks the relations between the thresholds, with a message for the user.
    pub fn validate(&self) -> Result<(), String> {
        if self.resume_above_mb <= self.drain_below_mb {
            return Err("the disk guard must resume at a higher free space than it drains at".into());
        }
        if self.reserve_mb >= self.drain_below_mb {
            return Err("the disk guard's reserve must be smaller than the drain threshold".into());
        }
        Ok(())
    }
}

/// How full the disk is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiskLevel {
    /// Enough free space.
    Ok,
    /// Below the drain threshold: runners are held.
    Low,
    /// Below the reserve: Pitcrew protects itself too.
    Critical,
}

/// The level for `free` bytes (`None`: unknown, treated as fine).
pub fn level(policy: &DiskGuardPolicy, free: Option<u64>) -> DiskLevel {
    match free {
        Some(f) if f < policy.reserve_mb.saturating_mul(MB) => DiskLevel::Critical,
        Some(f) if f < policy.drain_below_mb.saturating_mul(MB) => DiskLevel::Low,
        _ => DiskLevel::Ok,
    }
}

/// Whether the low-disk hold is wanted now. `held`: it is on already, so it stays until the free space has recovered
/// above the resume threshold. An unknown free space keeps the current state.
pub fn hold_wanted(policy: &DiskGuardPolicy, free: Option<u64>, held: bool) -> bool {
    if !policy.enabled {
        return false;
    }
    let Some(f) = free else { return held };
    if held {
        f < policy.resume_above_mb.saturating_mul(MB)
    } else {
        f < policy.drain_below_mb.saturating_mul(MB)
    }
}

/// Free and total bytes of a disk.
pub type Space = (u64, u64);

/// Remembers the last reading of each disk so the OS is asked at most once per [`MIN_POLL_GAP_MS`].
#[derive(Debug, Default)]
pub struct SpaceCache {
    entries: HashMap<String, (Timestamp, Option<Space>)>,
}

impl SpaceCache {
    /// The space of the disk `key` (a folder on it): the remembered reading when it is younger than the gap, else
    /// `read()`'s. A failed read is remembered too (and retried after the gap).
    pub fn get(&mut self, key: &str, now: Timestamp, read: impl FnOnce() -> Option<Space>) -> Option<Space> {
        if let Some((at, space)) = self.entries.get(key) {
            if now.0.saturating_sub(at.0) < MIN_POLL_GAP_MS {
                return *space;
            }
        }
        let space = read();
        if self.entries.len() >= MAX_DISKS && !self.entries.contains_key(key) {
            self.entries.clear();
        }
        self.entries.insert(key.to_string(), (now, space));
        space
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1000 * MB;

    #[test]
    fn levels_follow_the_thresholds() {
        let p = DiskGuardPolicy::default();
        assert_eq!(level(&p, Some(10 * GB)), DiskLevel::Ok);
        assert_eq!(level(&p, Some(3 * GB)), DiskLevel::Low);
        assert_eq!(level(&p, Some(GB / 2)), DiskLevel::Critical);
        assert_eq!(level(&p, None), DiskLevel::Ok);
    }

    #[test]
    fn the_hold_has_hysteresis_and_unknown_space_changes_nothing() {
        let p = DiskGuardPolicy::default();
        assert!(
            !hold_wanted(&p, Some(5 * GB), false),
            "between the thresholds and not held: stay free"
        );
        assert!(hold_wanted(&p, Some(3 * GB), false));
        assert!(hold_wanted(&p, Some(5 * GB), true), "held until above the resume threshold");
        assert!(!hold_wanted(&p, Some(7 * GB), true));
        assert!(hold_wanted(&p, None, true));
        assert!(!hold_wanted(&p, None, false));
        let off = DiskGuardPolicy { enabled: false, ..p };
        assert!(!hold_wanted(&off, Some(0), true));
    }

    #[test]
    fn thresholds_are_validated() {
        assert!(DiskGuardPolicy::default().validate().is_ok());
        let bad = DiskGuardPolicy {
            resume_above_mb: 100,
            drain_below_mb: 100,
            ..Default::default()
        };
        assert!(bad.validate().unwrap_err().contains("higher"));
        let bad = DiskGuardPolicy {
            reserve_mb: 5000,
            ..Default::default()
        };
        assert!(bad.validate().unwrap_err().contains("reserve"));
    }

    #[test]
    fn the_disk_is_read_at_most_once_a_minute() {
        let mut c = SpaceCache::default();
        let mut reads = 0;
        let mut get = |c: &mut SpaceCache, at: u64| {
            c.get("/", Timestamp(at), || {
                reads += 1;
                Some((reads, 100))
            })
        };
        assert_eq!(get(&mut c, 1_000), Some((1, 100)));
        assert_eq!(get(&mut c, 30_000), Some((1, 100)), "answered from memory");
        assert_eq!(get(&mut c, 59_999), Some((1, 100)));
        assert_eq!(get(&mut c, 61_000), Some((2, 100)), "a minute later it is read again");
        assert_eq!(get(&mut c, 62_000), Some((2, 100)), "and then again from memory");
        // A failed read is remembered and not retried inside the gap.
        let mut c = SpaceCache::default();
        assert_eq!(c.get("x", Timestamp(0), || None), None);
        assert_eq!(c.get("x", Timestamp(10), || Some((1, 1))), None);
        // The memory is bounded.
        for i in 0..200 {
            c.get(&format!("d{i}"), Timestamp(0), || Some((1, 1)));
        }
        assert!(c.entries.len() <= MAX_DISKS);
    }
}
