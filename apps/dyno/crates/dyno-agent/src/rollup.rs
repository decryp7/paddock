//! Writing history: raw points every few seconds, per-minute app rows, disk free space, and the minute, quarter-hour
//! and hourly rollups built from the tier below when a bucket closes (ADR 0008). Rollups read the lower tier from the
//! database, so they survive restarts and fill gaps (a bounded number of missed buckets are caught up).

use crate::store::{Result, Store};
use dyno_core::grouping::AppGroup;
use dyno_core::history::{core_bucket_from, downsample, downsample_cores, point_from, AppMinute, Tier, RAW_STEP_MS, TOP_APPS_PER_MINUTE};
use dyno_core::metrics::SystemSample;
use dyno_core::time::{Timestamp, HOUR, MINUTE};
use std::collections::HashMap;

/// (destination, source) of each rollup.
const LEVELS: [(Tier, Tier); 3] = [
    (Tier::Minute, Tier::Raw),
    (Tier::QuarterHour, Tier::Minute),
    (Tier::Hour, Tier::QuarterHour),
];
/// The most buckets caught up per level in one pass.
const MAX_CATCH_UP: i64 = 12;
/// How many ticks in a row may fail to store a record before the stuck minute's app sums are dropped (fourth review: a
/// writer that keeps failing must not stop the history; the newer ticks' sums are not given up for an old minute).
pub const MAX_RECORD_TRIES: u32 = 3;

fn meta_key(t: Tier) -> String {
    format!("rollup.{}", t.step_ms())
}

/// Per-app sums over the minute in progress.
#[derive(Default, Clone)]
struct AppAccum {
    minute: Timestamp,
    ticks: u32,
    apps: HashMap<String, (f64, u64)>,
}

/// Writes history as samples arrive. Cloned before a tick so a tick whose writes were not stored can be forgotten
/// ([`HistoryWriter::record`], the sampler's tick).
#[derive(Clone)]
pub struct HistoryWriter {
    last_raw: Option<Timestamp>,
    apps: AppAccum,
    last_prune: Timestamp,
    /// Ticks in a row whose record was not stored.
    failed: u32,
}

impl Default for HistoryWriter {
    fn default() -> Self {
        HistoryWriter {
            last_raw: None,
            apps: AppAccum::default(),
            last_prune: Timestamp(0),
            failed: 0,
        }
    }
}

impl HistoryWriter {
    /// Records one tick: the raw point (when a step has passed), app sums, disk space and any closed buckets.
    /// `groups` is empty on ticks that did not refresh the process table.
    /// One transaction for the whole tick (rule 85; ADR 0108 DY-7), with two writers in it (Dyno ADR 0008, "Store writers"):
    ///
    /// - the record (the raw and per-core points and a closed minute's apps), one savepoint: all or none for the record. When it
    ///   fails the writer is put back as it was, so the next tick writes the raw point and flushes the minute again (points
    ///   replace, a minute's apps are one whole: never twice). After [`MAX_RECORD_TRIES`] failures in a row the stuck minute's
    ///   app sums are dropped, with an error naming the minute and the cause, and the writer moves on.
    /// - the rollup, its own savepoint: it keeps nothing in memory (its progress marks are in the store), so a failed one is
    ///   rolled back alone and done again by the next tick; it never costs the raw points.
    ///
    /// Returns the record's error, if any. A failed commit (alone, outside the sampler's batch) puts the writer back too.
    pub fn record(&mut self, store: &Store, s: &SystemSample, groups: &[AppGroup]) -> Result<()> {
        let before = self.clone();
        let batch = store.batch();
        let written = store.atomic(|| self.write_record(store, s, groups));
        if let Err(e) = &written {
            let failed = before.failed + 1;
            *self = before.clone();
            self.failed = failed;
            if failed >= MAX_RECORD_TRIES {
                log::error!(
                    minute = self.apps.minute.millis(), tries = failed;
                    "dyno-agent: the history of a minute could not be stored ({e}); its app sums are dropped and the history goes on"
                );
                self.apps = AppAccum::default();
                self.failed = 0;
            }
        } else {
            self.failed = 0;
        }
        if let Err(e) = store.atomic(|| roll_up(store, s.at)) {
            log::error!("dyno-agent: the history rollup could not be stored ({e}); the next tick rolls up again");
        }
        let committed = batch.commit();
        if committed.is_err() {
            // Nothing of this tick was stored: as it was before it.
            *self = before;
        }
        written.and(committed)
    }

    /// The raw-point mark (the contract harness reads whether a refused tick moved it).
    #[cfg(test)]
    pub(crate) fn raw_mark(&self) -> Option<Timestamp> {
        self.last_raw
    }

    fn write_record(&mut self, store: &Store, s: &SystemSample, groups: &[AppGroup]) -> Result<()> {
        let at = s.at.floor_to(RAW_STEP_MS);
        if self.last_raw != Some(at) {
            let mut p = point_from(s);
            p.at = at;
            store.put_point(Tier::Raw, &p)?;
            let mut c = core_bucket_from(s);
            c.at = at;
            store.put_core_point(Tier::Raw, &c)?;
            self.last_raw = Some(at);
        }
        self.add_apps(store, s.at, groups)
    }

    fn add_apps(&mut self, store: &Store, now: Timestamp, groups: &[AppGroup]) -> Result<()> {
        let minute = now.floor_to(MINUTE);
        if self.apps.minute != minute {
            self.flush_apps(store)?;
            self.apps.minute = minute;
        }
        if groups.is_empty() {
            return Ok(());
        }
        self.apps.ticks += 1;
        for g in groups {
            let e = self.apps.apps.entry(g.name.clone()).or_default();
            e.0 += f64::from(g.cpu_read());
            e.1 = e.1.max(g.memory_read());
        }
        Ok(())
    }

    /// Writes the finished minute's top apps (average CPU over the process ticks of the minute, peak memory).
    fn flush_apps(&mut self, store: &Store) -> Result<()> {
        let acc = std::mem::take(&mut self.apps);
        if acc.ticks == 0 {
            return Ok(());
        }
        let n = f64::from(acc.ticks);
        let mut rows: Vec<AppMinute> = acc
            .apps
            .into_iter()
            .map(|(app, (cpu, memory))| AppMinute {
                at: acc.minute,
                app,
                cpu: (cpu / n) as f32,
                memory,
            })
            .collect();
        rows.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.memory.cmp(&a.memory)));
        rows.truncate(TOP_APPS_PER_MINUTE);
        store.put_app_minutes(&rows)?;
        Ok(())
    }

    /// Stores each disk's free space once per quarter hour (the trend needs days, not seconds), in one transaction.
    pub fn record_disks(&self, store: &Store, s: &SystemSample) -> Result<()> {
        let batch = store.batch();
        let at = s.at.floor_to(15 * MINUTE);
        store.atomic(|| {
            for d in &s.disks {
                store.put_disk_free(&d.id, at, d.total, d.free)?;
            }
            Ok(())
        })?;
        batch.commit()
    }

    /// Prunes at most once an hour.
    pub fn prune(&mut self, store: &Store, retention: dyno_core::history::Retention, now: Timestamp) -> Result<()> {
        if now.since(self.last_prune) >= HOUR {
            // Undone alone: a prune that was not stored is tried again at the next tick.
            store.prune(retention, now)?;
            self.last_prune = now;
        }
        Ok(())
    }
}

/// Builds the buckets that closed since the last pass, tier by tier.
pub fn roll_up(store: &Store, now: Timestamp) -> Result<()> {
    for (dest, src) in LEVELS {
        let width = dest.step_ms();
        let end = now.floor_to(width);
        let done = store.meta(&meta_key(dest))?.and_then(|v| v.parse::<i64>().ok());
        if done == Some(end.millis()) {
            continue;
        }
        let mut from = done.unwrap_or(end.millis() - width).max(end.millis() - MAX_CATCH_UP * width);
        while from < end.millis() {
            let pts = store.points(src, Timestamp(from), Timestamp(from + width))?;
            for p in downsample(&pts, width) {
                store.put_point(dest, &p)?;
            }
            let cores = store.core_points(src, Timestamp(from), Timestamp(from + width))?;
            for c in downsample_cores(&cores, width) {
                store.put_core_point(dest, &c)?;
            }
            from += width;
        }
        store.set_meta(&meta_key(dest), &end.millis().to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::history::HistoryPoint;
    use dyno_core::process::Measured;

    fn raw(store: &Store, at: i64, cpu: f32) {
        store
            .put_point(
                Tier::Raw,
                &HistoryPoint {
                    at: Timestamp(at),
                    cpu,
                    cpu_max: cpu,
                    cpu_min: cpu,
                    ..Default::default()
                },
            )
            .unwrap();
    }

    #[test]
    fn closed_buckets_roll_up_through_every_tier_with_min_avg_max() {
        let store = Store::memory().unwrap();
        // Two hours of raw points: cpu alternates 10/90 each 5 s.
        let start = 10 * HOUR;
        for i in 0..(2 * HOUR / RAW_STEP_MS) {
            raw(&store, start + i * RAW_STEP_MS, if i % 2 == 0 { 10.0 } else { 90.0 });
        }
        // First pass at the start of the third hour only knows "now"; seed progress so everything is built.
        store.set_meta(&meta_key(Tier::Minute), &(start).to_string()).unwrap();
        roll_up(&store, Timestamp(start + 2 * HOUR)).unwrap();
        let minutes = store.points(Tier::Minute, Timestamp(start), Timestamp(start + 2 * HOUR)).unwrap();
        assert_eq!(minutes.len(), 12, "catch-up is bounded to {MAX_CATCH_UP} buckets");
        assert!((minutes[0].cpu - 50.0).abs() < 1.0);
        assert_eq!((minutes[0].cpu_min, minutes[0].cpu_max), (10.0, 90.0));
        // The quarter-hour and hour tiers were built from the minute tier that exists.
        assert!(!store
            .points(Tier::QuarterHour, Timestamp(0), Timestamp(i64::MAX))
            .unwrap()
            .is_empty());
        // Nothing new to do at the same instant.
        roll_up(&store, Timestamp(start + 2 * HOUR)).unwrap();
    }

    #[test]
    fn the_first_pass_builds_only_the_bucket_just_closed() {
        let store = Store::memory().unwrap();
        raw(&store, 3 * MINUTE + 5_000, 40.0);
        raw(&store, 2 * MINUTE + 5_000, 20.0);
        roll_up(&store, Timestamp(4 * MINUTE + 1)).unwrap();
        let m = store.points(Tier::Minute, Timestamp(0), Timestamp(10 * MINUTE)).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].at, Timestamp(3 * MINUTE));
        assert_eq!(m[0].cpu, 40.0);
    }

    #[test]
    fn the_writer_stores_one_raw_point_per_step_and_the_minutes_top_apps() {
        let store = Store::memory().unwrap();
        let mut w = HistoryWriter::default();
        let mut s = dyno_platform::fake::quiet_sample(Timestamp(0));
        s.disks.push(dyno_core::metrics::DiskSample {
            id: "/".into(),
            name: "Disk".into(),
            mount: "/".into(),
            total: 100,
            free: 40,
            removable: false,
        });
        let group = |name: &str, cpu: f32| AppGroup {
            id: name.into(),
            name: name.into(),
            exe: None,
            pids: vec![1],
            cpu: Measured::Value(cpu),
            memory: Measured::Value(100),
            disk_bps: Measured::Value(0.0),
            partial: false,
        };
        for i in 0..30 {
            s.at = Timestamp(i * 1_000);
            let g = vec![group("busy", 80.0), group("idle", 1.0)];
            w.record(&store, &s, &g).unwrap();
        }
        assert_eq!(store.core_points(Tier::Raw, Timestamp(0), Timestamp(MINUTE)).unwrap().len(), 6);
        assert_eq!(
            store.core_points(Tier::Raw, Timestamp(0), Timestamp(MINUTE)).unwrap()[0]
                .cores
                .len(),
            4
        );
        // 30 s of ticks at 1 s: 6 raw points.
        assert_eq!(store.points(Tier::Raw, Timestamp(0), Timestamp(MINUTE)).unwrap().len(), 6);
        s.at = Timestamp(MINUTE + 1);
        w.record(&store, &s, &[]).unwrap();
        let rows = store.app_minutes(Timestamp(0), Timestamp(MINUTE)).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].app, "busy");
        assert!((rows[0].cpu - 80.0).abs() < 0.01);
        w.record_disks(&store, &s).unwrap();
        assert_eq!(store.disk_free(Timestamp(0), Timestamp(HOUR)).unwrap().len(), 1);
        w.prune(&store, Default::default(), Timestamp(MINUTE)).unwrap();
    }

    /// Review P2-1: a tick whose commit fails says so, and the writer forgets nothing it did not store: the next tick writes the
    /// raw point and flushes the minute's apps that the failed one lost.
    #[test]
    fn a_tick_whose_commit_fails_is_written_again_by_the_next() {
        let mut store = Store::memory().unwrap();
        let commits = store.count_commits();
        let mut w = HistoryWriter::default();
        let mut s = dyno_platform::fake::quiet_sample(Timestamp(0));
        let apps = [AppGroup {
            id: "busy".into(),
            name: "busy".into(),
            exe: None,
            pids: vec![1],
            cpu: Measured::Value(50.0),
            memory: Measured::Value(100),
            disk_bps: Measured::Value(0.0),
            partial: false,
        }];
        for i in 0..(MINUTE / RAW_STEP_MS) {
            s.at = Timestamp(i * RAW_STEP_MS);
            w.record(&store, &s, &apps).unwrap();
        }
        commits.fail(true);
        s.at = Timestamp(MINUTE + 1);
        assert!(w.record(&store, &s, &apps).is_err(), "the failed commit is reported");
        commits.fail(false);
        assert!(store
            .points(Tier::Raw, Timestamp(MINUTE), Timestamp(MINUTE + RAW_STEP_MS))
            .unwrap()
            .is_empty());
        assert!(store.app_minutes(Timestamp(0), Timestamp(MINUTE)).unwrap().is_empty());
        s.at = Timestamp(MINUTE + 2);
        w.record(&store, &s, &apps).unwrap();
        assert_eq!(
            store
                .points(Tier::Raw, Timestamp(MINUTE), Timestamp(MINUTE + RAW_STEP_MS))
                .unwrap()
                .len(),
            1,
            "the raw point the failed tick lost is written"
        );
        assert_eq!(
            store.app_minutes(Timestamp(0), Timestamp(MINUTE)).unwrap().len(),
            1,
            "the first minute's apps are flushed"
        );
    }

    /// Second review P1-A, as the fourth review set it: a record that fails inside the sampler's batch (the closed minute's
    /// apps refused) leaves none of its own rows, the batch commits, and the next tick's flush of that minute is the only one.
    #[test]
    fn a_tick_that_fails_inside_a_batch_leaves_none_of_its_rows() {
        let store = Store::memory().unwrap();
        let mut w = HistoryWriter::default();
        let mut s = dyno_platform::fake::quiet_sample(Timestamp(0));
        let apps = [AppGroup {
            id: "busy".into(),
            name: "busy".into(),
            exe: None,
            pids: vec![1],
            cpu: Measured::Value(50.0),
            memory: Measured::Value(100),
            disk_bps: Measured::Value(0.0),
            partial: false,
        }];
        for i in 0..(MINUTE / RAW_STEP_MS) {
            s.at = Timestamp(i * RAW_STEP_MS);
            w.record(&store, &s, &apps).unwrap();
        }
        store.exec_for_test(
            "CREATE TRIGGER no_busy BEFORE INSERT ON app_minutes WHEN NEW.app = 'busy' BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        );
        let batch = store.batch();
        s.at = Timestamp(MINUTE + 1);
        assert!(w.record(&store, &s, &apps).is_err());
        batch.commit().unwrap();
        assert!(
            store
                .points(Tier::Raw, Timestamp(MINUTE), Timestamp(MINUTE + RAW_STEP_MS))
                .unwrap()
                .is_empty(),
            "none of the failed record's rows"
        );
        store.exec_for_test("DROP TRIGGER no_busy;");
        s.at = Timestamp(MINUTE + 2);
        w.record(&store, &s, &apps).unwrap();
        assert_eq!(
            store.app_minutes(Timestamp(0), Timestamp(MINUTE)).unwrap().len(),
            1,
            "one row for the minute"
        );
    }

    /// Fourth review: the rollup keeps no state in memory (it reads its progress marks from the store), so a refused rollup is
    /// its own savepoint: the record's raw points are kept, and the next tick rolls up again.
    #[test]
    fn a_refused_rollup_does_not_cost_the_raw_points() {
        let store = Store::memory().unwrap();
        let mut w = HistoryWriter::default();
        let mut s = dyno_platform::fake::quiet_sample(Timestamp(0));
        store.exec_for_test(
            "CREATE TRIGGER no_mark BEFORE INSERT ON meta WHEN NEW.key LIKE 'rollup.%' BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        );
        for i in 0..(2 * MINUTE / RAW_STEP_MS) {
            s.at = Timestamp(i * RAW_STEP_MS);
            let _ = w.record(&store, &s, &[]);
        }
        assert_eq!(store.points(Tier::Raw, Timestamp(0), Timestamp(2 * MINUTE)).unwrap().len(), 24);
        store.exec_for_test("DROP TRIGGER no_mark;");
        s.at = Timestamp(2 * MINUTE + 1);
        w.record(&store, &s, &[]).unwrap();
        assert!(
            !store.points(Tier::Minute, Timestamp(0), Timestamp(2 * MINUTE)).unwrap().is_empty(),
            "rolled up again"
        );
    }

    /// Fourth review: a minute whose apps cannot be stored is tried three times and then dropped with an error, so the ticks
    /// after it store their own points and sums instead of failing with it for ever.
    #[test]
    fn a_minute_that_cannot_be_stored_is_dropped_after_three_tries() {
        let store = Store::memory().unwrap();
        let mut w = HistoryWriter::default();
        let mut s = dyno_platform::fake::quiet_sample(Timestamp(0));
        let apps = [AppGroup {
            id: "stuck".into(),
            name: "stuck".into(),
            exe: None,
            pids: vec![1],
            cpu: Measured::Value(50.0),
            memory: Measured::Value(100),
            disk_bps: Measured::Value(0.0),
            partial: false,
        }];
        for i in 0..(MINUTE / RAW_STEP_MS) {
            s.at = Timestamp(i * RAW_STEP_MS);
            w.record(&store, &s, &apps).unwrap();
        }
        store.exec_for_test(
            "CREATE TRIGGER no_stuck BEFORE INSERT ON app_minutes WHEN NEW.app = 'stuck' BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        );
        let mut results = Vec::new();
        for i in 0..6 {
            s.at = Timestamp(MINUTE + 1 + i * RAW_STEP_MS);
            results.push(w.record(&store, &s, &[]).is_ok());
        }
        assert_eq!(
            results,
            [false, false, false, true, true, true],
            "three tries, then the minute is dropped"
        );
        assert_eq!(
            store.points(Tier::Raw, Timestamp(MINUTE), Timestamp(2 * MINUTE)).unwrap().len(),
            3,
            "the ticks after it store their points"
        );
    }

    /// DY-7 (ADR 0108, guard D): a tick's rows (the raw and per-core points, the closed minute's apps, every rollup tier and
    /// its progress mark), a quarter hour's disks and an hourly prune are one commit each, however many rows they write.
    #[test]
    fn a_tick_its_disks_and_a_prune_are_one_commit_each() {
        let mut store = Store::memory().unwrap();
        let mut w = HistoryWriter::default();
        let mut s = dyno_platform::fake::quiet_sample(Timestamp(0));
        for i in 0..3 {
            s.disks.push(dyno_core::metrics::DiskSample {
                id: format!("disk{i}"),
                name: "Disk".into(),
                mount: format!("/d{i}"),
                total: 100,
                free: 40,
                removable: false,
            });
        }
        let apps = [AppGroup {
            id: "busy".into(),
            name: "busy".into(),
            exe: None,
            pids: vec![1],
            cpu: Measured::Value(50.0),
            memory: Measured::Value(100),
            disk_bps: Measured::Value(0.0),
            partial: false,
        }];
        for i in 0..(HOUR / RAW_STEP_MS) {
            s.at = Timestamp(i * RAW_STEP_MS);
            w.record(&store, &s, &apps).unwrap();
        }
        let commits = store.count_commits();
        // An hour boundary: a raw point, the last minute's apps, and the minute, quarter-hour and hour rollups with their marks.
        s.at = Timestamp(HOUR + 1);
        w.record(&store, &s, &apps).unwrap();
        assert!(!store.points(Tier::Hour, Timestamp(0), Timestamp(2 * HOUR)).unwrap().is_empty());
        assert_eq!(commits.take(), 1, "one tick, one transaction");
        w.record_disks(&store, &s).unwrap();
        assert_eq!(store.disk_free(Timestamp(0), Timestamp(2 * HOUR)).unwrap().len(), 3);
        assert_eq!(commits.take(), 1, "every disk of a quarter hour in one transaction");
        w.prune(&store, Default::default(), Timestamp(HOUR + 1)).unwrap();
        assert_eq!(commits.take(), 1, "the prune's deletes in one transaction");
    }
}
