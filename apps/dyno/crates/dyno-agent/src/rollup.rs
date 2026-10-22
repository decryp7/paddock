//! Writing history: raw points every few seconds, per-minute app rows, disk free space, and the minute, quarter-hour
//! and hourly rollups built from the tier below when a bucket closes (ADR 0008). Rollups read the lower tier from the
//! database, so they survive restarts and fill gaps (a bounded number of missed buckets are caught up).

use crate::store::{Result, Store};
use dyno_core::grouping::AppGroup;
use dyno_core::history::{downsample, point_from, AppMinute, Tier, RAW_STEP_MS, TOP_APPS_PER_MINUTE};
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

fn meta_key(t: Tier) -> String {
    format!("rollup.{}", t.step_ms())
}

/// Per-app sums over the minute in progress.
#[derive(Default)]
struct AppAccum {
    minute: Timestamp,
    ticks: u32,
    apps: HashMap<String, (f64, u64)>,
}

/// Writes history as samples arrive.
pub struct HistoryWriter {
    last_raw: Option<Timestamp>,
    apps: AppAccum,
    last_prune: Timestamp,
}

impl Default for HistoryWriter {
    fn default() -> Self {
        HistoryWriter {
            last_raw: None,
            apps: AppAccum::default(),
            last_prune: Timestamp(0),
        }
    }
}

impl HistoryWriter {
    /// Records one tick: the raw point (when a step has passed), app sums, disk space and any closed buckets.
    /// `groups` is empty on ticks that did not refresh the process table.
    pub fn record(&mut self, store: &mut Store, s: &SystemSample, groups: &[AppGroup]) -> Result<()> {
        let at = s.at.floor_to(RAW_STEP_MS);
        if self.last_raw != Some(at) {
            let mut p = point_from(s);
            p.at = at;
            store.put_point(Tier::Raw, &p)?;
            self.last_raw = Some(at);
        }
        self.add_apps(store, s.at, groups)?;
        roll_up(store, s.at)
    }

    fn add_apps(&mut self, store: &mut Store, now: Timestamp, groups: &[AppGroup]) -> Result<()> {
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
            e.0 += f64::from(g.cpu);
            e.1 = e.1.max(g.memory);
        }
        Ok(())
    }

    /// Writes the finished minute's top apps (average CPU over the process ticks of the minute, peak memory).
    fn flush_apps(&mut self, store: &mut Store) -> Result<()> {
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

    /// Stores each disk's free space once per quarter hour (the trend needs days, not seconds).
    pub fn record_disks(&self, store: &Store, s: &SystemSample) -> Result<()> {
        let at = s.at.floor_to(15 * MINUTE);
        for d in &s.disks {
            store.put_disk_free(&d.id, at, d.total, d.free)?;
        }
        Ok(())
    }

    /// Prunes at most once an hour.
    pub fn prune(&mut self, store: &Store, retention: dyno_core::history::Retention, now: Timestamp) -> Result<()> {
        if now.since(self.last_prune) >= HOUR {
            self.last_prune = now;
            store.prune(retention, now)?;
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
        let mut store = Store::memory().unwrap();
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
            cpu,
            memory: 100,
            disk_bps: 0.0,
        };
        for i in 0..30 {
            s.at = Timestamp(i * 1_000);
            let g = vec![group("busy", 80.0), group("idle", 1.0)];
            w.record(&mut store, &s, &g).unwrap();
        }
        // 30 s of ticks at 1 s: 6 raw points.
        assert_eq!(store.points(Tier::Raw, Timestamp(0), Timestamp(MINUTE)).unwrap().len(), 6);
        s.at = Timestamp(MINUTE + 1);
        w.record(&mut store, &s, &[]).unwrap();
        let rows = store.app_minutes(Timestamp(0), Timestamp(MINUTE)).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].app, "busy");
        assert!((rows[0].cpu - 80.0).abs() < 0.01);
        w.record_disks(&store, &s).unwrap();
        assert_eq!(store.disk_free(Timestamp(0), Timestamp(HOUR)).unwrap().len(), 1);
        w.prune(&store, Default::default(), Timestamp(MINUTE)).unwrap();
    }
}
