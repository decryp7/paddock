//! Progress and time, as two small ports so the index stays free of any job framework (ER-18) and of the wall clock.

/// Live, phase-labelled progress from the first moment. Each phase has its own scale; a phase's count only grows; the next stage
/// is a new phase, never a restart. The engine adapts this to `paddock-jobs`.
pub trait Progress: Sync {
    /// Starts a phase: a stable id, the label shown to the user (sentence case), and the total when it is known.
    fn phase(&self, id: &'static str, label: &'static str, total: Option<u64>);

    /// `n` more items of the current phase are done.
    fn add(&self, n: u64);

    /// The total of the current phase became known (a discovery phase learns it at the end).
    fn set_total(&self, total: u64);
}

/// Reports nothing.
pub struct NoProgress;

impl Progress for NoProgress {
    fn phase(&self, _: &'static str, _: &'static str, _: Option<u64>) {}
    fn add(&self, _: u64) {}
    fn set_total(&self, _: u64) {}
}

/// Nanoseconds since the Unix epoch. Used only to stamp when a file was read (the shortcut's racy rule), never to judge
/// liveness (ADR 0031).
pub trait EpochClock: Sync {
    /// Now.
    fn now_ns(&self) -> i64;
}

/// The system clock.
pub struct SystemEpoch;

impl EpochClock for SystemEpoch {
    fn now_ns(&self) -> i64 {
        crate::fs::epoch_ns(std::time::SystemTime::now()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_report_nothing_and_the_system_clock_is_after_the_epoch() {
        NoProgress.phase("x", "X", None);
        NoProgress.add(1);
        NoProgress.set_total(3);
        assert!(SystemEpoch.now_ns() > 1_600_000_000_000_000_000);
    }
}
