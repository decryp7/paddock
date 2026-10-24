use crate::clock::Clock;
use crate::error::{BenchError, Result};
use crate::kind::Kind;
use crate::results::{Measurement, Results};
use crate::stats::Samples;

/// Collects an app's measurements into [`Results`]. Timed measurements run the work several times and keep the median,
/// so one noisy run does not move a budget.
#[derive(Debug)]
pub struct Recorder {
    results: Results,
}

impl Recorder {
    /// A recorder for `app` at `app_version`.
    pub fn new(app: &str, app_version: &str) -> Recorder {
        Recorder {
            results: Results::new(app, app_version),
        }
    }

    /// Notes the commit measured.
    pub fn commit(mut self, commit: &str) -> Recorder {
        self.results.commit = Some(commit.into());
        self
    }

    /// Records a number the app measured itself (a time in milliseconds, a rate, megabytes).
    pub fn record(
        &mut self,
        name: &str,
        kind: Kind,
        value: f64,
        runs: u32,
    ) -> Result<&mut Recorder> {
        self.results.measurements.push(Measurement {
            name: name.into(),
            kind,
            value,
            runs,
        });
        if let Err(e) = self.results.validate() {
            self.results.measurements.pop();
            return Err(e);
        }
        Ok(self)
    }

    /// Runs `work` `runs` times, timing each with `clock`, and records the median in milliseconds
    /// (use with [`Kind::Startup`], [`Kind::FirstEvent`] or [`Kind::QueryLatency`]).
    pub fn time_ms(
        &mut self,
        clock: &dyn Clock,
        name: &str,
        kind: Kind,
        runs: u32,
        mut work: impl FnMut(),
    ) -> Result<&mut Recorder> {
        let samples = Self::sample(clock, runs, &mut work)?;
        let median = samples.summary().map_or(0.0, |s| s.median) * 1000.0;
        self.record(name, kind, median, runs)
    }

    /// Runs `work` (which handles `items` items) `runs` times and records the median rate in items per second
    /// ([`Kind::Throughput`]). A run that took no measurable time is an error, not an infinite rate.
    pub fn throughput(
        &mut self,
        clock: &dyn Clock,
        name: &str,
        items: u64,
        runs: u32,
        mut work: impl FnMut(),
    ) -> Result<&mut Recorder> {
        let samples = Self::sample(clock, runs, &mut work)?;
        let median_secs = samples.summary().map_or(0.0, |s| s.median);
        if median_secs <= 0.0 {
            return Err(BenchError::Invalid(format!(
                "{name} took no measurable time; make the input larger."
            )));
        }
        self.record(name, Kind::Throughput, items as f64 / median_secs, runs)
    }

    /// Records a memory figure given in bytes as megabytes ([`Kind::Memory`]).
    pub fn memory_bytes(&mut self, name: &str, bytes: u64) -> Result<&mut Recorder> {
        self.record(name, Kind::Memory, bytes as f64 / (1024.0 * 1024.0), 1)
    }

    fn sample(clock: &dyn Clock, runs: u32, work: &mut dyn FnMut()) -> Result<Samples> {
        if runs == 0 {
            return Err(BenchError::Invalid(
                "a timed measurement needs at least one run.".into(),
            ));
        }
        let mut samples = Samples::new();
        for _ in 0..runs {
            let start = clock.now();
            work();
            samples.push(clock.now().saturating_sub(start).as_secs_f64());
        }
        Ok(samples)
    }

    /// The results so far.
    pub fn finish(self) -> Results {
        self.results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;
    use std::time::Duration;

    #[test]
    fn times_with_the_injected_clock_and_keeps_the_median() {
        let clock = ManualClock::new();
        let costs = [
            Duration::from_millis(100),
            Duration::from_millis(900),
            Duration::from_millis(120),
        ];
        let mut i = 0;
        let mut rec = Recorder::new("demo", "1.0.0").commit("abc");
        rec.time_ms(&clock, "query", Kind::QueryLatency, 3, || {
            clock.advance(costs[i]);
            i += 1;
        })
        .unwrap();
        let r = rec.finish();
        assert_eq!(r.commit.as_deref(), Some("abc"));
        let m = r.get("query").unwrap();
        assert!(
            (m.value - 120.0).abs() < 1e-9,
            "median ignores the noisy 900 ms run: {}",
            m.value
        );
        assert_eq!((m.kind, m.runs), (Kind::QueryLatency, 3));
    }

    #[test]
    fn throughput_is_items_over_median_seconds() {
        let clock = ManualClock::new();
        let mut rec = Recorder::new("demo", "1.0.0");
        rec.throughput(&clock, "import", 500_000, 3, || {
            clock.advance(Duration::from_millis(250))
        })
        .unwrap();
        let v = rec.finish().get("import").unwrap().value;
        assert!((v - 2_000_000.0).abs() < 1.0, "{v}");
    }

    #[test]
    fn instant_work_zero_runs_and_bad_numbers_are_errors() {
        let clock = ManualClock::new();
        let mut rec = Recorder::new("demo", "1.0.0");
        assert!(
            matches!(rec.throughput(&clock, "x", 10, 2, || {}), Err(BenchError::Invalid(m)) if m.contains("no measurable time"))
        );
        assert!(rec.time_ms(&clock, "x", Kind::Startup, 0, || {}).is_err());
        assert!(rec.record("neg", Kind::Memory, -1.0, 1).is_err());
        assert!(rec.record("ok", Kind::Memory, 1.0, 1).is_ok());
        assert!(
            rec.record("ok", Kind::Memory, 2.0, 1).is_err(),
            "duplicate names are refused and not kept"
        );
        assert_eq!(rec.finish().measurements.len(), 1);
    }

    #[test]
    fn memory_is_recorded_in_megabytes() {
        let mut rec = Recorder::new("demo", "1.0.0");
        rec.memory_bytes("rss", 300 * 1024 * 1024).unwrap();
        let m = rec.finish();
        assert_eq!(
            (m.get("rss").unwrap().value, m.get("rss").unwrap().kind),
            (300.0, Kind::Memory)
        );
    }
}
