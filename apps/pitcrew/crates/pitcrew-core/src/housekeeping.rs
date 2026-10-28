//! Disk housekeeping: what Pitcrew may delete in a runner's folder, and when.
//!
//! A runner's folder grows: job work folders (`work/<hash>`, which the runner's own stale-directory cleaning often
//! fails on, e.g. "failed to clean stale directory … Access is denied" for read-only or locked files), the action
//! cache, leftover downloads, old binaries and rotated logs. The service lists the folder into [`DiskEntry`]s, asks
//! [`due`] whether to clean and [`plan`] what to delete, and carries the plan out **only while the runner runs no
//! job** (or is stopped or drained): nothing a running job uses is ever touched. Files it cannot delete are reported
//! and tried again next time; they never fail the cleanup.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Most entries one scan reports (bigger folders are cut; the biggest work entries come first in the scan).
pub const MAX_ENTRIES: usize = 4096;
/// One MB (storage steps: 1000 bytes per KB, as the Finder).
const MB: u64 = 1_000_000;
/// Work and cache entries changed this recently are never planned for deletion (a job may have just started).
pub const RECENT_GRACE: Duration = Duration::from_secs(600);

/// The cleanup rules (defaults for every runner; a runner may override them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CleanupPolicy {
    /// Clean on the schedule and under disk pressure by itself (manual cleaning always works).
    pub automatic: bool,
    /// Hours between scheduled cleanups (at least 1).
    pub every_hours: u32,
    /// Work folders untouched for this many hours are deleted on a scheduled cleanup.
    pub work_max_age_hours: u32,
    /// The action cache is kept below this size (MB; 0 = no cap).
    pub cache_cap_mb: u64,
    /// Clean when the runner's folder is larger than this (MB; 0 = no cap).
    pub folder_cap_mb: u64,
    /// Clean (and warn) when the disk has less free space than this (MB).
    pub min_free_mb: u64,
    /// Keep the previous runner binary for roll-back.
    pub keep_previous: bool,
}

impl Default for CleanupPolicy {
    fn default() -> Self {
        CleanupPolicy {
            automatic: true,
            every_hours: 24,
            work_max_age_hours: 72,
            cache_cap_mb: 10_000,
            folder_cap_mb: 50_000,
            min_free_mb: 10_000,
            keep_previous: true,
        }
    }
}

/// What a scanned entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    /// A job's work folder (`work/<hash>`).
    Work,
    /// An entry of the action cache.
    Cache,
    /// A leftover download (`*.download`, `*_new.exe`).
    Download,
    /// The previous binary, kept for roll-back.
    Previous,
    /// A binary of an older runner name (`act_runner[.exe]` next to `gitea-runner[.exe]`).
    OldBinary,
    /// A rotated log file `<log>.<n>`.
    RotatedLog {
        /// Its number.
        n: u32,
    },
}

/// One entry of a runner's folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskEntry {
    /// Its path relative to the runner's folder (`/`-separated).
    pub path: String,
    /// What it is.
    pub kind: EntryKind,
    /// Its size (recursive for folders).
    pub bytes: u64,
    /// When it last changed.
    pub modified: Timestamp,
}

/// A runner folder's scan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskScan {
    /// The entries Pitcrew may clean.
    pub entries: Vec<DiskEntry>,
    /// The whole folder's size.
    pub folder_bytes: u64,
    /// Free space on its disk, when known.
    pub free_bytes: Option<u64>,
    /// The scan stopped at [`MAX_ENTRIES`] or a time limit; sizes are lower bounds.
    pub truncated: bool,
}

impl DiskScan {
    /// Bytes of entries of a kind.
    pub fn bytes_of(&self, pred: impl Fn(EntryKind) -> bool) -> u64 {
        self.entries.iter().filter(|e| pred(e.kind)).map(|e| e.bytes).sum()
    }
}

/// What started a cleanup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CleanupTrigger {
    /// The schedule.
    Schedule,
    /// Free disk space fell below the minimum.
    LowDisk,
    /// The runner's folder grew past its cap.
    FolderCap,
    /// The user.
    Manual,
}

/// Why an entry is deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeleteReason {
    /// A leftover download or an old binary.
    Leftover,
    /// More rotated logs than the rotation keeps.
    OldLog,
    /// A work folder older than the age limit.
    Aged,
    /// The cache is over its cap.
    CacheCap,
    /// Disk space or the folder cap needs it.
    Pressure,
}

/// One planned deletion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deletion {
    /// The entry.
    pub entry: DiskEntry,
    /// Why.
    pub reason: DeleteReason,
}

/// What a cleanup will delete (shown as a preview before it runs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlan {
    /// What started it.
    pub trigger: CleanupTrigger,
    /// The deletions, in order.
    pub deletions: Vec<Deletion>,
    /// Bytes they free.
    pub freed_bytes: u64,
    /// Bytes still needed to get back under the disk or folder limits after the plan (0 when it suffices).
    pub still_needed_bytes: u64,
}

/// An entry that could not be deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupFailure {
    /// The entry's path relative to the runner's folder.
    pub path: String,
    /// Why, for the user (e.g. "a file is in use by another program").
    pub reason: String,
}

/// How a cleanup went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupReport {
    /// What started it.
    pub trigger: CleanupTrigger,
    /// When it ended.
    pub at: Timestamp,
    /// Bytes freed.
    pub freed_bytes: u64,
    /// Entries deleted.
    pub deleted: u32,
    /// Entries that could not be deleted.
    pub failures: Vec<CleanupFailure>,
}

/// Bytes to free to get back under the free-space minimum and the folder cap.
pub fn bytes_needed(policy: &CleanupPolicy, scan: &DiskScan) -> u64 {
    let for_disk = scan
        .free_bytes
        .map_or(0, |free| (policy.min_free_mb.saturating_mul(MB)).saturating_sub(free));
    let for_folder = if policy.folder_cap_mb == 0 {
        0
    } else {
        scan.folder_bytes.saturating_sub(policy.folder_cap_mb.saturating_mul(MB))
    };
    for_disk.max(for_folder)
}

/// Whether the disk is low on space for this policy.
pub fn disk_low(policy: &CleanupPolicy, free_bytes: Option<u64>) -> bool {
    free_bytes.is_some_and(|f| f < policy.min_free_mb.saturating_mul(MB))
}

/// Whether an automatic cleanup is due now (`None`: not now). Pressure wins over the schedule; a cleanup that
/// just ran is not repeated within `min_gap` even under pressure (it may have freed all it could).
pub fn due(
    policy: &CleanupPolicy,
    scan: &DiskScan,
    last_cleanup: Option<Timestamp>,
    now: Timestamp,
    min_gap: Duration,
) -> Option<CleanupTrigger> {
    if !policy.automatic {
        return None;
    }
    if last_cleanup.is_some_and(|t| now.since(t) < min_gap) {
        return None;
    }
    if disk_low(policy, scan.free_bytes) {
        return Some(CleanupTrigger::LowDisk);
    }
    if policy.folder_cap_mb > 0 && scan.folder_bytes > policy.folder_cap_mb.saturating_mul(MB) {
        return Some(CleanupTrigger::FolderCap);
    }
    let every = Duration::from_secs(u64::from(policy.every_hours.max(1)) * 3600);
    match last_cleanup {
        Some(t) if now.since(t) < every => None,
        _ => Some(CleanupTrigger::Schedule),
    }
}

/// What to delete. `keep_logs` is the log rotation's `keep`; `failed_recently` are paths that failed last time
/// (planned again — the cause may be gone — but last, so they don't hide what can be freed).
pub fn plan(policy: &CleanupPolicy, scan: &DiskScan, trigger: CleanupTrigger, keep_logs: u32, now: Timestamp) -> CleanupPlan {
    let mut chosen: Vec<Deletion> = vec![];
    let mut taken = vec![false; scan.entries.len()];
    for (i, e) in scan.entries.iter().enumerate() {
        if matches!(e.kind, EntryKind::Work | EntryKind::Cache) && now.since(e.modified) < RECENT_GRACE {
            taken[i] = true;
        }
    }
    let mut take = |i: usize, reason: DeleteReason, chosen: &mut Vec<Deletion>| {
        if !taken[i] {
            taken[i] = true;
            chosen.push(Deletion {
                entry: scan.entries[i].clone(),
                reason,
            });
        }
    };

    // Always safe: leftovers and logs beyond the rotation.
    for (i, e) in scan.entries.iter().enumerate() {
        match e.kind {
            EntryKind::Download | EntryKind::OldBinary => take(i, DeleteReason::Leftover, &mut chosen),
            EntryKind::RotatedLog { n } if n > keep_logs => take(i, DeleteReason::OldLog, &mut chosen),
            EntryKind::Previous if !policy.keep_previous => take(i, DeleteReason::Leftover, &mut chosen),
            _ => {}
        }
    }
    // Work folders past the age limit.
    let max_age = Duration::from_secs(u64::from(policy.work_max_age_hours) * 3600);
    for (i, e) in scan.entries.iter().enumerate() {
        if e.kind == EntryKind::Work && now.since(e.modified) >= max_age {
            take(i, DeleteReason::Aged, &mut chosen);
        }
    }
    // The cache cap: oldest first.
    let mut by_age: Vec<usize> = (0..scan.entries.len()).collect();
    by_age.sort_by_key(|&i| scan.entries[i].modified);
    if policy.cache_cap_mb > 0 {
        let cap = policy.cache_cap_mb.saturating_mul(MB);
        let mut cache = scan.bytes_of(|k| k == EntryKind::Cache);
        for &i in &by_age {
            if cache <= cap {
                break;
            }
            if scan.entries[i].kind == EntryKind::Cache {
                cache = cache.saturating_sub(scan.entries[i].bytes);
                take(i, DeleteReason::CacheCap, &mut chosen);
            }
        }
    }
    // Pressure (and a manual cleanup, which frees what it can short of the previous binary): oldest work folders,
    // then the oldest cache entries, until the limits are met.
    let freed = |chosen: &[Deletion]| chosen.iter().map(|d| d.entry.bytes).sum::<u64>();
    let needed = bytes_needed(policy, scan);
    let all = trigger == CleanupTrigger::Manual;
    for kind in [EntryKind::Work, EntryKind::Cache] {
        for &i in &by_age {
            if !all && freed(&chosen) >= needed {
                break;
            }
            if scan.entries[i].kind == kind {
                take(i, DeleteReason::Pressure, &mut chosen);
            }
        }
    }
    let freed_bytes = freed(&chosen);
    CleanupPlan {
        trigger,
        deletions: chosen,
        freed_bytes,
        still_needed_bytes: needed.saturating_sub(freed_bytes),
    }
}

/// Classifies a file name found in a runner's binary folder: `binary` is the kind's binary file name.
pub fn classify_binary_file(name: &str, binary: &str) -> Option<EntryKind> {
    let lower = name.to_ascii_lowercase();
    let stem = binary.trim_end_matches(".exe");
    if lower == format!("{binary}.previous") {
        Some(EntryKind::Previous)
    } else if lower.ends_with(".download") || lower == format!("{stem}_new.exe") || lower == format!("{stem}_new") {
        Some(EntryKind::Download)
    } else if (lower == "act_runner.exe" || lower == "act_runner") && stem != "act_runner" {
        Some(EntryKind::OldBinary)
    } else {
        None
    }
}

/// The rotation number of `<log>.<n>` (`None` for the live log and other files).
pub fn rotated_log_number(name: &str, log: &str) -> Option<u32> {
    name.strip_prefix(log)?.strip_prefix('.')?.parse().ok().filter(|n| *n > 0)
}

/// A user-facing reason for an OS error while deleting (`code`: the raw OS error; `windows`: whose codes they are).
pub fn failure_reason(code: Option<i32>, windows: bool, fallback: &str) -> String {
    let retry = "; Pitcrew tries again at the next cleanup";
    let what = match (windows, code) {
        (true, Some(5)) | (false, Some(13)) => "access is denied (a file is read-only, in use or owned by another user)",
        (false, Some(1)) => "the operation is not permitted",
        (true, Some(32 | 33)) | (false, Some(16)) => "a file is in use by another program",
        (true, Some(2 | 3)) | (false, Some(2)) => return "it was already gone".into(),
        (true, Some(206)) | (false, Some(36)) => "a path inside is too long even for long-path deletion",
        _ => fallback,
    };
    format!("{what}{retry}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: u64 = 3600 * 1000;

    fn e(path: &str, kind: EntryKind, mb: u64, age_h: u64, now: Timestamp) -> DiskEntry {
        DiskEntry {
            path: path.into(),
            kind,
            bytes: mb * MB,
            modified: Timestamp(now.0 - age_h * HOUR),
        }
    }

    fn scan(now: Timestamp) -> DiskScan {
        DiskScan {
            entries: vec![
                e("bin/gitea-runner.exe.download", EntryKind::Download, 18, 1, now),
                e("act_runner.exe", EntryKind::OldBinary, 19, 100, now),
                e("bin/gitea-runner.exe.previous", EntryKind::Previous, 18, 10, now),
                e("logs/runner.log.6", EntryKind::RotatedLog { n: 6 }, 10, 5, now),
                e("logs/runner.log.2", EntryKind::RotatedLog { n: 2 }, 10, 5, now),
                e("work/old", EntryKind::Work, 500, 100, now),
                e("work/recent", EntryKind::Work, 300, 2, now),
                e("work/newest", EntryKind::Work, 200, 1, now),
                e("cache/a", EntryKind::Cache, 6000, 50, now),
                e("cache/b", EntryKind::Cache, 6000, 5, now),
            ],
            folder_bytes: 13_100 * MB,
            free_bytes: Some(100_000 * MB),
            truncated: false,
        }
    }

    fn paths(p: &CleanupPlan) -> Vec<&str> {
        p.deletions.iter().map(|d| d.entry.path.as_str()).collect()
    }

    #[test]
    fn a_scheduled_cleanup_removes_leftovers_old_logs_aged_work_and_cache_over_its_cap() {
        let now = Timestamp(1000 * HOUR);
        let p = plan(&CleanupPolicy::default(), &scan(now), CleanupTrigger::Schedule, 5, now);
        assert_eq!(
            paths(&p),
            vec![
                "bin/gitea-runner.exe.download",
                "act_runner.exe",
                "logs/runner.log.6",
                "work/old",
                "cache/a"
            ]
        );
        assert_eq!(p.deletions[3].reason, DeleteReason::Aged);
        assert_eq!(p.deletions[4].reason, DeleteReason::CacheCap);
        assert_eq!(p.freed_bytes, (18 + 19 + 10 + 500 + 6000) * MB);
        assert_eq!(p.still_needed_bytes, 0);
        assert!(
            !paths(&p).contains(&"bin/gitea-runner.exe.previous"),
            "the roll-back binary is kept"
        );
    }

    #[test]
    fn pressure_frees_the_oldest_work_then_cache_until_enough() {
        let now = Timestamp(1000 * HOUR);
        let mut s = scan(now);
        let policy = CleanupPolicy {
            cache_cap_mb: 0,
            work_max_age_hours: 1000,
            min_free_mb: 1000,
            ..CleanupPolicy::default()
        };
        // 1000 MB minimum, 300 MB free: 700 MB needed; leftovers give 47, then work/old (500), then work/recent (300).
        s.free_bytes = Some(300 * MB);
        let p = plan(&policy, &s, CleanupTrigger::LowDisk, 5, now);
        assert_eq!(
            paths(&p),
            vec![
                "bin/gitea-runner.exe.download",
                "act_runner.exe",
                "logs/runner.log.6",
                "work/old",
                "work/recent"
            ]
        );
        assert_eq!(p.still_needed_bytes, 0);
        // Not enough to free: the rest is reported.
        s.free_bytes = Some(0);
        let policy = CleanupPolicy {
            min_free_mb: 100_000,
            ..policy
        };
        let p = plan(&policy, &s, CleanupTrigger::LowDisk, 5, now);
        assert!(p.still_needed_bytes > 0);
        assert!(paths(&p).contains(&"cache/b"));
    }

    #[test]
    fn a_manual_cleanup_frees_everything_but_the_previous_binary_and_kept_logs() {
        let now = Timestamp(1000 * HOUR);
        let p = plan(&CleanupPolicy::default(), &scan(now), CleanupTrigger::Manual, 5, now);
        assert_eq!(p.deletions.len(), 8, "the kept logs stay too");
        let mut fresh = scan(now);
        fresh.entries.push(e("work/just-started", EntryKind::Work, 900, 0, now));
        let p = plan(&CleanupPolicy::default(), &fresh, CleanupTrigger::Manual, 5, now);
        assert!(
            !paths(&p).contains(&"work/just-started"),
            "a folder changed minutes ago is left alone"
        );
        let p = plan(
            &CleanupPolicy {
                keep_previous: false,
                ..CleanupPolicy::default()
            },
            &scan(now),
            CleanupTrigger::Manual,
            5,
            now,
        );
        assert_eq!(p.deletions.len(), 9);
    }

    #[test]
    fn cleanups_are_due_on_pressure_or_schedule() {
        let now = Timestamp(1000 * HOUR);
        let p = CleanupPolicy::default();
        let gap = Duration::from_secs(600);
        let mut s = scan(now);
        assert_eq!(due(&p, &s, None, now, gap), Some(CleanupTrigger::Schedule));
        assert_eq!(due(&p, &s, Some(Timestamp(now.0 - HOUR)), now, gap), None);
        assert_eq!(
            due(&p, &s, Some(Timestamp(now.0 - 25 * HOUR)), now, gap),
            Some(CleanupTrigger::Schedule)
        );
        s.free_bytes = Some(MB);
        assert_eq!(due(&p, &s, Some(Timestamp(now.0 - HOUR)), now, gap), Some(CleanupTrigger::LowDisk));
        assert_eq!(due(&p, &s, Some(Timestamp(now.0 - 60_000)), now, gap), None, "not again right away");
        s.free_bytes = None;
        s.folder_bytes = 60_000 * MB;
        assert_eq!(
            due(&p, &s, Some(Timestamp(now.0 - HOUR)), now, gap),
            Some(CleanupTrigger::FolderCap)
        );
        let off = CleanupPolicy { automatic: false, ..p };
        assert_eq!(due(&off, &s, None, now, gap), None);
        let uncapped = CleanupPolicy { folder_cap_mb: 0, ..p };
        assert_eq!(bytes_needed(&uncapped, &s), 0);
        assert!(!disk_low(&p, None));
    }

    #[test]
    fn binary_and_log_files_are_recognised() {
        let b = "gitea-runner.exe";
        assert_eq!(classify_binary_file("gitea-runner.exe.previous", b), Some(EntryKind::Previous));
        assert_eq!(classify_binary_file("gitea-runner.exe.download", b), Some(EntryKind::Download));
        assert_eq!(classify_binary_file("gitea-runner_new.exe", b), Some(EntryKind::Download));
        assert_eq!(classify_binary_file("act_runner.exe", b), Some(EntryKind::OldBinary));
        assert_eq!(classify_binary_file("gitea-runner.exe", b), None);
        assert_eq!(classify_binary_file("config.yaml", b), None);
        assert_eq!(classify_binary_file("gitea-runner_new", "gitea-runner"), Some(EntryKind::Download));
        assert_eq!(rotated_log_number("runner.log.3", "runner.log"), Some(3));
        assert_eq!(rotated_log_number("runner.log", "runner.log"), None);
        assert_eq!(rotated_log_number("runner.log.0", "runner.log"), None);
        assert_eq!(rotated_log_number("runner.log.x", "runner.log"), None);
        assert_eq!(rotated_log_number("other.log.1", "runner.log"), None);
    }

    #[test]
    fn failures_say_what_happened_and_that_it_is_retried() {
        assert!(failure_reason(Some(32), true, "x").contains("in use"));
        assert!(failure_reason(Some(16), false, "x").contains("in use"));
        assert!(failure_reason(Some(5), true, "x").contains("access is denied"));
        assert!(failure_reason(Some(13), false, "x").contains("access is denied"));
        assert!(failure_reason(Some(1), false, "x").contains("not permitted"));
        assert_eq!(failure_reason(Some(3), true, "x"), "it was already gone");
        assert!(failure_reason(Some(206), true, "x").contains("too long"));
        assert!(failure_reason(Some(5), false, "odd").starts_with("odd;"));
        assert!(failure_reason(None, true, "odd").ends_with("next cleanup"));
        assert_eq!(scan(Timestamp(1000 * HOUR)).bytes_of(|k| k == EntryKind::Cache), 12_000 * MB);
    }
}
