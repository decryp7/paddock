//! The hang detector of `heavy run --stall S` (a pure core: time and readings are passed in). A command is hung when it has written
//! nothing and its process tree has used no CPU for `S` seconds. It is a hang detector, never a speed limit (ER-12): a slow command
//! that computes or prints is never ended, however long it takes.
//!
//! - Output is watched all the time (a byte count the output pumps keep); CPU is read only after the output has been quiet for a
//!   sampling interval, so a chatty build costs no process-table reads.
//! - Any change of the tree's CPU time counts as activity, a fall too (a child that exits takes its time out of the sum, and a
//!   child exiting is progress).
//! - A CPU time that cannot be read is "not measured", never "idle" (ER-33): the clock starts again and the caller says so once.

use std::time::Duration;

/// The longest gap between two CPU readings while the output is quiet.
pub const MAX_SAMPLE: Duration = Duration::from_secs(1);
/// The shortest gap between two CPU readings.
pub const MIN_SAMPLE: Duration = Duration::from_millis(50);

/// What one observation found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Output or CPU moved, or it is too early to tell.
    Alive,
    /// The CPU time could not be read this time: the quiet clock started again (unknown is never idle).
    NotMeasured,
    /// No output and no CPU for this long (at least the limit).
    Hung(Duration),
}

/// The state of one watched command.
#[derive(Debug, Clone)]
pub struct HangWatch {
    limit_us: u64,
    sample_us: u64,
    last_activity_us: u64,
    next_sample_us: u64,
    bytes: u64,
    cpu_us: Option<u64>,
}

fn micros(d: Duration) -> u64 {
    u64::try_from(d.as_micros()).unwrap_or(u64::MAX)
}

impl HangWatch {
    /// A watch that calls a command hung after `limit` without output or CPU, starting at `now_us`.
    pub fn new(limit: Duration, now_us: u64) -> HangWatch {
        let sample = (limit / 4).clamp(MIN_SAMPLE, MAX_SAMPLE);
        HangWatch {
            limit_us: micros(limit),
            sample_us: micros(sample),
            last_activity_us: now_us,
            next_sample_us: now_us.saturating_add(micros(sample)),
            bytes: 0,
            cpu_us: None,
        }
    }

    /// The gap between two CPU readings while the output is quiet: a quarter of the limit, between [`MIN_SAMPLE`] and [`MAX_SAMPLE`].
    pub fn sample_interval(&self) -> Duration {
        Duration::from_micros(self.sample_us)
    }

    /// Something else showed the command is not hung (it is paused by the OS, not stuck): the quiet clock starts again.
    pub fn touch(&mut self, now_us: u64) {
        self.last_activity_us = now_us;
    }

    /// One look: `bytes` is the output written so far; `cpu` reads the tree's CPU time in microseconds (`None`: not measured) and is
    /// called only when the output was quiet and a sampling interval has passed.
    pub fn observe(&mut self, now_us: u64, bytes: u64, cpu: impl FnOnce() -> Option<u64>) -> Verdict {
        if bytes != self.bytes {
            self.bytes = bytes;
            self.last_activity_us = now_us;
            return Verdict::Alive;
        }
        if now_us < self.next_sample_us {
            return Verdict::Alive;
        }
        self.next_sample_us = now_us.saturating_add(self.sample_us);
        let Some(c) = cpu() else {
            self.last_activity_us = now_us;
            return Verdict::NotMeasured;
        };
        if self.cpu_us != Some(c) {
            self.cpu_us = Some(c);
            self.last_activity_us = now_us;
            return Verdict::Alive;
        }
        let quiet = now_us.saturating_sub(self.last_activity_us);
        if quiet >= self.limit_us {
            Verdict::Hung(Duration::from_micros(quiet))
        } else {
            Verdict::Alive
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    /// Drives a watch every 100 ms from 0 to `until_s` with `out(t)` bytes and `cpu(t)` CPU; returns the first hung time.
    fn drive(limit_s: u64, until_s: u64, out: impl Fn(u64) -> u64, cpu: impl Fn(u64) -> Option<u64>) -> Option<(u64, Duration)> {
        let mut w = HangWatch::new(Duration::from_secs(limit_s), 0);
        let mut t = 0;
        while t <= until_s * S {
            if let Verdict::Hung(q) = w.observe(t, out(t), || cpu(t)) {
                return Some((t, q));
            }
            t += S / 10;
        }
        None
    }

    #[test]
    fn silent_and_idle_is_hung_after_the_limit_and_not_before() {
        let (at, quiet) = drive(10, 60, |_| 0, |_| Some(5)).expect("hung");
        assert!(quiet >= Duration::from_secs(10), "{quiet:?}");
        // Called no later than the limit plus two sampling intervals (the first reading sets the baseline).
        assert!((10 * S..=12 * S).contains(&at), "{at}");
    }

    #[test]
    fn silent_but_busy_is_never_hung() {
        assert_eq!(drive(5, 120, |_| 0, |t| Some(t / 1000)), None);
    }

    #[test]
    fn idle_but_printing_is_never_hung() {
        assert_eq!(drive(5, 120, |t| t / (3 * S), |_| Some(7)), None);
    }

    #[test]
    fn a_falling_cpu_sum_counts_as_activity() {
        // A child exits every 3 s and takes its time out of the sum.
        assert_eq!(drive(5, 60, |_| 0, |t| Some(100 - (t / (3 * S)).min(99))), None);
    }

    #[test]
    fn cpu_that_cannot_be_read_is_never_called_idle() {
        let mut w = HangWatch::new(Duration::from_secs(2), 0);
        let mut seen_not_measured = false;
        for i in 0..1000 {
            match w.observe(i * S / 10, 0, || None) {
                Verdict::Hung(_) => panic!("unknown CPU read as idle"),
                Verdict::NotMeasured => seen_not_measured = true,
                Verdict::Alive => {}
            }
        }
        assert!(seen_not_measured);
    }

    #[test]
    fn cpu_is_read_only_after_a_quiet_sampling_interval() {
        let mut w = HangWatch::new(Duration::from_secs(4), 0);
        assert_eq!(w.sample_interval(), Duration::from_secs(1));
        let mut reads = 0;
        // Output every 100 ms for 10 s: never a CPU read.
        for i in 1..=100 {
            let _ = w.observe(i * S / 10, i, || {
                reads += 1;
                Some(0)
            });
        }
        assert_eq!(reads, 0);
        assert_eq!(HangWatch::new(Duration::from_millis(40), 0).sample_interval(), MIN_SAMPLE);
    }

    #[test]
    fn touch_restarts_the_quiet_clock() {
        let mut w = HangWatch::new(Duration::from_secs(2), 0);
        let _ = w.observe(S, 0, || Some(1));
        w.touch(2 * S);
        assert_eq!(w.observe(3 * S, 0, || Some(1)), Verdict::Alive);
        assert!(matches!(w.observe(4 * S, 0, || Some(1)), Verdict::Hung(q) if q == Duration::from_secs(2)));
    }
}
