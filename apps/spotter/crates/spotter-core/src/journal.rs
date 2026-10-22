//! The local history behind the dashboard: an event feed, daily check counts per watch (source reliability) and
//! downloads. Everything is bounded by count and by age ([`RETENTION_DAYS`]).

use crate::time::Timestamp;
use crate::watch::WatchId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

/// Events kept.
pub const MAX_EVENTS: usize = 1000;
/// Downloads kept.
pub const MAX_DOWNLOADS: usize = 500;
/// Days of history kept (a year and a bit, for the 1-year charts).
pub const RETENTION_DAYS: i64 = 400;
/// Milliseconds per day.
pub const DAY_MS: i64 = 86_400_000;

/// What happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum EventKind {
    /// A check found a new release.
    NewRelease {
        /// Its version.
        version: String,
        /// Its name.
        name: String,
    },
    /// A watch started failing (logged when the status changes, not at every check).
    CheckFailed {
        /// Why.
        message: String,
    },
    /// A failing watch works again.
    Recovered,
    /// A download finished (and was verified when a checksum was known).
    DownloadFinished {
        /// The release's version.
        version: String,
        /// Where the file is.
        path: String,
        /// Its size.
        size: u64,
        /// Whether a checksum was verified.
        verified: bool,
    },
    /// A download or another action failed.
    ActionFailed {
        /// Which action (`download`, `run-command`, `open-location`).
        action: String,
        /// Why.
        message: String,
    },
}

/// One event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// When.
    pub at: Timestamp,
    /// The watch it concerns.
    pub watch: Option<WatchId>,
    /// What.
    #[serde(flatten)]
    pub kind: EventKind,
}

/// Checks of one watch on one day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DayCounts {
    /// Checks that worked.
    pub ok: u32,
    /// Checks that failed.
    pub failed: u32,
}

/// One download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadRecord {
    /// When it finished.
    pub at: Timestamp,
    /// The watch.
    pub watch: WatchId,
    /// The release's version.
    pub version: String,
    /// Bytes written (0 when it failed early).
    pub size: u64,
    /// Whether it worked.
    pub ok: bool,
}

/// The dashboard's history.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Journal {
    /// Events, oldest first.
    pub events: VecDeque<Event>,
    /// Check counts per watch and day (days since the epoch, UTC).
    pub checks: BTreeMap<WatchId, BTreeMap<i64, DayCounts>>,
    /// Downloads, oldest first.
    pub downloads: VecDeque<DownloadRecord>,
}

impl Journal {
    /// Records an event.
    pub fn record(&mut self, at: Timestamp, watch: Option<WatchId>, kind: EventKind) {
        self.events.push_back(Event { at, watch, kind });
        while self.events.len() > MAX_EVENTS {
            self.events.pop_front();
        }
    }

    /// Counts one check.
    pub fn count_check(&mut self, watch: &WatchId, at: Timestamp, ok: bool) {
        let day = at.0.div_euclid(DAY_MS);
        let c = self.checks.entry(watch.clone()).or_default().entry(day).or_default();
        if ok {
            c.ok = c.ok.saturating_add(1);
        } else {
            c.failed = c.failed.saturating_add(1);
        }
    }

    /// Records a download.
    pub fn record_download(&mut self, record: DownloadRecord) {
        self.downloads.push_back(record);
        while self.downloads.len() > MAX_DOWNLOADS {
            self.downloads.pop_front();
        }
    }

    /// Drops what is older than [`RETENTION_DAYS`] and the history of watches that no longer exist.
    pub fn prune(&mut self, now: Timestamp, watches: &[WatchId]) {
        let cutoff = now.0 - RETENTION_DAYS * DAY_MS;
        self.events
            .retain(|e| e.at.0 >= cutoff && e.watch.as_ref().is_none_or(|w| watches.contains(w)));
        self.downloads.retain(|d| d.at.0 >= cutoff && watches.contains(&d.watch));
        self.checks.retain(|w, _| watches.contains(w));
        let first_day = cutoff.div_euclid(DAY_MS);
        for days in self.checks.values_mut() {
            days.retain(|d, _| *d >= first_day);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> WatchId {
        WatchId::parse(s).unwrap()
    }

    #[test]
    fn records_are_bounded_and_pruned() {
        let mut j = Journal::default();
        for i in 0..MAX_EVENTS + 5 {
            j.record(Timestamp(i as i64), Some(id("a")), EventKind::Recovered);
        }
        assert_eq!(j.events.len(), MAX_EVENTS);
        for i in 0..MAX_DOWNLOADS + 5 {
            j.record_download(DownloadRecord {
                at: Timestamp(i as i64),
                watch: id("a"),
                version: "1".into(),
                size: 1,
                ok: true,
            });
        }
        assert_eq!(j.downloads.len(), MAX_DOWNLOADS);
        j.count_check(&id("a"), Timestamp(DAY_MS + 5), true);
        j.count_check(&id("a"), Timestamp(DAY_MS + 6), false);
        j.count_check(&id("b"), Timestamp(-1), true);
        assert_eq!(j.checks[&id("a")][&1], DayCounts { ok: 1, failed: 1 });
        assert!(j.checks[&id("b")].contains_key(&-1));

        let now = Timestamp(RETENTION_DAYS * DAY_MS + 10 * DAY_MS);
        j.record(now, None, EventKind::Recovered);
        j.count_check(&id("a"), now, true);
        j.prune(now, &[id("a")]);
        assert_eq!(j.events.len(), 1, "old events go");
        assert!(j.downloads.is_empty());
        assert!(!j.checks.contains_key(&id("b")));
        assert_eq!(j.checks[&id("a")].len(), 1);
    }

    #[test]
    fn events_serialise_flat() {
        let e = Event {
            at: Timestamp(1),
            watch: None,
            kind: EventKind::CheckFailed { message: "x".into() },
        };
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(json, r#"{"at":1,"watch":null,"kind":"check-failed","message":"x"}"#);
        assert_eq!(serde_json::from_str::<Event>(&json).unwrap(), e);
    }
}
