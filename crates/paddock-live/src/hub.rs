//! The hub: state, sequence numbers and bounded subscriber queues.

use crate::frame::{Frame, Link};
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};

/// Frames a subscriber may have queued before it is resynchronised with a snapshot.
pub const DEFAULT_QUEUE_CAP: usize = 256;
/// The smallest queue cap ([`Hub::with_queue_cap`] clamps to it).
pub const MIN_QUEUE_CAP: usize = 4;
/// The largest queue cap.
pub const MAX_QUEUE_CAP: usize = 4096;

/// The state of one feed. The same fold is written once more in the UI (`foldFrame`'s `apply`); a shared fixture test keeps
/// the two equal.
pub trait LiveState: Clone + Send + Serialize + 'static {
    /// One change.
    type Delta: Clone + Send + Serialize + 'static;

    /// Folds one change into the state.
    fn apply(&mut self, delta: &Self::Delta);

    /// Merges `next` into `prev` when the two can travel as one frame; false keeps them apart. A merge must mean "the
    /// latest wins" (a memory sample replaces the previous one): a window may apply a merged frame that overlaps a snapshot
    /// it already has, so applying it twice must not change the result. Counters and appends must not merge.
    fn merge(_prev: &mut Self::Delta, _next: &Self::Delta) -> bool {
        false
    }
}

type F<S> = Frame<S, <S as LiveState>::Delta>;

struct Queue<S: LiveState> {
    frames: VecDeque<F<S>>,
    closed: bool,
    resyncs: u64,
}

struct Chan<S: LiveState> {
    queue: Mutex<Queue<S>>,
    ready: Condvar,
}

impl<S: LiveState> Chan<S> {
    fn lock(&self) -> MutexGuard<'_, Queue<S>> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Replaces everything queued with `snapshot`.
    fn replace_with(&self, snapshot: F<S>) {
        let mut q = self.lock();
        q.frames.clear();
        q.frames.push_back(snapshot);
        drop(q);
        self.ready.notify_all();
    }

    fn push_delta(&self, seq: u64, delta: &S::Delta, cap: usize, snapshot: &dyn Fn() -> F<S>) {
        let mut q = self.lock();
        if let Some(Frame::Delta {
            seq: last,
            delta: prev,
            ..
        }) = q.frames.back_mut()
            && S::merge(prev, delta)
        {
            *last = seq;
        } else if q.frames.len() >= cap {
            // A consumer that cannot keep up gets the state, not a longer queue.
            q.frames.clear();
            q.frames.push_back(snapshot());
            q.resyncs += 1;
        } else {
            q.frames.push_back(Frame::Delta {
                from: seq,
                seq,
                delta: delta.clone(),
            });
        }
        drop(q);
        self.ready.notify_all();
    }

    fn push_link(&self, link: &Link, cap: usize, snapshot: &dyn Fn() -> F<S>) {
        let mut q = self.lock();
        if let Some(Frame::Link { link: last }) = q.frames.back_mut() {
            *last = link.clone();
        } else if q.frames.len() >= cap {
            q.frames.clear();
            q.frames.push_back(snapshot());
            q.resyncs += 1;
        } else {
            q.frames.push_back(Frame::Link { link: link.clone() });
        }
        drop(q);
        self.ready.notify_all();
    }

    fn close(&self) {
        self.lock().closed = true;
        self.ready.notify_all();
    }
}

struct Core<S: LiveState> {
    state: S,
    seq: u64,
    link: Link,
    cap: usize,
    closed: bool,
    subs: Vec<Weak<Chan<S>>>,
}

impl<S: LiveState> Core<S> {
    fn snapshot(&self) -> F<S> {
        Frame::Snapshot {
            seq: self.seq,
            state: self.state.clone(),
            link: self.link.clone(),
        }
    }

    /// The live subscribers (the dropped ones are forgotten).
    fn live(&mut self) -> Vec<Arc<Chan<S>>> {
        self.subs.retain(|w| w.strong_count() > 0);
        self.subs.iter().filter_map(Weak::upgrade).collect()
    }
}

/// The state of a feed and the queues of whoever listens. Cheap to clone: clones share the feed.
pub struct Hub<S: LiveState>(Arc<Mutex<Core<S>>>);

impl<S: LiveState> Clone for Hub<S> {
    fn clone(&self) -> Self {
        Hub(self.0.clone())
    }
}

impl<S: LiveState> Hub<S> {
    /// A hub whose state starts as `initial` (what a window shows before the first seed), link [`Link::Connecting`],
    /// sequence 0.
    pub fn new(initial: S) -> Self {
        Self::with_queue_cap(initial, DEFAULT_QUEUE_CAP)
    }

    /// As [`new`](Self::new) with a queue cap per subscriber, clamped to [`MIN_QUEUE_CAP`]..=[`MAX_QUEUE_CAP`].
    pub fn with_queue_cap(initial: S, cap: usize) -> Self {
        Hub(Arc::new(Mutex::new(Core {
            state: initial,
            seq: 0,
            link: Link::Connecting,
            cap: cap.clamp(MIN_QUEUE_CAP, MAX_QUEUE_CAP),
            closed: false,
            subs: Vec::new(),
        })))
    }

    fn core(&self) -> MutexGuard<'_, Core<S>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Folds `delta` into the state, numbers it and queues it for every subscriber. Returns its sequence number (the
    /// current one after [`close`](Self::close), which ignores it).
    pub fn publish(&self, delta: S::Delta) -> u64 {
        let mut core = self.core();
        if core.closed {
            return core.seq;
        }
        core.state.apply(&delta);
        core.seq += 1;
        let (seq, cap) = (core.seq, core.cap);
        let subs = core.live();
        let snapshot = || core.snapshot();
        for sub in subs {
            sub.push_delta(seq, &delta, cap, &snapshot);
        }
        seq
    }

    /// Replaces the state (a reconnect that seeds again) and sends every subscriber a snapshot in place of whatever it had
    /// queued.
    pub fn reset(&self, state: S) -> u64 {
        let mut core = self.core();
        let link = core.link.clone();
        Self::reset_locked(&mut core, state, link)
    }

    /// As [`reset`](Self::reset), and sets the link in the same snapshot (so a window never sees the new state with the old
    /// link).
    pub fn reset_with(&self, state: S, link: Link) -> u64 {
        let mut core = self.core();
        Self::reset_locked(&mut core, state, link)
    }

    fn reset_locked(core: &mut Core<S>, state: S, link: Link) -> u64 {
        if core.closed {
            return core.seq;
        }
        core.state = state;
        core.link = link;
        core.seq += 1;
        let snapshot = core.snapshot();
        for sub in core.live() {
            sub.replace_with(snapshot.clone());
        }
        core.seq
    }

    /// Sets the link; a window is told only when it changed.
    pub fn set_link(&self, link: Link) {
        let mut core = self.core();
        if core.closed || core.link == link {
            return;
        }
        core.link = link.clone();
        let cap = core.cap;
        let subs = core.live();
        let snapshot = || core.snapshot();
        for sub in subs {
            sub.push_link(&link, cap, &snapshot);
        }
    }

    /// The link now.
    pub fn link(&self) -> Link {
        self.core().link.clone()
    }

    /// The sequence number of the state now.
    pub fn seq(&self) -> u64 {
        self.core().seq
    }

    /// A copy of the state now.
    pub fn state(&self) -> S {
        self.core().state.clone()
    }

    /// The whole state as a snapshot frame: what a window asks for when it opens, reloads or sees a gap.
    pub fn snapshot(&self) -> F<S> {
        self.core().snapshot()
    }

    /// A new subscriber. Its first frame is the snapshot of now; after it come the deltas, in order.
    pub fn subscribe(&self) -> Subscription<S> {
        let mut core = self.core();
        let chan = Arc::new(Chan {
            queue: Mutex::new(Queue {
                frames: VecDeque::from([core.snapshot()]),
                closed: core.closed,
                resyncs: 0,
            }),
            ready: Condvar::new(),
        });
        core.subs.push(Arc::downgrade(&chan));
        Subscription { chan }
    }

    /// How many subscribers are listening.
    pub fn subscribers(&self) -> usize {
        self.core().live().len()
    }

    /// Ends the feed: nothing more is accepted and every subscriber's [`next`](Subscription::next) returns `None` once it
    /// has read what is queued.
    pub fn close(&self) {
        let mut core = self.core();
        core.closed = true;
        for sub in core.live() {
            sub.close();
        }
    }

    /// Whether [`close`](Self::close) was called.
    pub fn is_closed(&self) -> bool {
        self.core().closed
    }
}

/// One listener's queue: at most the hub's cap of frames, in order. Dropping it unsubscribes.
pub struct Subscription<S: LiveState> {
    chan: Arc<Chan<S>>,
}

impl<S: LiveState> Subscription<S> {
    /// The next frame if one is queued.
    pub fn try_next(&self) -> Option<F<S>> {
        self.chan.lock().frames.pop_front()
    }

    /// The next frame, waiting for one; `None` when the hub is closed and the queue is empty.
    pub fn next(&self) -> Option<F<S>> {
        let mut q = self.chan.lock();
        loop {
            if let Some(f) = q.frames.pop_front() {
                return Some(f);
            }
            if q.closed {
                return None;
            }
            q = self
                .chan
                .ready
                .wait(q)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Everything queued now, in order.
    pub fn drain(&self) -> Vec<F<S>> {
        self.chan.lock().frames.drain(..).collect()
    }

    /// Frames waiting.
    pub fn pending(&self) -> usize {
        self.chan.lock().frames.len()
    }

    /// How often this subscriber fell behind and was given a snapshot in place of its queue.
    pub fn resyncs(&self) -> u64 {
        self.chan.lock().resyncs
    }
}

impl<S: LiveState> Drop for Subscription<S> {
    fn drop(&mut self) {
        self.chan.close();
    }
}
