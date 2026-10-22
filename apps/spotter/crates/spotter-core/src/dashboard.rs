//! The dashboard's numbers, computed from the watches' state and the journal: what is new, release cadence,
//! releases over time, source reliability and downloads, bucketed for a range. The UI only draws them.

use crate::journal::{Journal, DAY_MS};
use crate::release::Release;
use crate::state::WatchState;
use crate::time::Timestamp;
use crate::watch::WatchId;

/// The time range of the charts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    /// 30 days in daily buckets.
    Days30,
    /// 90 days in daily buckets.
    Days90,
    /// A year in weekly buckets.
    Year,
}

impl Range {
    /// From `30d`, `90d`, `1y`.
    pub fn parse(s: &str) -> Option<Range> {
        match s {
            "30d" => Some(Range::Days30),
            "90d" => Some(Range::Days90),
            "1y" => Some(Range::Year),
            _ => None,
        }
    }

    fn days(self) -> i64 {
        match self {
            Range::Days30 => 30,
            Range::Days90 => 90,
            Range::Year => 364,
        }
    }

    fn bucket_days(self) -> i64 {
        match self {
            Range::Year => 7,
            _ => 1,
        }
    }
}

/// Bucket starts for a range ending at `now` (UTC days), oldest first.
pub fn buckets(range: Range, now: Timestamp) -> Vec<Timestamp> {
    let today = now.0.div_euclid(DAY_MS);
    let n = range.days() / range.bucket_days();
    (0..n)
        .map(|i| Timestamp((today - (n - 1 - i) * range.bucket_days()) * DAY_MS))
        .collect()
}

fn bucket_of(range: Range, start: &[Timestamp], at: Timestamp) -> Option<usize> {
    let first = start.first()?.0;
    if at.0 < first {
        return None;
    }
    let i = ((at.0 - first) / (range.bucket_days() * DAY_MS)) as usize;
    (i < start.len()).then_some(i)
}

/// One watch's input.
pub struct WatchInput<'a> {
    /// Its id.
    pub id: &'a WatchId,
    /// Its state.
    pub state: &'a WatchState,
}

/// A recent release for the "What's new" hero.
#[derive(Debug, Clone, PartialEq)]
pub struct NewItem {
    /// The watch.
    pub watch: WatchId,
    /// The release.
    pub release: Release,
    /// When it came out (published, else first seen).
    pub released_at: Timestamp,
    /// Newer than the installed version (`None` when that is unknown).
    pub newer_than_installed: Option<bool>,
    /// Newer than the acknowledged version (not seen yet).
    pub unseen: bool,
}

/// One watch's numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct WatchNumbers {
    /// The watch.
    pub watch: WatchId,
    /// Average time between its releases, from the known history.
    pub average_interval_ms: Option<i64>,
    /// Time since its newest release.
    pub since_last_ms: Option<i64>,
    /// Releases per bucket in the range.
    pub releases: Vec<u32>,
    /// Failed checks per bucket.
    pub failed_checks: Vec<u32>,
    /// All checks per bucket.
    pub checks: Vec<u32>,
    /// Releases per bucket over the last 12 weeks, for the tile's sparkline.
    pub sparkline: Vec<u32>,
}

/// Downloads in one bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DownloadBucket {
    /// Downloads that worked.
    pub count: u32,
    /// Downloads that failed.
    pub failed: u32,
    /// Bytes downloaded.
    pub bytes: u64,
}

/// The dashboard.
#[derive(Debug, Clone, PartialEq)]
pub struct Dashboard {
    /// The newest releases across watches, newest first.
    pub whats_new: Vec<NewItem>,
    /// Bucket starts.
    pub buckets: Vec<Timestamp>,
    /// Per watch.
    pub watches: Vec<WatchNumbers>,
    /// Downloads per bucket.
    pub downloads: Vec<DownloadBucket>,
}

/// Computes the dashboard. `limit` caps "What's new".
pub fn compute(watches: &[WatchInput<'_>], journal: &Journal, range: Range, now: Timestamp, limit: usize) -> Dashboard {
    let starts = buckets(range, now);
    let weeks = buckets(Range::Year, now);
    let spark_start = weeks.len().saturating_sub(12);
    let mut whats_new = Vec::new();
    let mut numbers = Vec::new();
    for w in watches {
        let s = w.state;
        let mut releases = vec![0u32; starts.len()];
        let mut sparkline = vec![0u32; 12];
        let mut times: Vec<i64> = Vec::new();
        for k in &s.history {
            let at = k.released_at();
            times.push(at.0);
            if let Some(i) = bucket_of(range, &starts, at) {
                releases[i] += 1;
            }
            if let Some(i) = bucket_of(Range::Year, &weeks, at).and_then(|i| i.checked_sub(spark_start)) {
                sparkline[i] += 1;
            }
            whats_new.push(NewItem {
                watch: w.id.clone(),
                release: k.release.clone(),
                released_at: at,
                newer_than_installed: s.installed.as_ref().map(|i| k.release.version > *i),
                unseen: s.acknowledged.as_ref().is_none_or(|a| k.release.version > *a),
            });
        }
        times.sort_unstable();
        let average_interval_ms = (times.len() >= 2).then(|| (times[times.len() - 1] - times[0]) / (times.len() as i64 - 1));
        let mut failed_checks = vec![0u32; starts.len()];
        let mut checks = vec![0u32; starts.len()];
        for (day, c) in journal.checks.get(w.id).into_iter().flatten() {
            if let Some(i) = bucket_of(range, &starts, Timestamp(day * DAY_MS)) {
                failed_checks[i] += c.failed;
                checks[i] += c.ok + c.failed;
            }
        }
        numbers.push(WatchNumbers {
            watch: w.id.clone(),
            average_interval_ms,
            since_last_ms: times.last().map(|t| (now.0 - t).max(0)),
            releases,
            failed_checks,
            checks,
            sparkline,
        });
    }
    whats_new.sort_by(|a, b| {
        b.released_at
            .cmp(&a.released_at)
            .then_with(|| b.release.version.cmp(&a.release.version))
    });
    whats_new.truncate(limit);
    let mut downloads = vec![DownloadBucket::default(); starts.len()];
    for d in &journal.downloads {
        if let Some(i) = bucket_of(range, &starts, d.at) {
            if d.ok {
                downloads[i].count += 1;
                downloads[i].bytes += d.size;
            } else {
                downloads[i].failed += 1;
            }
        }
    }
    Dashboard {
        whats_new,
        buckets: starts,
        watches: numbers,
        downloads,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::DownloadRecord;
    use crate::state::KnownRelease;
    use crate::version::VersionScheme;

    fn known(v: &str, day: i64) -> KnownRelease {
        KnownRelease {
            release: Release {
                version: VersionScheme::Semver.extract(v).unwrap(),
                name: v.into(),
                location: "l".into(),
                published_at: None,
                size: None,
                sha256: None,
            },
            first_seen: Timestamp(day * DAY_MS + 1000),
        }
    }

    #[test]
    fn ranges_and_buckets() {
        assert_eq!(Range::parse("30d"), Some(Range::Days30));
        assert_eq!(Range::parse("90d"), Some(Range::Days90));
        assert_eq!(Range::parse("1y"), Some(Range::Year));
        assert_eq!(Range::parse("2y"), None);
        let now = Timestamp(1000 * DAY_MS + 5);
        let b = buckets(Range::Days30, now);
        assert_eq!((b.len(), b[29], b[0]), (30, Timestamp(1000 * DAY_MS), Timestamp(971 * DAY_MS)));
        assert_eq!(buckets(Range::Days90, now).len(), 90);
        let y = buckets(Range::Year, now);
        assert_eq!((y.len(), y[51]), (52, Timestamp(1000 * DAY_MS)));
        assert_eq!(bucket_of(Range::Days30, &b, Timestamp(0)), None);
        assert_eq!(bucket_of(Range::Days30, &b, Timestamp(2000 * DAY_MS)), None);
        assert_eq!(bucket_of(Range::Days30, &[], now), None);
    }

    #[test]
    fn the_dashboard_counts_releases_checks_and_downloads() {
        let now = Timestamp(1000 * DAY_MS + 5);
        let a = WatchId::parse("a").unwrap();
        let b = WatchId::parse("b").unwrap();
        let mut sa = WatchState {
            history: vec![known("1.2.0", 999), known("1.1.0", 990), known("1.0.0", 900)],
            ..WatchState::default()
        };
        sa.acknowledged = Some(VersionScheme::Semver.extract("1.1.0").unwrap());
        sa.installed = Some(VersionScheme::Semver.extract("1.0.0").unwrap());
        let sb = WatchState::default();
        let mut j = Journal::default();
        j.count_check(&a, Timestamp(999 * DAY_MS), false);
        j.count_check(&a, Timestamp(999 * DAY_MS), true);
        j.count_check(&a, Timestamp(10 * DAY_MS), true);
        for (ok, size, day) in [(true, 100, 1000), (false, 0, 1000), (true, 5, 1)] {
            j.record_download(DownloadRecord {
                at: Timestamp(day * DAY_MS),
                watch: a.clone(),
                version: "1".into(),
                size,
                ok,
            });
        }
        let inputs = [WatchInput { id: &a, state: &sa }, WatchInput { id: &b, state: &sb }];
        let d = compute(&inputs, &j, Range::Days30, now, 2);
        assert_eq!(d.whats_new.len(), 2);
        assert_eq!(d.whats_new[0].release.name, "1.2.0");
        assert!(d.whats_new[0].unseen && d.whats_new[0].newer_than_installed == Some(true));
        assert!(!d.whats_new[1].unseen);
        let na = &d.watches[0];
        assert_eq!(na.releases.iter().sum::<u32>(), 2, "1.0.0 is outside 30 days");
        assert_eq!((na.failed_checks[28], na.checks[28]), (1, 2));
        assert_eq!(na.average_interval_ms, Some(99 * DAY_MS / 2));
        assert_eq!(na.since_last_ms, Some(DAY_MS - 995));
        assert_eq!(na.sparkline.iter().sum::<u32>(), 2);
        let nb = &d.watches[1];
        assert_eq!((nb.average_interval_ms, nb.since_last_ms), (None, None));
        assert_eq!(
            d.downloads[29],
            DownloadBucket {
                count: 1,
                failed: 1,
                bytes: 100
            }
        );
        assert_eq!(
            compute(&inputs, &j, Range::Year, now, 10).watches[0].releases.iter().sum::<u32>(),
            3
        );
    }
}
