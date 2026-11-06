//! How long a background loop (the catalogue, the full-text index) waits after a turn. A turn with nothing to do waits for
//! a wake-up, not for the clock: an idle store runs no statement and wakes no thread (budget: at most 1 wake-up a second;
//! research note `docs/research/2026-10-28-cpu-memory.md`, H6). Only a turn that was blocked (an import holds the write
//! lock) tries again soon.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
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

/// A safety net against a missed wake-up, not a poll: an idle loop wakes once a minute (60 s), without news it runs no statement.
pub(crate) const IDLE_BACKSTOP: Duration = Duration::from_secs(60);

/// Whether a loop has news since its last look. A loop without news runs no statement: the data of the store changes
/// only through an import, a removal or a refresh, and each of those bumps the store's generation (or wakes the loop
/// itself), so a turn that finds the generation as it was and no wake-up has nothing to learn from the database.
pub(crate) struct Gate {
    /// Set by a wake-up, and by a turn that has to try again (it did work, or was blocked, or failed).
    dirty: AtomicBool,
    /// The generation the last look saw.
    seen: AtomicU64,
    generation: Arc<AtomicU64>,
}

impl Gate {
    /// A gate that lets the first turn look.
    pub(crate) fn new(generation: Arc<AtomicU64>) -> Gate {
        Gate {
            dirty: AtomicBool::new(true),
            seen: AtomicU64::new(0),
            generation,
        }
    }

    /// There is news (or a turn has to be tried again): the next turn looks.
    pub(crate) fn news(&self) {
        self.dirty.store(true, Ordering::SeqCst);
    }

    /// Whether a turn has anything to look at. Taking it consumes the news: a change that follows is news again.
    pub(crate) fn take(&self) -> bool {
        let now = self.generation.load(Ordering::SeqCst);
        let before = self.seen.swap(now, Ordering::SeqCst);
        self.dirty.swap(false, Ordering::SeqCst) || now != before
    }

    /// Ends a turn: one that did work or could not finish tries again at once or soon, an idle one waits for news.
    pub(crate) fn after(&self, turn: Turn) {
        if turn != Turn::Idle {
            self.news();
        }
    }
}

/// How long to wait before the next turn; `None` means go on at once.
pub(crate) fn wait_after(turn: Turn) -> Option<Duration> {
    match turn {
        Turn::Worked => None,
        Turn::Blocked => Some(BLOCKED_RETRY),
        Turn::Idle => Some(IDLE_BACKSTOP),
    }
}

/// Counting the statements a store's connections run (DuckDB's query log), for the tests that say an idle loop runs none.
#[cfg(test)]
pub(crate) mod statement_log {
    use duckdb::Connection;

    /// Starts logging every statement of the database `conn` belongs to.
    pub(crate) fn start(conn: &Connection) {
        conn.execute_batch("CALL enable_logging('QueryLog')").unwrap();
    }

    /// The statements logged so far, not counting the reads of this log.
    pub(crate) fn count(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT message FROM duckdb_logs WHERE type = 'QueryLog' AND message NOT LIKE '%duckdb_logs%' ORDER BY timestamp")
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0)).unwrap().map(Result::unwrap).collect()
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
    fn a_gate_lets_a_turn_look_only_with_news() {
        let generation = Arc::new(AtomicU64::new(0));
        let gate = Gate::new(generation.clone());
        assert!(gate.take(), "the first turn looks");
        assert!(!gate.take(), "no news, no look");
        gate.news();
        assert!(gate.take(), "a wake-up is news");
        assert!(!gate.take());
        generation.fetch_add(1, Ordering::SeqCst);
        assert!(gate.take(), "a change of the data is news");
        assert!(!gate.take());
        gate.after(Turn::Idle);
        assert!(!gate.take(), "an idle turn waits for news");
        gate.after(Turn::Blocked);
        assert!(gate.take(), "a blocked turn is tried again");
        gate.after(Turn::Worked);
        assert!(gate.take(), "a turn that worked looks for more");
    }

    #[test]
    fn a_blocked_turn_retries_soon_and_work_goes_on() {
        assert_eq!(wait_after(Turn::Blocked), Some(BLOCKED_RETRY));
        assert_eq!(wait_after(Turn::Worked), None);
    }
}
