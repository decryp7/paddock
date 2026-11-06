//! Counts of what the store's background loops did, for the worker's `store.activity` line (ER-27): a loop that wakes every
//! half second and finds nothing to do shows as many idle turns and no work, which is what "Debrief uses CPU while idle"
//! looks like in a log. Process-wide counters, read and reset by the worker every ten minutes.

use std::sync::atomic::{AtomicU64, Ordering};

/// A background loop (or step source) of the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loop {
    /// The full-text index writer's loop.
    Fts,
    /// The name catalogue's loop.
    Catalogue,
    /// The live tail's refreshes.
    Tail,
    /// Time-order position rebuilds and extensions.
    Positions,
}

impl Loop {
    /// Every loop, in the order of the log line.
    pub const ALL: [Loop; 4] = [Loop::Fts, Loop::Catalogue, Loop::Tail, Loop::Positions];

    /// The word in the log line.
    pub fn name(self) -> &'static str {
        match self {
            Loop::Fts => "fts",
            Loop::Catalogue => "catalogue",
            Loop::Tail => "tail",
            Loop::Positions => "positions",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// What one turn of a loop came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// It did work (or tried to and failed).
    Worked,
    /// It looked and found nothing to do.
    Idle,
    /// There was work but the write lock was busy.
    Blocked,
    /// The turn failed (the loop waits before it tries again).
    Failed,
}

/// The turns of one loop since the last [`take`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub worked: u64,
    pub idle: u64,
    pub blocked: u64,
    pub failed: u64,
}

/// The counters of every loop: [`record`] and [`take`] work on the process's one set; tests make their own.
pub struct Counters([[AtomicU64; 4]; 4]);

impl Counters {
    /// All zero.
    pub const fn new() -> Counters {
        Counters([const { [const { AtomicU64::new(0) }; 4] }; 4])
    }

    /// Counts one turn of `l`.
    pub fn record(&self, l: Loop, turn: Turn) {
        self.0[l.index()][turn as usize].fetch_add(1, Ordering::Relaxed);
    }

    /// The counts of every loop since the last call, reset to zero.
    pub fn take(&self) -> [(Loop, Counts); 4] {
        Loop::ALL.map(|l| {
            let c = &self.0[l.index()];
            (
                l,
                Counts {
                    worked: c[Turn::Worked as usize].swap(0, Ordering::Relaxed),
                    idle: c[Turn::Idle as usize].swap(0, Ordering::Relaxed),
                    blocked: c[Turn::Blocked as usize].swap(0, Ordering::Relaxed),
                    failed: c[Turn::Failed as usize].swap(0, Ordering::Relaxed),
                },
            )
        })
    }
}

impl Default for Counters {
    fn default() -> Self {
        Counters::new()
    }
}

static COUNTERS: Counters = Counters::new();

/// Counts one turn of `l` in the process's counters.
pub fn record(l: Loop, turn: Turn) {
    COUNTERS.record(l, turn);
}

/// The process's counts of every loop since the last call, reset to zero.
pub fn take() -> [(Loop, Counts); 4] {
    COUNTERS.take()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_are_counted_per_loop_and_reset_by_take() {
        // Its own counters: the process's are fed by the other tests' loops at the same time.
        let c = Counters::new();
        c.record(Loop::Fts, Turn::Idle);
        c.record(Loop::Fts, Turn::Idle);
        c.record(Loop::Fts, Turn::Worked);
        c.record(Loop::Catalogue, Turn::Blocked);
        c.record(Loop::Catalogue, Turn::Failed);
        let got = c.take();
        assert_eq!(
            got[0],
            (
                Loop::Fts,
                Counts {
                    worked: 1,
                    idle: 2,
                    blocked: 0,
                    failed: 0
                }
            )
        );
        assert_eq!(
            got[1],
            (
                Loop::Catalogue,
                Counts {
                    worked: 0,
                    idle: 0,
                    blocked: 1,
                    failed: 1
                }
            )
        );
        assert_eq!(got[2], (Loop::Tail, Counts::default()));
        assert_eq!(got[3], (Loop::Positions, Counts::default()));
        assert_eq!(c.take()[0].1, Counts::default(), "taking resets");
        assert_eq!(Loop::ALL.map(Loop::name), ["fts", "catalogue", "tail", "positions"]);
        assert_eq!(Loop::ALL.map(Loop::index), [0, 1, 2, 3]);
    }
}
