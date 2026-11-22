//! The pure part of the stall guard: the limits, the breaker and the per-connection watch. Every time is passed in, so each rule is
//! tested without a clock or a socket.

use std::time::{Duration, Instant};

/// The first bytes of a connection that stop the guard.
pub const STOP_MAGIC: &[u8] = b"PADDOCK-STALL-GUARD-STOP\n";

/// The most connections the guard serves at once; one more is closed at once.
pub const MAX_CONNECTIONS: usize = 256;

/// The guard's limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// No byte in either direction for this long cuts a connection.
    pub stall: Duration,
    /// After a cut with an answer outstanding, or a failed connect, every new connection is refused for this long.
    pub cooldown: Duration,
    /// The longest a connect to the store may take.
    pub connect: Duration,
    /// The guard ends after this long without a connection: the net for a guard no job end stopped, never a limit a running job can
    /// reach (see [`Limits::default`]).
    pub idle_exit: Duration,
}

impl Default for Limits {
    /// The limits CI's S3 cache uses: `stall` is 100 times the largest measured average request (0.02 to 0.07 s read, 0.05 s write,
    /// docs/benchmarks.md, "CI's sccache on RustFS"); the slowest single request was not measured, and a request cut that the
    /// store would have answered is a cache miss, never a failed build.
    fn default() -> Limits {
        Limits {
            stall: Duration::from_secs(10),
            cooldown: Duration::from_secs(60),
            connect: Duration::from_secs(3),
            // Twice the longest job (3 h). Was 15 minutes: too short, measured in ci.yml run 8363 (docs/benchmarks.md): one job's guard
            // ended between two compiles and the rest of that job lost its S3 hits. The guard is stopped at the end of its job
            // (`ci sccache-stats`, `ci isolation release` as the net); this only ends one that no job end ever reached.
            idle_exit: Duration::from_secs(6 * 3600),
        }
    }
}

/// Whether new connections are refused: opened by a trip, closed again once the cooldown has passed.
#[derive(Debug, Default)]
pub struct Breaker {
    open_until: Option<Instant>,
}

impl Breaker {
    /// Whether a connection at `now` is refused.
    pub fn is_open(&self, now: Instant) -> bool {
        self.open_until.is_some_and(|until| now < until)
    }

    /// Opens the breaker for `cooldown` from `now` (a later trip never shortens an earlier one).
    pub fn trip(&mut self, now: Instant, cooldown: Duration) {
        let until = now + cooldown;
        if self.open_until.is_none_or(|old| until > old) {
            self.open_until = Some(until);
        }
    }
}

/// What the guard does with a connection at one moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Keep forwarding.
    Continue,
    /// No byte for a stall and nothing outstanding: an idle keep-alive connection. Close it; the store is not at fault.
    CutQuiet,
    /// No byte for a stall while the client's request is unanswered: the store hangs. Close it and open the breaker.
    CutUnanswered,
}

/// One connection's record: when a byte last passed, and whether the client's last bytes are still unanswered.
#[derive(Debug, Clone, Copy)]
pub struct Watch {
    last: Instant,
    awaiting: bool,
}

impl Watch {
    /// A connection that has just opened.
    pub fn new(now: Instant) -> Watch {
        Watch {
            last: now,
            awaiting: false,
        }
    }

    /// The client sent bytes: an answer is now outstanding.
    pub fn client_bytes(&mut self, now: Instant) {
        self.last = now;
        self.awaiting = true;
    }

    /// The store sent bytes: the answer has started.
    pub fn upstream_bytes(&mut self, now: Instant) {
        self.last = now;
        self.awaiting = false;
    }

    /// What to do at `now` with a stall of `stall`.
    pub fn verdict(&self, now: Instant, stall: Duration) -> Verdict {
        if now.saturating_duration_since(self.last) <= stall {
            Verdict::Continue
        } else if self.awaiting {
            Verdict::CutUnanswered
        } else {
            Verdict::CutQuiet
        }
    }
}

/// Whether `head` (the first bytes a connection sent) is the stop request.
pub fn is_stop(head: &[u8]) -> bool {
    head.len() >= STOP_MAGIC.len() && head[..STOP_MAGIC.len()] == *STOP_MAGIC
}

#[cfg(test)]
mod tests {
    use super::*;

    const STALL: Duration = Duration::from_secs(10);

    #[test]
    fn a_connection_is_cut_only_after_a_full_stall() {
        let t0 = Instant::now();
        let mut w = Watch::new(t0);
        w.client_bytes(t0);
        assert_eq!(
            w.verdict(t0 + STALL, STALL),
            Verdict::Continue,
            "exactly one stall is not past it"
        );
        assert_eq!(
            w.verdict(t0 + STALL + Duration::from_millis(1), STALL),
            Verdict::CutUnanswered
        );
    }

    #[test]
    fn an_answer_in_progress_is_never_unanswered_and_an_idle_connection_is_quiet() {
        let t0 = Instant::now();
        let mut w = Watch::new(t0);
        w.client_bytes(t0);
        w.upstream_bytes(t0 + Duration::from_secs(1));
        let late = t0 + Duration::from_secs(30);
        assert_eq!(
            w.verdict(late, STALL),
            Verdict::CutQuiet,
            "the store answered: the silence after it is keep-alive"
        );
        // Bytes keep resetting the clock: a long transfer is never cut.
        let mut w = Watch::new(t0);
        for s in 1..=100 {
            w.upstream_bytes(t0 + Duration::from_secs(s * 5));
            assert_eq!(
                w.verdict(t0 + Duration::from_secs(s * 5 + 4), STALL),
                Verdict::Continue
            );
        }
    }

    #[test]
    fn a_new_request_on_a_kept_alive_connection_is_outstanding_again() {
        let t0 = Instant::now();
        let mut w = Watch::new(t0);
        w.client_bytes(t0);
        w.upstream_bytes(t0);
        w.client_bytes(t0 + Duration::from_secs(20));
        assert_eq!(
            w.verdict(t0 + Duration::from_secs(31), STALL),
            Verdict::CutUnanswered
        );
    }

    #[test]
    fn the_breaker_is_open_for_the_cooldown_and_a_later_trip_never_shortens_it() {
        let t0 = Instant::now();
        let mut b = Breaker::default();
        assert!(!b.is_open(t0));
        b.trip(t0, Duration::from_secs(60));
        assert!(b.is_open(t0 + Duration::from_secs(59)));
        assert!(!b.is_open(t0 + Duration::from_secs(60)));
        b.trip(t0 + Duration::from_secs(10), Duration::from_secs(5));
        assert!(
            b.is_open(t0 + Duration::from_secs(59)),
            "the shorter later trip did not shorten the open time"
        );
        b.trip(t0 + Duration::from_secs(50), Duration::from_secs(60));
        assert!(b.is_open(t0 + Duration::from_secs(109)));
    }

    #[test]
    fn only_the_whole_stop_request_stops_the_guard() {
        assert!(is_stop(STOP_MAGIC));
        assert!(is_stop(&[STOP_MAGIC, b"more"].concat()));
        assert!(!is_stop(&STOP_MAGIC[..STOP_MAGIC.len() - 1]));
        assert!(!is_stop(b"GET / HTTP/1.1\r\n\r\n"));
        assert!(!is_stop(b""));
    }

    #[test]
    fn the_default_limits_are_the_documented_ones_and_the_guard_outlives_a_job() {
        let l = Limits::default();
        assert_eq!(
            (l.stall.as_secs(), l.cooldown.as_secs(), l.connect.as_secs()),
            (10, 60, 3)
        );
        // The guard lives as long as its job: it is stopped at the job's end, and this limit is only the net for a job that never got
        // there. 15 minutes was measured too short (ci.yml run 8363: one job's guard ended between two compiles and the rest of the
        // job lost its S3 hits); it must be far above the longest job (3 h), so a job's own compile gaps never reach it.
        assert!(
            l.idle_exit >= Duration::from_secs(2 * 3 * 3600),
            "{:?}",
            l.idle_exit
        );
    }
}
