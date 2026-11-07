//! The bounded queue between the callers of a connection and its writer thread (IP1 item 2).
//!
//! Frames wait here until the writer has put them on the stream. The queue holds at most `max_frames` frames and
//! `max_bytes` bytes (one frame larger than the byte bound is accepted when the queue is empty, so it can never be
//! stuck). A caller that finds it full **blocks** until the writer frees room, and **fails** with [`Push::Stalled`] once
//! `stall` has passed on the peer's clock without room: the other side has stopped reading, so memory does not grow and
//! the caller learns of the hang. The wait is event-driven (the writer cancels the waiters' tokens), so tests drive it
//! with a fake clock and no sleeps.

use paddock_jobs::CancelToken;
use paddock_jobs::time::{Clock, Deadline};
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

/// What the writer thread takes from the queue.
pub(crate) enum Out {
    /// One encoded frame.
    Frame(Vec<u8>),
    /// Stop after everything queued before this.
    Shutdown,
}

/// How a [`WriteQueue::push`] ended.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Push {
    /// Queued.
    Queued,
    /// No room for `stall`: the other side is not reading.
    Stalled,
    /// No room for the caller's own limit (shorter than the stall): this call gives up, the queue is as it was.
    Expired,
    /// [`WriteQueue::try_push`] found no room.
    Full,
    /// The queue was shut down.
    Closed,
}

#[derive(Default)]
struct State {
    items: VecDeque<Out>,
    frames: usize,
    bytes: usize,
    /// Callers waiting for room, by number.
    waiters: Vec<(u64, CancelToken)>,
    next_waiter: u64,
    closed: bool,
}

/// See the module.
pub(crate) struct WriteQueue {
    state: Mutex<State>,
    ready: Condvar,
    max_frames: usize,
    max_bytes: usize,
    /// Frames beyond `max_frames` that [`WriteQueue::try_push`] may use (control frames of the reader thread).
    reserve: usize,
    stall: Duration,
    clock: Arc<dyn Clock>,
}

/// The bytes one reserve frame may add beyond `max_bytes`: control frames (a refusal, a cancel) are small.
const RESERVE_FRAME_BYTES: usize = 4096;

fn lock(m: &Mutex<State>) -> MutexGuard<'_, State> {
    // The state stays consistent if a holder panicked (every update is a few field writes).
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl WriteQueue {
    pub(crate) fn new(
        max_frames: usize,
        max_bytes: usize,
        reserve: usize,
        stall: Duration,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            state: Mutex::new(State::default()),
            ready: Condvar::new(),
            max_frames: max_frames.max(1),
            max_bytes,
            reserve,
            stall,
            clock,
        }
    }

    /// Whether a frame of `len` bytes fits now (an empty queue takes any frame, so one over the byte bound cannot get stuck).
    fn fits(&self, s: &State, len: usize) -> bool {
        s.frames < self.max_frames
            && (s.frames == 0 || s.bytes.saturating_add(len) <= self.max_bytes)
    }

    /// Whether a control frame of `len` bytes fits in the reserve.
    fn fits_reserve(&self, s: &State, len: usize) -> bool {
        s.frames < self.max_frames.saturating_add(self.reserve)
            && s.bytes.saturating_add(len)
                <= self
                    .max_bytes
                    .saturating_add(self.reserve.saturating_mul(RESERVE_FRAME_BYTES))
    }

    fn queue(&self, mut s: MutexGuard<'_, State>, bytes: Vec<u8>) -> Push {
        s.frames += 1;
        s.bytes += bytes.len();
        s.items.push_back(Out::Frame(bytes));
        drop(s);
        self.ready.notify_one();
        Push::Queued
    }

    /// Queues a frame, blocking while the queue is full (see the module). The wait ends at the stall, or earlier at the
    /// caller's own `limit` (a call's timeout): [`Push::Stalled`] means the other side stopped reading, [`Push::Expired`]
    /// only that this caller ran out of time.
    pub(crate) fn push(&self, bytes: Vec<u8>, limit: Option<Duration>) -> Push {
        let stall = Deadline::after(self.clock.clone(), self.stall);
        let own = limit.map(|l| Deadline::after(self.clock.clone(), l));
        loop {
            let (id, token) = {
                let mut s = lock(&self.state);
                if s.closed {
                    return Push::Closed;
                }
                if self.fits(&s, bytes.len()) {
                    return self.queue(s, bytes);
                }
                // No room: say which deadline is over, the stall first (it closes the connection).
                if stall.passed() {
                    return Push::Stalled;
                }
                if own.as_ref().is_some_and(Deadline::passed) {
                    return Push::Expired;
                }
                let token = CancelToken::new();
                s.next_waiter += 1;
                let id = s.next_waiter;
                s.waiters.push((id, token.clone()));
                (id, token)
            };
            // Ends when the writer frees room (the token is cancelled) or a deadline passes; either way look again.
            let left = [
                stall.remaining(),
                own.as_ref().and_then(Deadline::remaining),
            ]
            .into_iter()
            .flatten()
            .min();
            let _ = self.clock.wait(left.unwrap_or(Duration::MAX), &token);
            // A waiter that timed out is still listed (the writer only drops the ones it woke): take it out.
            lock(&self.state).waiters.retain(|(w, _)| *w != id);
        }
    }

    /// Queues a frame if there is room now, in the bounds or in the reserve beyond them; [`Push::Full`] otherwise. For the
    /// thread that reads the connection, which must never wait for the writer. A queue that is merely full is
    /// back-pressure and no sign of silence, so its small control frames (refusals, cancels) get `reserve` frames of
    /// their own; only when even those are used up has the other side sent more than it reads.
    pub(crate) fn try_push(&self, bytes: Vec<u8>) -> Push {
        let s = lock(&self.state);
        if s.closed {
            Push::Closed
        } else if self.fits(&s, bytes.len()) || self.fits_reserve(&s, bytes.len()) {
            self.queue(s, bytes)
        } else {
            Push::Full
        }
    }

    /// Ends the queue: callers waiting for room return [`Push::Closed`], and the writer stops after the frames queued
    /// so far. Always succeeds, whatever the bounds.
    pub(crate) fn shutdown(&self) {
        let waiters = {
            let mut s = lock(&self.state);
            if !s.closed {
                s.closed = true;
                s.items.push_back(Out::Shutdown);
            }
            std::mem::take(&mut s.waiters)
        };
        for (_, w) in waiters {
            w.cancel();
        }
        self.ready.notify_all();
    }

    /// The next item for the writer, waiting for one.
    pub(crate) fn pop(&self) -> Out {
        let mut s = lock(&self.state);
        loop {
            if let Some(out) = s.items.pop_front() {
                if let Out::Frame(bytes) = &out {
                    s.frames -= 1;
                    s.bytes -= bytes.len();
                }
                let waiters = std::mem::take(&mut s.waiters);
                drop(s);
                for (_, w) in waiters {
                    w.cancel();
                }
                return out;
            }
            s = self.ready.wait(s).unwrap_or_else(|p| p.into_inner());
        }
    }

    /// Whether the queue was shut.
    #[cfg(test)]
    pub(crate) fn is_shut(&self) -> bool {
        lock(&self.state).closed
    }

    /// Frames and bytes waiting now.
    pub(crate) fn queued(&self) -> (usize, usize) {
        let s = lock(&self.state);
        (s.frames, s.bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_jobs::time::FakeClock;
    use std::sync::mpsc;
    use std::time::Duration;

    const GUARD: Duration = Duration::from_secs(10);

    fn queue(frames: usize, bytes: usize, clock: &Arc<FakeClock>) -> Arc<WriteQueue> {
        Arc::new(WriteQueue::new(
            frames,
            bytes,
            2,
            Duration::from_secs(30),
            clock.clone(),
        ))
    }

    /// A push on its own thread, whose outcome arrives on a channel: a push that never returns fails the test after
    /// `GUARD` instead of hanging it.
    fn pushing(
        q: &Arc<WriteQueue>,
        bytes: Vec<u8>,
        limit: Option<Duration>,
    ) -> mpsc::Receiver<Push> {
        let (tx, rx) = mpsc::channel();
        let q = q.clone();
        std::thread::spawn(move || {
            let _ = tx.send(q.push(bytes, limit));
        });
        rx
    }

    fn outcome(rx: &mpsc::Receiver<Push>) -> Push {
        rx.recv_timeout(GUARD).expect("the push never returned")
    }

    #[test]
    fn frames_come_out_in_order_and_the_counts_follow() {
        let clock = Arc::new(FakeClock::new());
        let q = queue(4, 100, &clock);
        assert_eq!(q.push(vec![1; 10], None), Push::Queued);
        assert_eq!(q.push(vec![2; 20], None), Push::Queued);
        assert_eq!(q.queued(), (2, 30));
        assert!(matches!(q.pop(), Out::Frame(b) if b[0] == 1));
        assert_eq!(q.queued(), (1, 20));
        assert!(matches!(q.pop(), Out::Frame(b) if b[0] == 2));
        assert_eq!(q.queued(), (0, 0));
    }

    #[test]
    fn a_frame_over_the_byte_bound_goes_in_alone() {
        let clock = Arc::new(FakeClock::new());
        let q = queue(4, 10, &clock);
        assert_eq!(q.push(vec![0; 50], None), Push::Queued);
        // Nothing else fits beside it: the next push stalls once the clock passes the stall.
        let rx = pushing(&q, vec![0; 1], None);
        assert!(clock.wait_for_waiters(1, GUARD));
        clock.advance(Duration::from_secs(30));
        assert_eq!(outcome(&rx), Push::Stalled);
        assert_eq!(q.queued(), (1, 50));
    }

    #[test]
    fn a_full_queue_blocks_then_fails_at_the_stall_and_never_grows() {
        let clock = Arc::new(FakeClock::new());
        let q = queue(2, 1 << 20, &clock);
        assert_eq!(q.push(vec![0; 1], None), Push::Queued);
        assert_eq!(q.push(vec![0; 1], None), Push::Queued);
        let rx = pushing(&q, vec![0; 1], None);
        assert!(clock.wait_for_waiters(1, GUARD));
        // Not before the stall.
        clock.advance(Duration::from_secs(29));
        assert!(rx.try_recv().is_err());
        clock.advance(Duration::from_secs(1));
        assert_eq!(outcome(&rx), Push::Stalled);
        assert_eq!(q.queued(), (2, 2));
        assert!(
            lock(&q.state).waiters.is_empty(),
            "a waiter that timed out is taken out of the list"
        );
    }

    #[test]
    fn a_callers_own_limit_ends_its_wait_before_the_stall_and_leaves_the_queue_as_it_was() {
        let clock = Arc::new(FakeClock::new());
        let q = queue(1, 1 << 20, &clock);
        assert_eq!(q.push(vec![0; 1], None), Push::Queued);
        let rx = pushing(&q, vec![0; 1], Some(Duration::from_secs(2)));
        assert!(clock.wait_for_waiters(1, GUARD));
        clock.advance(Duration::from_secs(1));
        assert!(rx.try_recv().is_err(), "not before its own limit");
        clock.advance(Duration::from_secs(1));
        assert_eq!(outcome(&rx), Push::Expired);
        assert_eq!(q.queued(), (1, 1));
        assert!(lock(&q.state).waiters.is_empty());
        // A limit longer than the stall is the stall.
        let rx = pushing(&q, vec![0; 1], Some(Duration::from_secs(600)));
        assert!(clock.wait_for_waiters(1, GUARD));
        clock.advance(Duration::from_secs(30));
        assert_eq!(outcome(&rx), Push::Stalled);
    }

    #[test]
    fn room_freed_by_the_writer_wakes_the_blocked_caller() {
        let clock = Arc::new(FakeClock::new());
        let q = queue(1, 1 << 20, &clock);
        assert_eq!(q.push(vec![7; 1], None), Push::Queued);
        let rx = pushing(&q, vec![8; 1], None);
        assert!(clock.wait_for_waiters(1, GUARD));
        assert!(matches!(q.pop(), Out::Frame(b) if b[0] == 7));
        assert_eq!(outcome(&rx), Push::Queued);
        assert!(matches!(q.pop(), Out::Frame(b) if b[0] == 8));
    }

    #[test]
    fn shutdown_releases_waiters_and_stops_the_writer_after_what_is_queued() {
        let clock = Arc::new(FakeClock::new());
        let q = queue(1, 1 << 20, &clock);
        assert_eq!(q.push(vec![1; 1], None), Push::Queued);
        let rx = pushing(&q, vec![2; 1], None);
        assert!(clock.wait_for_waiters(1, GUARD));
        q.shutdown();
        assert_eq!(outcome(&rx), Push::Closed);
        assert_eq!(q.push(vec![3; 1], None), Push::Closed);
        assert_eq!(q.try_push(vec![3; 1]), Push::Closed);
        assert!(matches!(q.pop(), Out::Frame(_)));
        assert!(matches!(q.pop(), Out::Shutdown));
        q.shutdown();
    }

    #[test]
    fn try_push_never_waits() {
        let clock = Arc::new(FakeClock::new());
        let q = queue(1, 1 << 20, &clock);
        assert_eq!(q.try_push(vec![1; 1]), Push::Queued);
        assert_eq!(q.try_push(vec![2; 1]), Push::Queued);
        assert_eq!(q.try_push(vec![3; 1]), Push::Queued);
        assert_eq!(q.try_push(vec![4; 1]), Push::Full);
        assert_eq!(q.queued(), (3, 3));
        assert_eq!(clock.waiters(), 0);
    }

    #[test]
    fn try_push_uses_the_reserve_beyond_the_bounds_and_no_more() {
        let clock = Arc::new(FakeClock::new());
        let q = queue(1, 100, &clock);
        assert_eq!(q.try_push(vec![1; 1]), Push::Queued);
        // The reserve is 2 frames: two more fit though the queue is full, the third does not.
        assert_eq!(q.try_push(vec![2; 1]), Push::Queued);
        assert_eq!(q.try_push(vec![3; 1]), Push::Queued);
        assert_eq!(q.try_push(vec![4; 1]), Push::Full);
        // The reserve is for small control frames: a big one does not use it.
        assert!(matches!(q.pop(), Out::Frame(_)));
        assert_eq!(q.try_push(vec![5; 100_000]), Push::Full);
        // A caller that blocks (push) never gets the reserve.
        assert_eq!(q.push(vec![6; 1], Some(Duration::ZERO)), Push::Expired);
    }

    #[test]
    fn the_writer_waits_for_a_frame() {
        let clock = Arc::new(FakeClock::new());
        let q = queue(2, 100, &clock);
        let q2 = q.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(matches!(q2.pop(), Out::Frame(b) if b == vec![5; 3]));
        });
        // Queue after the writer is waiting or before: either way it gets the frame.
        assert_eq!(q.push(vec![5; 3], None), Push::Queued);
        assert!(
            rx.recv_timeout(GUARD)
                .expect("the writer never got the frame")
        );
    }
}
