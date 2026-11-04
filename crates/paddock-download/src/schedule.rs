//! The generic parts of a downloaded data source that is kept up to date: when it may be fetched again (a floor that also
//! holds for "Update now"), when an automatic run is due (once after a sleep, not once per missed interval), whether a fresh copy
//! looks broken (a large drop in entries), and how a new file replaces the last good one (the previous copy stays). Pure
//! functions over plain numbers and a folder; time is passed in (`now_ms`, milliseconds since the Unix epoch), so tests drive a
//! fake clock. Used by Dyno's list subscriptions and location databases; the helper's tool download and Sighter's weights need
//! the same.
//!
//! The last attempt time belongs **outside** the cache folder (a cleared cache must not allow an early fetch): [`FetchRecord`]
//! is the small piece of state to keep there.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What to keep about one source between runs, outside its cache folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FetchRecord {
    /// When a fetch was last tried; the floor counts from here, so a failed try also waits.
    pub last_attempt_ms: Option<i64>,
    /// When a copy was last taken.
    pub last_fetched_ms: Option<i64>,
    /// The publisher's validator, for a conditional request when the transport offers one.
    pub etag: Option<String>,
}

impl FetchRecord {
    /// Notes an attempt at `now_ms`.
    pub fn attempted(&mut self, now_ms: i64) {
        self.last_attempt_ms = Some(now_ms);
    }

    /// Notes a copy taken at `now_ms`.
    pub fn fetched(&mut self, now_ms: i64) {
        self.last_fetched_ms = Some(now_ms);
    }

    /// When the next attempt is allowed, or `None` when now is fine (see [`next_allowed`]).
    pub fn next_allowed(&self, now_ms: i64, floor: Duration) -> Option<i64> {
        next_allowed(self.last_attempt_ms, now_ms, floor)
    }
}

fn millis(d: Duration) -> i64 {
    i64::try_from(d.as_millis()).unwrap_or(i64::MAX)
}

/// When a source may be fetched again (ms since the epoch), or `None` when it may be fetched now: `floor` after the last attempt.
/// A clock that moved back (the last attempt in the future) never locks the source for more than one `floor` from now.
pub fn next_allowed(last_attempt_ms: Option<i64>, now_ms: i64, floor: Duration) -> Option<i64> {
    let last = last_attempt_ms?;
    if last > now_ms {
        return Some(now_ms.saturating_add(millis(floor)));
    }
    let at = last.saturating_add(millis(floor));
    (at > now_ms).then_some(at)
}

/// Whether an automatic run is due: never tried, or `every` (at least `floor`) has passed since the last attempt. A computer that
/// slept through ten intervals is due once.
pub fn automatic_due(
    last_attempt_ms: Option<i64>,
    now_ms: i64,
    every: Duration,
    floor: Duration,
) -> bool {
    next_allowed(last_attempt_ms, now_ms, every.max(floor)).is_none()
}

/// Whether a fresh list of `new_entries` is a large drop against the last good copy of `old_entries`: it emptied a list that had
/// entries, or fell below `keep_percent` of a list of at least `min_old` entries. The caller then keeps the old copy.
pub fn is_large_drop(old_entries: u64, new_entries: u64, keep_percent: u64, min_old: u64) -> bool {
    (new_entries == 0 && old_entries > 0)
        || (old_entries >= min_old
            && new_entries.saturating_mul(100) < old_entries.saturating_mul(keep_percent))
}

/// Moves `from` over `to` (Windows cannot rename onto an existing file).
pub fn replace(from: &Path, to: &Path) -> io::Result<()> {
    if cfg!(windows) && to.exists() {
        std::fs::remove_file(to)?;
    }
    std::fs::rename(from, to)
}

/// Makes the verified file `staged` the current copy `current` in its folder: the copy that was current becomes `previous` (the
/// one before it is dropped). Nothing is left half done: `staged` is moved last, with one rename. Both names are in one folder.
pub fn swap_in(staged: &Path, current: &Path, previous: &Path) -> io::Result<()> {
    if !staged.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "the staged file is missing",
        ));
    }
    if current.is_file() {
        replace(current, previous)?;
    }
    replace(staged, current)
}

/// The path of the staging file for `current` in the same folder (`list.txt` gives `list.txt.staging`), where a download is
/// written and checked before [`swap_in`].
pub fn staging_path(current: &Path) -> PathBuf {
    let mut name = current
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".staging");
    current.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = 3_600_000;
    const HOUR: Duration = Duration::from_secs(3600);

    #[test]
    fn the_floor_counts_from_the_last_attempt() {
        assert_eq!(next_allowed(None, 1_000, HOUR), None);
        assert_eq!(next_allowed(Some(0), 30 * 60_000, HOUR), Some(H));
        assert_eq!(
            next_allowed(Some(0), H, HOUR),
            None,
            "exactly at the floor is allowed"
        );
        assert_eq!(next_allowed(Some(0), H - 1, HOUR), Some(H));
    }

    #[test]
    fn a_clock_that_moved_back_waits_one_floor_at_most() {
        assert_eq!(next_allowed(Some(10 * H), H, HOUR), Some(2 * H));
    }

    #[test]
    fn huge_values_saturate() {
        assert_eq!(
            next_allowed(Some(i64::MAX - 1), i64::MAX - 1, HOUR),
            Some(i64::MAX)
        );
        assert_eq!(next_allowed(Some(0), 0, Duration::MAX), Some(i64::MAX));
    }

    #[test]
    fn automatic_runs_follow_the_longer_of_the_interval_and_the_floor() {
        let day = Duration::from_secs(86_400);
        assert!(automatic_due(None, 0, day, HOUR));
        assert!(!automatic_due(Some(0), 23 * H, day, HOUR));
        assert!(automatic_due(Some(0), 24 * H, day, HOUR));
        assert!(
            automatic_due(Some(0), 240 * H, day, HOUR),
            "slept through ten days: due once"
        );
        assert!(
            !automatic_due(Some(0), 30 * 60_000, Duration::from_secs(60), HOUR),
            "never below the floor"
        );
    }

    #[test]
    fn a_large_drop_is_told_from_a_small_change() {
        assert!(is_large_drop(1200, 3, 10, 10));
        assert!(is_large_drop(1200, 0, 10, 10));
        assert!(!is_large_drop(1200, 200, 10, 10));
        assert!(
            !is_large_drop(1200, 120, 10, 10),
            "exactly the share is kept"
        );
        assert!(is_large_drop(1200, 119, 10, 10));
        assert!(!is_large_drop(5, 1, 10, 10), "tiny lists may change freely");
        assert!(is_large_drop(5, 0, 10, 10), "but not become empty");
        assert!(!is_large_drop(0, 0, 10, 10));
    }

    #[test]
    fn the_record_tracks_attempts_and_round_trips() {
        let mut r = FetchRecord::default();
        assert_eq!(r.next_allowed(5, HOUR), None);
        r.attempted(0);
        r.fetched(0);
        assert_eq!(r.next_allowed(60_000, HOUR), Some(H));
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("lastAttemptMs"));
        assert_eq!(serde_json::from_str::<FetchRecord>(&json).unwrap(), r);
        assert_eq!(
            serde_json::from_str::<FetchRecord>("{}").unwrap(),
            FetchRecord::default(),
            "missing fields give defaults"
        );
    }

    #[test]
    fn a_swap_keeps_the_previous_copy_and_leaves_no_staging_file() {
        let dir = tempfile::tempdir().unwrap();
        let cur = dir.path().join("list.txt");
        let prev = dir.path().join("previous.txt");
        let st = staging_path(&cur);
        assert_eq!(st.file_name().unwrap(), "list.txt.staging");
        std::fs::write(&st, "one").unwrap();
        swap_in(&st, &cur, &prev).unwrap();
        assert_eq!(std::fs::read_to_string(&cur).unwrap(), "one");
        assert!(
            !prev.exists() && !st.exists(),
            "first copy: nothing to keep"
        );
        std::fs::write(&st, "two").unwrap();
        swap_in(&st, &cur, &prev).unwrap();
        std::fs::write(&st, "three").unwrap();
        swap_in(&st, &cur, &prev).unwrap();
        assert_eq!(std::fs::read_to_string(&cur).unwrap(), "three");
        assert_eq!(
            std::fs::read_to_string(&prev).unwrap(),
            "two",
            "only one older copy is kept"
        );
        assert!(!st.exists());
    }

    #[test]
    fn a_missing_staging_file_is_refused_and_the_current_copy_stays_where_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let cur = dir.path().join("a");
        std::fs::write(&cur, "x").unwrap();
        let e = swap_in(&dir.path().join("none"), &cur, &dir.path().join("p")).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
        assert_eq!(std::fs::read_to_string(&cur).unwrap(), "x");
        assert!(!dir.path().join("p").exists());
    }

    #[test]
    fn a_file_name_less_path_still_gets_a_staging_name() {
        assert_eq!(staging_path(Path::new("/")), Path::new("/.staging"));
    }
}
