//! ADR 0031's stall judgement for a child process, as a pure state machine. No clock: the unit is the **tick** (one wake-up of
//! the host's monitor, however late it comes), and what is observed per tick is the child's run state, whether its CPU time
//! grew, and whether the call made progress.

/// What a child is doing, from the operating system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    /// Runnable (or running).
    Runnable,
    /// Sleeping on something: a lock, a wait with no I/O pending.
    Blocked,
    /// Waiting for I/O (a busy disk is load, not a stall).
    IoWait,
    /// Stopped by the operating system or another app (paused).
    Stopped,
}

/// One observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    /// The child's run state.
    pub state: RunState,
    /// CPU time the child used since the previous tick, in microseconds.
    pub cpu_grew_micros: u64,
    /// The call made progress since the previous tick (an item streamed, a callback answered).
    pub progress: bool,
    /// The host owes the call something (a callback in its pump, a file read it must serve): the child is waiting on the host.
    pub owes_host: bool,
}

/// The limits, in ticks and CPU time. Both are Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StallLimits {
    /// Ticks a call may stay blocked without progress before it is stuck (deadlocked).
    pub blocked_ticks: u32,
    /// CPU time a call may use without progress before it is stuck (spinning), in microseconds; load does not inflate it.
    pub spin_cpu_micros: u64,
}

impl Default for StallLimits {
    fn default() -> Self {
        StallLimits {
            blocked_ticks: 120,
            spin_cpu_micros: 60_000_000,
        }
    }
}

/// Why a child is stuck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stuck {
    /// Blocked for this many ticks with nothing owed to it.
    Blocked(u32),
    /// Used this much CPU (microseconds) without progress.
    Spinning(u64),
}

/// The verdict of one tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Fine: working, starved or waiting on I/O. Nothing counts.
    Fine,
    /// Paused by the operating system or another app: wait, and say so ("Paused by macOS or another app").
    Paused,
    /// Stuck: end it.
    Stuck(Stuck),
}

/// Judges one call of one child. Counters are per call and reset by progress.
#[derive(Debug, Clone)]
pub struct StallJudge {
    limits: StallLimits,
    blocked: u32,
    spin_micros: u64,
}

impl StallJudge {
    /// A judge for a new call.
    pub fn new(limits: StallLimits) -> StallJudge {
        StallJudge {
            limits,
            blocked: 0,
            spin_micros: 0,
        }
    }

    /// Takes one tick.
    pub fn observe(&mut self, tick: Tick) -> Verdict {
        if tick.progress {
            self.blocked = 0;
            self.spin_micros = 0;
            return Verdict::Fine;
        }
        if tick.state == RunState::Stopped {
            return Verdict::Paused;
        }
        if tick.cpu_grew_micros > 0 {
            // it ran and still made no progress
            self.spin_micros = self.spin_micros.saturating_add(tick.cpu_grew_micros);
            if self.spin_micros >= self.limits.spin_cpu_micros {
                return Verdict::Stuck(Stuck::Spinning(self.spin_micros));
            }
            return Verdict::Fine;
        }
        if tick.state == RunState::Blocked && !tick.owes_host {
            self.blocked += 1;
            if self.blocked >= self.limits.blocked_ticks {
                return Verdict::Stuck(Stuck::Blocked(self.blocked));
            }
        }
        // runnable with no CPU (starved) and waiting for I/O count nothing
        Verdict::Fine
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tick(state: RunState, cpu: u64, progress: bool, owes: bool) -> Tick {
        Tick {
            state,
            cpu_grew_micros: cpu,
            progress,
            owes_host: owes,
        }
    }

    fn judge(blocked: u32, spin_secs: u64) -> StallJudge {
        StallJudge::new(StallLimits {
            blocked_ticks: blocked,
            spin_cpu_micros: spin_secs * 1_000_000,
        })
    }

    #[test]
    fn a_blocked_child_with_nothing_owed_is_stuck_after_the_blocked_limit_of_ticks() {
        let mut j = judge(3, 60);
        let t = tick(RunState::Blocked, 0, false, false);
        assert_eq!((j.observe(t), j.observe(t)), (Verdict::Fine, Verdict::Fine));
        assert_eq!(j.observe(t), Verdict::Stuck(Stuck::Blocked(3)));
    }

    #[test]
    fn a_blocked_child_that_waits_on_the_host_is_never_stuck_by_that() {
        let mut j = judge(2, 60);
        for _ in 0..50 {
            assert_eq!(j.observe(tick(RunState::Blocked, 0, false, true)), Verdict::Fine);
        }
    }

    #[test]
    fn a_spinning_child_is_stuck_by_cpu_time_not_by_ticks() {
        let mut j = judge(3, 2);
        // many ticks of a little CPU each: the limit is CPU-seconds, and load does not inflate CPU time
        for _ in 0..19 {
            assert_eq!(j.observe(tick(RunState::Runnable, 100_000, false, false)), Verdict::Fine);
        }
        assert_eq!(
            j.observe(tick(RunState::Runnable, 100_000, false, false)),
            Verdict::Stuck(Stuck::Spinning(2_000_000))
        );
    }

    #[test]
    fn a_starved_child_and_one_waiting_for_io_are_fine_however_long() {
        let mut j = judge(2, 1);
        for _ in 0..1000 {
            assert_eq!(
                j.observe(tick(RunState::Runnable, 0, false, false)),
                Verdict::Fine,
                "starved: runnable, no CPU"
            );
            assert_eq!(
                j.observe(tick(RunState::IoWait, 0, false, false)),
                Verdict::Fine,
                "waiting for the disk"
            );
        }
    }

    #[test]
    fn a_stopped_child_is_paused_and_counts_nothing_even_after_an_hour_of_ticks() {
        let mut j = judge(2, 1);
        for _ in 0..1000 {
            assert_eq!(j.observe(tick(RunState::Stopped, 0, false, false)), Verdict::Paused);
        }
        // and when it resumes, the limits are where they were
        assert_eq!(j.observe(tick(RunState::Blocked, 0, false, false)), Verdict::Fine);
        assert_eq!(
            j.observe(tick(RunState::Blocked, 0, false, false)),
            Verdict::Stuck(Stuck::Blocked(2))
        );
    }

    #[test]
    fn progress_resets_both_counters() {
        let mut j = judge(3, 2);
        let blocked = tick(RunState::Blocked, 0, false, false);
        j.observe(blocked);
        j.observe(blocked);
        assert_eq!(j.observe(tick(RunState::Blocked, 0, true, false)), Verdict::Fine);
        j.observe(blocked);
        assert_eq!(j.observe(blocked), Verdict::Fine, "the count started again after the progress");
        let mut s = judge(3, 2);
        s.observe(tick(RunState::Runnable, 1_900_000, false, false));
        s.observe(tick(RunState::Runnable, 1, true, false));
        assert_eq!(s.observe(tick(RunState::Runnable, 1_900_000, false, false)), Verdict::Fine);
    }

    #[test]
    fn the_defaults_are_the_documented_ones() {
        assert_eq!(
            StallLimits::default(),
            StallLimits {
                blocked_ticks: 120,
                spin_cpu_micros: 60_000_000
            }
        );
    }
}
