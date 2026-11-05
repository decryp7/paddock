//! How long a background loop (the catalogue, the full-text index) waits after a turn. A turn with nothing to do waits for
//! a wake-up, not for the clock: an idle store runs no statement and wakes no thread (budget: at most 1 wake-up a second;
//! research note `docs/research/2026-10-28-cpu-memory.md`, H6). Only a turn that was blocked (an import holds the write
//! lock) tries again soon.

use std::time::Duration;

/// What one turn of a background loop found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Turn {
    /// It did work: take the next turn at once.
    Worked,
    /// There is work, but the write lock was busy or the work failed: try again soon.
    Blocked,
    /// Nothing to do: wait for a wake-up.
    Idle,
}

/// The retry delay of a blocked turn.
pub(crate) const BLOCKED_RETRY: Duration = Duration::from_millis(500);

/// A safety net against a missed wake-up, not a poll: an idle loop wakes this rarely (well under 1 a second).
pub(crate) const IDLE_BACKSTOP: Duration = Duration::from_secs(60);

/// How long to wait before the next turn; `None` means go on at once.
pub(crate) fn wait_after(turn: Turn) -> Option<Duration> {
    match turn {
        Turn::Worked => None,
        Turn::Blocked => Some(BLOCKED_RETRY),
        Turn::Idle => Some(IDLE_BACKSTOP),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_idle_turn_does_not_poll() {
        let idle = wait_after(Turn::Idle).unwrap();
        assert!(
            idle >= Duration::from_secs(10),
            "an idle loop must not wake more than once in 10 s, waits {idle:?}"
        );
    }

    #[test]
    fn a_blocked_turn_retries_soon_and_work_goes_on() {
        assert_eq!(wait_after(Turn::Blocked), Some(BLOCKED_RETRY));
        assert_eq!(wait_after(Turn::Worked), None);
    }
}
