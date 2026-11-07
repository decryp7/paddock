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
    /// The queue was shut down.
    Closed,
}

#[derive(Default)]
struct State {
    items: VecDeque<Out>,
    frames: usize,
    bytes: usize,
    /// Callers waiting for room.
    waiters: Vec<CancelToken>,
    closed: bool,
}

/// See the module.
pub(crate) struct WriteQueue {
    state: Mutex<State>,
    ready: Condvar,
    max_frames: usize,
    max_bytes: usize,
    stall: Duration,
    clock: Arc<dyn Clock>,
}

fn lock(m: &Mutex<State>) -> MutexGuard<'_, State> {
    // The state stays consistent if a holder panicked (every update is a few field writes).
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl WriteQueue {
    pub(crate) fn new(
        max_frames: usize,
        max_bytes: usize,
        stall: Duration,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            state: Mutex::new(State::default()),
            ready: Condvar::new(),
            max_frames: max_frames.max(1),
            max_bytes,
            stall,
            clock,
        }
    }

    /// Queues a frame, blocking while the queue is full (see the module).
    pub(crate) fn push(&self, bytes: Vec<u8>) -> Push {
        let deadline = Deadline::after(self.clock.clone(), self.stall);
        loop {
            let token = {
                let mut s = lock(&self.state);
                if s.closed {
                    return Push::Closed;
                }
                if s.frames < self.max_frames
                    && (s.frames == 0 || s.bytes.saturating_add(bytes.len()) <= self.max_bytes)
                {
                    s.frames += 1;
                    s.bytes += bytes.len();
                    s.items.push_back(Out::Frame(bytes));
                    drop(s);
                    self.ready.notify_one();
                    return Push::Queued;
                }
                let token = CancelToken::new();
                s.waiters.push(token.clone());
                token
            };
            // Woken by the writer (the wait is cancelled) or by the stall passing (the wait ends).
            let left = deadline.remaining().unwrap_or(Duration::MAX);
            if !deadline.passed() && self.clock.wait(left, &token).is_err() {
                continue;
            }
            if deadline.passed() {
                // One more look: room may have appeared as the time ran out.
                let s = lock(&self.state);
                if s.closed {
                    return Push::Closed;
                }
                let room = s.frames < self.max_frames
                    && (s.frames == 0 || s.bytes.saturating_add(bytes.len()) <= self.max_bytes);
                if !room {
                    return Push::Stalled;
                }
            }
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
        for w in waiters {
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
                for w in waiters {
                    w.cancel();
                }
                return out;
            }
            s = self.ready.wait(s).unwrap_or_else(|p| p.into_inner());
        }
    }

    /// Frames and bytes waiting now.
    #[cfg(test)]
    pub(crate) fn queued(&self) -> (usize, usize) {
        let s = lock(&self.state);
        (s.frames, s.bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_jobs::time::FakeClock;
    use std::time::Duration;

    const GUARD: Duration = Duration::from_secs(10);

    fn queue(frames: usize, bytes: usize, clock: &Arc<FakeClock>) -> Arc<WriteQueue> {
        Arc::new(WriteQueue::new(
            frames,
            bytes,
            Duration::from_secs(30),
            clock.clone(),
        ))
    }

    #[test]
    fn frames_come_out_in_order_and_the_counts_follow() {
        let clock = Arc::new(FakeClock::new());
        let q = queue(4, 100, &clock);
        assert_eq!(q.push(vec![1; 10]), Push::Queued);
        assert_eq!(q.push(vec![2; 20]), Push::Queued);
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
        assert_eq!(q.push(vec![0; 50]), Push::Queued);
        // Nothing else fits beside it: the next push stalls once the clock passes the stall.
        let q2 = q.clone();
        let t = std::thread::spawn(move || q2.push(vec![0; 1]));
        assert!(clock.wait_for_waiters(1, GUARD));
        clock.advance(Duration::from_secs(30));
        assert_eq!(t.join().unwrap(), Push::Stalled);
        assert_eq!(q.queued(), (1, 50));
    }

    #[test]
    fn a_full_queue_blocks_then_fails_at_the_stall_and_never_grows() {
        let clock = Arc::new(FakeClock::new());
        let q = queue(2, 1 << 20, &clock);
        assert_eq!(q.push(vec![0; 1]), Push::Queued);
        assert_eq!(q.push(vec![0; 1]), Push::Queued);
        let q2 = q.clone();
        let t = std::thread::spawn(move || q2.push(vec![0; 1]));
        assert!(clock.wait_for_waiters(1, GUARD));
        // Not before the stall.
        clock.advance(Duration::from_secs(29));
        assert!(!t.is_finished());
        clock.advance(Duration::from_secs(1));
        assert_eq!(t.join().unwrap(), Push::Stalled);
        assert_eq!(q.queued(), (2, 2));
    }

    #[test]
    fn room_freed_by_the_writer_wakes_the_blocked_caller() {
        let clock = Arc::new(FakeClock::new());
        let q = queue(1, 1 << 20, &clock);
        assert_eq!(q.push(vec![7; 1]), Push::Queued);
        let q2 = q.clone();
        let t = std::thread::spawn(move || q2.push(vec![8; 1]));
        assert!(clock.wait_for_waiters(1, GUARD));
        assert!(matches!(q.pop(), Out::Frame(b) if b[0] == 7));
        assert_eq!(t.join().unwrap(), Push::Queued);
        assert!(matches!(q.pop(), Out::Frame(b) if b[0] == 8));
    }

    #[test]
    fn shutdown_releases_waiters_and_stops_the_writer_after_what_is_queued() {
        let clock = Arc::new(FakeClock::new());
        let q = queue(1, 1 << 20, &clock);
        assert_eq!(q.push(vec![1; 1]), Push::Queued);
        let q2 = q.clone();
        let t = std::thread::spawn(move || q2.push(vec![2; 1]));
        assert!(clock.wait_for_waiters(1, GUARD));
        q.shutdown();
        assert_eq!(t.join().unwrap(), Push::Closed);
        assert_eq!(q.push(vec![3; 1]), Push::Closed);
        assert!(matches!(q.pop(), Out::Frame(_)));
        assert!(matches!(q.pop(), Out::Shutdown));
        q.shutdown();
    }

    #[test]
    fn the_writer_waits_for_a_frame() {
        let clock = Arc::new(FakeClock::new());
        let q = queue(2, 100, &clock);
        let q2 = q.clone();
        let t = std::thread::spawn(move || q2.pop());
        // Queue after the writer is waiting or before: either way it gets the frame.
        assert_eq!(q.push(vec![5; 3]), Push::Queued);
        assert!(matches!(t.join().unwrap(), Out::Frame(b) if b == vec![5; 3]));
    }
}
