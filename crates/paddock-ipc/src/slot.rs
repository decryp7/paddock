//! One call's completion slot (ADR 0106): the one place where a call's events (items, then one last event) are put and
//! taken. The side that ends the call (an answer, a failure, a close) and the caller share it; nothing is looked up in a
//! map to be woken, so an end can never fire before the caller has registered for it, or reach a registration the caller no
//! longer waits on.
//!
//! The invariants, all kept under the slot's one lock:
//! - **One completion.** After a last event nothing more is accepted (like `tokio::sync::oneshot`): a late answer after a
//!   close, or a close after the answer, is dropped and [`CallSlot::post`] says so.
//! - **No lost wake-up.** A wait on the clock takes its wake-up token from the slot while it sees the slot empty, under
//!   the lock a post takes; every post cancels the token in the slot. So a post either comes before the look (the wait
//!   takes the event) or after it (it cancels the token the wait holds).
//! - **An end even without a post.** [`SlotSender`] (the end held by whoever may answer) posts `gone` when it is dropped
//!   without having posted a last event, like a channel whose sender hung up: a close that panics half way still ends
//!   every caller.
//!
//! The waits are on the injected clock (a test's fake clock never moves by itself) or, without a time limit, on the slot's
//! condition variable.

use paddock_jobs::CancelToken;
use paddock_jobs::time::Clock;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

struct State<E> {
    events: VecDeque<E>,
    /// A last event was put: nothing more is accepted.
    ended: bool,
    /// Cancelled by every post; replaced (while the slot is empty) by a wait that finds it cancelled.
    wake: CancelToken,
}

struct Shared<E> {
    state: Mutex<State<E>>,
    cv: Condvar,
}

/// The caller's end of a call's slot.
pub(crate) struct CallSlot<E> {
    shared: Arc<Shared<E>>,
}

/// The answering end of a call's slot: kept by whoever may end the call (the peer's map of pending calls). Dropped
/// without a last event, it ends the call with `gone()`.
pub(crate) struct SlotSender<E> {
    shared: Arc<Shared<E>>,
    gone: fn() -> E,
}

/// A new slot: the caller's end and the answering end. `gone` is the last event a dropped sender puts.
pub(crate) fn slot<E>(gone: fn() -> E) -> (CallSlot<E>, SlotSender<E>) {
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            events: VecDeque::new(),
            ended: false,
            wake: CancelToken::new(),
        }),
        cv: Condvar::new(),
    });
    (
        CallSlot {
            shared: shared.clone(),
        },
        SlotSender { shared, gone },
    )
}

fn put<E>(shared: &Shared<E>, event: E, last: bool) -> bool {
    let mut st = lock(&shared.state);
    if st.ended {
        return false;
    }
    st.events.push_back(event);
    st.ended = last;
    st.wake.cancel();
    drop(st);
    shared.cv.notify_all();
    true
}

impl<E> SlotSender<E> {
    /// Puts `event` (`last`: the call's end). False when the call had already ended: the event is dropped.
    pub(crate) fn post(&self, event: E, last: bool) -> bool {
        put(&self.shared, event, last)
    }

    /// Whether a last event was put (or the call was abandoned).
    pub(crate) fn is_ended(&self) -> bool {
        lock(&self.shared.state).ended
    }

    /// Ends a call nobody reads any more (its caller gave it up) without putting an event.
    pub(crate) fn abandon(&self) {
        lock(&self.shared.state).ended = true;
        self.shared.cv.notify_all();
    }
}

impl<E> Drop for SlotSender<E> {
    fn drop(&mut self) {
        // silent: false only when a last event was put already, which is the normal end
        #[allow(clippy::let_underscore_must_use, clippy::let_underscore_untyped)]
        let _ = put(&self.shared, (self.gone)(), true);
    }
}

impl<E> CallSlot<E> {
    /// The next event if one is there.
    pub(crate) fn try_take(&self) -> Option<E> {
        lock(&self.shared.state).events.pop_front()
    }

    /// The next event, waiting as long as it takes. It always comes: a sender that is dropped puts its `gone` event.
    pub(crate) fn take(&self) -> E {
        let mut st = lock(&self.shared.state);
        loop {
            if let Some(e) = st.events.pop_front() {
                return e;
            }
            st = self.shared.cv.wait(st).unwrap_or_else(|p| p.into_inner());
        }
    }

    /// The next event, waiting at most `timeout` on `clock`; `None` when the time passed first. `before_wait` runs after
    /// the slot was seen empty and the wake-up taken, right before the wait (a test seam; `|| {}` elsewhere).
    pub(crate) fn take_within(
        &self,
        clock: &dyn Clock,
        timeout: Duration,
        before_wait: impl FnOnce(),
    ) -> Option<E> {
        let token = {
            let mut st = lock(&self.shared.state);
            if let Some(e) = st.events.pop_front() {
                return Some(e);
            }
            // Empty, so a cancelled token is spent (its event was taken): a fresh one, under the lock every post takes.
            if st.wake.is_cancelled() {
                st.wake = CancelToken::new();
            }
            st.wake.clone()
        };
        before_wait();
        // silent: Err(Cancelled) is the wake-up; the slot is read right after
        #[allow(clippy::let_underscore_must_use, clippy::let_underscore_untyped)]
        let _ = clock.wait(timeout, &token);
        self.try_take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_jobs::time::FakeClock;

    #[derive(Debug, PartialEq)]
    enum Ev {
        Item(u8),
        Last(u8),
        Gone,
    }

    fn new() -> (CallSlot<Ev>, SlotSender<Ev>) {
        slot(|| Ev::Gone)
    }

    const GUARD: Duration = Duration::from_secs(30);

    /// Runs `f` on a thread of its own; its result arrives on the returned channel, so a wait that never ends fails the
    /// test at the guard instead of hanging it.
    fn spawn_wait<T: Send + 'static>(
        f: impl FnOnce() -> T + Send + 'static,
    ) -> std::sync::mpsc::Receiver<T> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx
    }

    #[test]
    fn an_event_put_before_the_wait_is_taken_without_waiting() {
        let clock = FakeClock::new();
        let (rx, tx) = new();
        assert!(tx.post(Ev::Last(1), true));
        assert_eq!(
            rx.take_within(&clock, Duration::from_secs(3600), || {}),
            Some(Ev::Last(1))
        );
        assert_eq!(clock.waiters(), 0);
    }

    /// The 8214 shape at the slot: the wait has taken its wake-up and is about to block when the end is put. The end
    /// cancels exactly the token the wait holds, so the wait ends without the clock moving.
    #[test]
    fn an_end_put_after_the_wait_took_its_wake_up_ends_the_wait_without_the_clock_moving() {
        let clock = FakeClock::new();
        let (rx, tx) = new();
        let (took_tx, took) = std::sync::mpsc::channel();
        let (go_tx, go) = std::sync::mpsc::channel::<()>();
        let waiter = {
            let clock = clock.clone();
            spawn_wait(move || {
                rx.take_within(&clock, Duration::from_secs(3600), || {
                    let _ = took_tx.send(());
                    let _ = go.recv_timeout(GUARD);
                })
            })
        };
        took.recv_timeout(GUARD).expect("the wait took its wake-up");
        // The end comes before the wait has even blocked on the clock.
        assert!(tx.post(Ev::Last(7), true));
        go_tx.send(()).unwrap();
        assert_eq!(
            waiter
                .recv_timeout(GUARD)
                .expect("the wait ended (a hang guard, not a timing)"),
            Some(Ev::Last(7))
        );
        assert_eq!(
            clock.monotonic(),
            Duration::ZERO,
            "the fake clock was never moved"
        );
    }

    #[test]
    fn an_end_put_while_the_wait_blocks_on_the_clock_ends_it() {
        let clock = FakeClock::new();
        let (rx, tx) = new();
        let waiter = {
            let clock = clock.clone();
            spawn_wait(move || rx.take_within(&clock, Duration::from_secs(3600), || {}))
        };
        assert!(
            clock.wait_for_waiters(1, GUARD),
            "the wait blocked on the clock"
        );
        assert!(tx.post(Ev::Last(2), true));
        assert_eq!(
            waiter
                .recv_timeout(GUARD)
                .expect("the wait ended (a hang guard, not a timing)"),
            Some(Ev::Last(2))
        );
    }

    /// One completion: nothing is accepted after the last event, neither a second answer nor the sender's own `gone`.
    #[test]
    fn after_the_last_event_nothing_more_is_accepted() {
        let (rx, tx) = new();
        assert!(tx.post(Ev::Item(1), false));
        assert!(tx.post(Ev::Last(2), true));
        assert!(!tx.post(Ev::Last(3), true), "a second end is refused");
        assert!(
            !tx.post(Ev::Item(4), false),
            "an item after the end is refused"
        );
        drop(tx);
        assert_eq!(rx.try_take(), Some(Ev::Item(1)));
        assert_eq!(rx.try_take(), Some(Ev::Last(2)));
        assert_eq!(rx.try_take(), None, "no gone after a real end");
    }

    /// A sender dropped without an end (a close that panicked half way) still ends a wait that has no time limit.
    #[test]
    fn a_sender_dropped_without_an_end_ends_the_call_with_gone() {
        let (rx, tx) = new();
        let waiter = spawn_wait(move || rx.take());
        drop(tx);
        assert_eq!(
            waiter
                .recv_timeout(GUARD)
                .expect("the wait ended (a hang guard, not a timing)"),
            Ev::Gone
        );
    }

    /// A taken item's spent token is replaced: the next wait on an empty slot waits on the clock instead of returning at
    /// once as if its time had passed.
    #[test]
    fn after_an_item_was_taken_the_next_wait_waits_again() {
        let clock = FakeClock::new();
        let (rx, tx) = new();
        assert!(tx.post(Ev::Item(1), false));
        assert_eq!(
            rx.take_within(&clock, Duration::from_secs(5), || {}),
            Some(Ev::Item(1))
        );
        let waiter = {
            let clock = clock.clone();
            spawn_wait(move || rx.take_within(&clock, Duration::from_secs(5), || {}))
        };
        assert!(
            clock.wait_for_waiters(1, GUARD),
            "the second wait blocked on the clock"
        );
        clock.advance(Duration::from_secs(5));
        assert_eq!(
            waiter
                .recv_timeout(GUARD)
                .expect("the wait ended (a hang guard, not a timing)"),
            None,
            "the time passed with nothing put"
        );
        drop(tx);
    }
}
