//! Watching a capped child's memory (ADR 0031 §5 of Debrief): the pure [`MemoryWatch`] turns samples into verdicts (the
//! peak, a warning at [`WARN_PERCENT`] of the cap with hysteresis, the stop over the cap) and the [`MemoryListener`]
//! hears the events. The sampling thread is in `Supervised`.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use std::sync::Arc;

/// The share of the cap at which the host is warned.
pub const WARN_PERCENT: u64 = 80;
/// The share of the cap below which a warning is armed again (a child hovering at 80% warns once).
pub const REARM_PERCENT: u64 = 70;

/// A child's memory at one moment: what it uses, its peak so far and its cap (bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryUse {
    /// Bytes in use now.
    pub used: u64,
    /// The most seen so far.
    pub peak: u64,
    /// The cap.
    pub limit: u64,
}

/// What the watcher reports about one child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryEvent {
    /// It crossed [`WARN_PERCENT`] of its cap (again after falling below [`REARM_PERCENT`]).
    Warning(MemoryUse),
    /// It used more than its cap and its tree was ended.
    Stopped(MemoryUse),
    /// Watching ended (the child ended or was ended for another reason); `peak` is the most it used.
    Ended(MemoryUse),
}

impl MemoryEvent {
    /// The figures of the event.
    pub fn usage(&self) -> MemoryUse {
        match self {
            MemoryEvent::Warning(u) | MemoryEvent::Stopped(u) | MemoryEvent::Ended(u) => *u,
        }
    }
}

type ListenFn = dyn Fn(&str, MemoryEvent) + Send + Sync;

/// Hears the events of one child: called on the watcher's thread with the child's label and the event. Must return
/// quickly; a panic is caught and ignored.
#[derive(Clone)]
pub struct MemoryListener(Arc<ListenFn>);

impl MemoryListener {
    /// A listener from a closure.
    pub fn new(f: impl Fn(&str, MemoryEvent) + Send + Sync + 'static) -> MemoryListener {
        MemoryListener(Arc::new(f))
    }

    /// Tells the listener; a panicking listener never takes the watcher down.
    pub fn tell(&self, label: &str, event: MemoryEvent) {
        let f = self.0.clone();
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(label, event))).is_err() {
            log::error!("a memory listener panicked while told about `{label}`; the watcher goes on");
        }
    }
}

impl std::fmt::Debug for MemoryListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MemoryListener")
    }
}

/// What one sample means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing to report.
    Fine,
    /// Report a warning; the child keeps running.
    Warn(MemoryUse),
    /// Over the cap: end the tree and report it.
    Stop(MemoryUse),
}

/// The state of watching one child against its cap. Pure: [`MemoryWatch::observe`] takes a sample and answers what to
/// do, so every threshold is tested without processes or clocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryWatch {
    limit: u64,
    peak: u64,
    armed: bool,
}

impl MemoryWatch {
    /// Watches against `limit` bytes.
    pub fn new(limit: u64) -> MemoryWatch {
        MemoryWatch {
            limit,
            peak: 0,
            armed: true,
        }
    }

    /// The cap.
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// The most seen so far.
    pub fn peak(&self) -> u64 {
        self.peak
    }

    /// The level of the warning, in bytes.
    pub fn warn_at(&self) -> u64 {
        share(self.limit, WARN_PERCENT)
    }

    fn usage(&self, used: u64) -> MemoryUse {
        MemoryUse {
            used,
            peak: self.peak,
            limit: self.limit,
        }
    }

    /// Takes a sample of the memory the child uses now.
    pub fn observe(&mut self, used: u64) -> Verdict {
        self.peak = self.peak.max(used);
        if used > self.limit {
            return Verdict::Stop(self.usage(used));
        }
        if used >= self.warn_at() {
            if self.armed {
                self.armed = false;
                return Verdict::Warn(self.usage(used));
            }
        } else if used < share(self.limit, REARM_PERCENT) {
            self.armed = true;
        }
        Verdict::Fine
    }

    /// The report when watching ends without a stop over the cap.
    pub fn ended(&self, used: u64) -> MemoryUse {
        self.usage(used)
    }
}

fn share(limit: u64, percent: u64) -> u64 {
    // In u128: a cap near u64::MAX must not overflow.
    (u128::from(limit) * u128::from(percent) / 100) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn a_sample_over_the_cap_stops_and_the_peak_is_kept() {
        let mut w = MemoryWatch::new(1000);
        assert_eq!(w.observe(100), Verdict::Fine);
        match w.observe(1001) {
            Verdict::Stop(u) => assert_eq!((u.used, u.peak, u.limit), (1001, 1001, 1000)),
            v => panic!("{v:?}"),
        }
        assert_eq!(w.peak(), 1001);
        assert_eq!(w.limit(), 1000);
        assert_eq!(
            w.observe(1000),
            Verdict::Warn(MemoryUse {
                used: 1000,
                peak: 1001,
                limit: 1000
            }),
            "exactly the cap is allowed"
        );
    }

    #[test]
    fn the_warning_comes_once_and_rearms_below_the_lower_level() {
        let mut w = MemoryWatch::new(1000);
        assert_eq!(w.warn_at(), 800);
        assert!(matches!(w.observe(800), Verdict::Warn(_)));
        assert_eq!(w.observe(900), Verdict::Fine);
        assert_eq!(
            w.observe(750),
            Verdict::Fine,
            "between the levels it stays quiet and unarmed"
        );
        assert_eq!(w.observe(850), Verdict::Fine);
        assert_eq!(w.observe(699), Verdict::Fine);
        assert!(
            matches!(w.observe(800), Verdict::Warn(_)),
            "armed again below 70%"
        );
    }

    #[test]
    fn a_cap_near_the_maximum_does_not_overflow() {
        let w = MemoryWatch::new(u64::MAX);
        assert_eq!(w.warn_at(), (u128::from(u64::MAX) * 80 / 100) as u64);
        assert_eq!(
            w.ended(5),
            MemoryUse {
                used: 5,
                peak: 0,
                limit: u64::MAX
            }
        );
    }

    #[test]
    fn a_listener_hears_events_and_a_panicking_one_is_contained() {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let h = heard.clone();
        let l = MemoryListener::new(move |label, e| {
            h.lock().unwrap().push((label.to_string(), e.usage().used))
        });
        let u = MemoryUse {
            used: 7,
            peak: 7,
            limit: 10,
        };
        l.tell("the engine", MemoryEvent::Warning(u));
        assert_eq!(*heard.lock().unwrap(), vec![("the engine".to_string(), 7)]);
        assert_eq!(format!("{l:?}"), "MemoryListener");
        MemoryListener::new(|_, _| panic!("listener bug")).tell("x", MemoryEvent::Ended(u));
        assert_eq!(MemoryEvent::Stopped(u).usage(), u);
    }
}
