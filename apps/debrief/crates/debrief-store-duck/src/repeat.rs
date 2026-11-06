//! "Log on change" for a loop that retries a failing step every few hundred milliseconds: the first failure of a cause is
//! logged with its chain, the repeats of the same cause are only counted, a different cause is logged as a new first failure,
//! and the recovery is one line with the count and the time it lasted. ADR 0055 §6's general limiter, when it lands, does not
//! say "recovered"; this does.

use std::time::{Duration, Instant};

/// The event ids and words of one loop's failure lines.
pub(crate) struct Names {
    pub failed: &'static str,
    pub cause_changed: &'static str,
    pub recovered: &'static str,
    /// "full-text indexing failed": what the warning says (never the error).
    pub what: &'static str,
}

/// The full-text indexing loop's lines.
pub(crate) const FTS: Names = Names {
    failed: "store.fts.failed",
    cause_changed: "store.fts.cause_changed",
    recovered: "store.fts.recovered",
    what: "full-text indexing failed",
};

/// The name catalogue loop's lines.
pub(crate) const CATALOGUE: Names = Names {
    failed: "store.catalogue.failed",
    cause_changed: "store.catalogue.cause_changed",
    recovered: "store.catalogue.recovered",
    what: "cataloguing failed",
};

/// What makes two failures the same cause: the outermost context (it names the segment or source) and the class of the root
/// cause, never the whole text (it can carry values that change from turn to turn, and a counter in it would defeat the guard).
pub(crate) fn cause_key(e: &anyhow::Error) -> String {
    let class = debrief_core::logsafe::class_of(&e.root_cause().to_string());
    if e.chain().count() > 1 {
        format!("{e}|{class}")
    } else {
        class
    }
}

/// What a failure means for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The first failure, or one with a different cause than the run before: log it. `ended` is the run of the previous
    /// cause that this one ended, when there was one.
    First { ended: Option<Run> },
    /// The same cause again: counted, not logged.
    Repeat,
}

/// A run of failures with one cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Run {
    pub failures: u64,
    pub lasted: Duration,
}

/// The state of one loop's failures.
#[derive(Debug, Default)]
pub(crate) struct RepeatGuard {
    cause: Option<String>,
    failures: u64,
    since: Option<Instant>,
}

impl RepeatGuard {
    /// A failure with `cause` (its message) at `now`.
    pub fn failed(&mut self, cause: &str, now: Instant) -> Failure {
        if self.cause.as_deref() == Some(cause) {
            self.failures += 1;
            return Failure::Repeat;
        }
        let ended = self.take(now);
        self.cause = Some(cause.to_string());
        self.failures = 1;
        self.since = Some(now);
        Failure::First { ended }
    }

    /// The loop worked again at `now`: the run that just ended, when there was one.
    pub fn recovered(&mut self, now: Instant) -> Option<Run> {
        self.take(now)
    }

    /// A failed turn: the first failure of a cause is logged (the class at warn, the text at debug), repeats are only
    /// counted, and a new cause says how long the old one lasted.
    pub fn log_failure(&mut self, names: &Names, e: &anyhow::Error, now: Instant) {
        if let Failure::First { ended } = self.failed(&cause_key(e), now) {
            if let Some(r) = ended {
                log::info!(EventId = names.cause_changed, Failures = r.failures, Ms = crate::ms(r.lasted); "The earlier failure happened {} times; a different one follows", r.failures);
            }
            debrief_core::log_failure!(warn, names.what, e; EventId = names.failed);
        }
    }

    /// A turn that did its job or found nothing to do (not one that was blocked): one line when the loop had been failing.
    pub fn log_recovery(&mut self, names: &Names, now: Instant) {
        if let Some(r) = self.recovered(now) {
            log::info!(EventId = names.recovered, Failures = r.failures, Ms = crate::ms(r.lasted); "Recovered after {} failures", r.failures);
        }
    }

    fn take(&mut self, now: Instant) -> Option<Run> {
        let since = self.since.take()?;
        self.cause = None;
        Some(Run {
            failures: std::mem::take(&mut self.failures),
            lasted: now.saturating_duration_since(since),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hundred_identical_failures_log_once_and_the_recovery_counts_them() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut g = RepeatGuard::default();
        assert_eq!(g.recovered(at(0)), None, "nothing failed: nothing to report");
        let logged = (0..100u64)
            .filter(|i| matches!(g.failed("disk full", at(i * 500)), Failure::First { .. }))
            .count();
        assert_eq!(logged, 1, "one warning for a hundred repeats");
        assert_eq!(
            g.recovered(at(49_500 + 500)),
            Some(Run {
                failures: 100,
                lasted: Duration::from_millis(50_000)
            })
        );
        assert_eq!(g.recovered(at(60_000)), None, "reported once");
    }

    #[test]
    fn a_loop_logs_a_hundred_identical_failures_once_and_its_recovery_once() {
        crate::testlog::install();
        const TEST: Names = Names {
            failed: "test.repeat.failed",
            cause_changed: "test.repeat.cause_changed",
            recovered: "test.repeat.recovered",
            what: "the test loop failed",
        };
        let t0 = Instant::now();
        let mut g = RepeatGuard::default();
        // The same cause with a different value each time (what a DuckDB message does): still one cause.
        for i in 0..100u64 {
            let e = anyhow::anyhow!("IO Error: could not read value {i}").context("indexing seg-0001");
            g.log_failure(&TEST, &e, t0 + Duration::from_millis(i * 500));
        }
        assert_eq!(crate::testlog::records("test.repeat.failed").len(), 1);
        let first = &crate::testlog::records("test.repeat.failed")[0];
        assert_eq!((first.level, first.props["Why"].as_str()), (log::Level::Warn, Some("IO Error")));
        assert!(!format!("{first:?}").contains("value 0"), "the text is for debug: {first:?}");
        // Another cause: logged, and the old run is closed with its count.
        g.log_failure(
            &TEST,
            &anyhow::anyhow!("Binder Error: x").context("indexing seg-0002"),
            t0 + Duration::from_secs(60),
        );
        assert_eq!(crate::testlog::records("test.repeat.failed").len(), 2);
        let changed = &crate::testlog::records("test.repeat.cause_changed")[0];
        assert_eq!(changed.props["Failures"], 100);
        g.log_recovery(&TEST, t0 + Duration::from_secs(70));
        g.log_recovery(&TEST, t0 + Duration::from_secs(71));
        let recovered = crate::testlog::records("test.repeat.recovered");
        assert_eq!(recovered.len(), 1, "once");
        assert_eq!(recovered[0].props["Failures"], 1);
    }

    #[test]
    fn a_new_cause_is_logged_and_ends_the_run_of_the_old_one() {
        let t0 = Instant::now();
        let mut g = RepeatGuard::default();
        assert_eq!(g.failed("a", t0), Failure::First { ended: None });
        assert_eq!(g.failed("a", t0 + Duration::from_secs(1)), Failure::Repeat);
        assert_eq!(
            g.failed("b", t0 + Duration::from_secs(2)),
            Failure::First {
                ended: Some(Run {
                    failures: 2,
                    lasted: Duration::from_secs(2)
                })
            }
        );
        assert_eq!(
            g.failed("a", t0 + Duration::from_secs(3)),
            Failure::First {
                ended: Some(Run {
                    failures: 1,
                    lasted: Duration::from_secs(1)
                })
            }
        );
    }
}
