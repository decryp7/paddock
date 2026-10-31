//! A pure state machine, one owner thread and the transitions it publishes: the shape of a supervisor (ADR 0025).
//!
//! Nothing in this module names Dyno. It is kept apart so that it moves to `paddock-proc` as it is, once a second
//! adopter exists (the family's rule of three: Pitcrew's controller and Pitwall's supervisor have the same shape).
//!
//! * [`Machine`] is the pure part: `step(event) -> commands`, no clock, no I/O, no threads.
//! * [`run_owner`] is the only writer: one thread takes events from one channel, calls `step`, executes the commands
//!   through whatever the caller passes in (ports) and publishes the new snapshot on a [`Transitions`] channel.
//! * [`Transitions`] is the published state. Observers (a status chip, a request that must wait for a connection, a
//!   test) wait on a condition of the snapshot instead of polling flags.
//! * [`spawn_ticker`] paces the machine: one tick per observation, at most one waiting.
//! * `explore` (tests only) walks every interleaving of a world model up to a bound.

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

#[cfg(test)]
pub mod explore;

/// A supervisor's pure core. Only [`Machine::step`] changes it.
pub trait Machine: Send + 'static {
    /// What happens in the world, or what a person asks.
    type Event: Send + 'static;
    /// What the machine asks the world to do.
    type Command: Send + 'static;
    /// What observers see after every step.
    type Snapshot: Clone + Send + 'static;

    /// Takes one event, changes the state, returns the commands to execute (in order).
    fn step(&mut self, event: Self::Event) -> Vec<Self::Command>;
    /// The state observers may see.
    fn snapshot(&self) -> Self::Snapshot;
    /// Whether the machine has ended (no event changes it any more); the owner thread then stops.
    fn finished(&self) -> bool;
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // Every guarded value stays consistent if a holder panicked.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct Shared<S> {
    state: Mutex<Published<S>>,
    changed: Condvar,
}

struct Published<S> {
    snapshot: S,
    /// How many snapshots have been published; it grows with every step.
    steps: u64,
    /// The owner thread has ended: nothing will be published any more.
    closed: bool,
}

/// The published state of a machine: the latest snapshot and a count of steps. Cloning shares it.
pub struct Transitions<S> {
    shared: Arc<Shared<S>>,
}

impl<S> Clone for Transitions<S> {
    fn clone(&self) -> Self {
        Transitions {
            shared: self.shared.clone(),
        }
    }
}

impl<S: Clone> Transitions<S> {
    /// A channel whose first snapshot is `initial`.
    pub fn new(initial: S) -> Transitions<S> {
        Transitions {
            shared: Arc::new(Shared {
                state: Mutex::new(Published {
                    snapshot: initial,
                    steps: 0,
                    closed: false,
                }),
                changed: Condvar::new(),
            }),
        }
    }

    /// Publishes the snapshot after a step and wakes every waiter.
    pub fn publish(&self, snapshot: S) {
        let mut p = lock(&self.shared.state);
        p.snapshot = snapshot;
        p.steps += 1;
        drop(p);
        self.shared.changed.notify_all();
    }

    /// No more snapshots will come (the owner ended): waiters stop waiting.
    pub fn close(&self) {
        lock(&self.shared.state).closed = true;
        self.shared.changed.notify_all();
    }

    /// The latest snapshot.
    pub fn current(&self) -> S {
        lock(&self.shared.state).snapshot.clone()
    }

    /// How many steps have been published.
    pub fn steps(&self) -> u64 {
        lock(&self.shared.state).steps
    }

    /// Waits until `condition` holds for a snapshot (or the owner has ended), and returns that snapshot. It waits on the
    /// condition, never on a clock; a caller that must not wait for ever puts its own bound around it
    /// ([`Transitions::wait_until_for`]).
    pub fn wait_until(&self, condition: impl Fn(&S) -> bool) -> S {
        let mut p = lock(&self.shared.state);
        while !p.closed && !condition(&p.snapshot) {
            p = self.shared.changed.wait(p).unwrap_or_else(|e| e.into_inner());
        }
        p.snapshot.clone()
    }

    /// [`Transitions::wait_until`] with a bound: `None` when `bound` passed without the condition holding. For tests'
    /// hang guards and for a caller with a recorded product limit.
    pub fn wait_until_for(&self, bound: Duration, condition: impl Fn(&S) -> bool) -> Option<S> {
        let end = Instant::now() + bound;
        let mut p = lock(&self.shared.state);
        loop {
            if condition(&p.snapshot) {
                return Some(p.snapshot.clone());
            }
            let left = end.saturating_duration_since(Instant::now());
            if p.closed || left.is_zero() {
                return None;
            }
            p = self.shared.changed.wait_timeout(p, left).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    /// Waits until `more` further steps have been published; false when `bound` passed first or the owner ended. For tests
    /// that let a wait run on without a clock: they wait for observations, not for seconds.
    pub fn wait_steps(&self, more: u64, bound: Duration) -> bool {
        let target = self.steps() + more;
        self.wait_until_for_steps(bound, target)
    }

    fn wait_until_for_steps(&self, bound: Duration, target: u64) -> bool {
        let end = Instant::now() + bound;
        let mut p = lock(&self.shared.state);
        while p.steps < target {
            let left = end.saturating_duration_since(Instant::now());
            if p.closed || left.is_zero() {
                return false;
            }
            p = self.shared.changed.wait_timeout(p, left).unwrap_or_else(|e| e.into_inner()).0;
        }
        true
    }

    /// Waits until more than `seen` steps have been published; false when the owner has ended.
    fn wait_step_after(&self, seen: u64) -> bool {
        let mut p = lock(&self.shared.state);
        while !p.closed && p.steps <= seen {
            p = self.shared.changed.wait(p).unwrap_or_else(|e| e.into_inner());
        }
        !p.closed
    }
}

/// Ends the transitions when the owner thread ends, however it ends (a panic included), so nobody waits for ever.
struct CloseOnDrop<S: Clone>(Transitions<S>);

impl<S: Clone> Drop for CloseOnDrop<S> {
    fn drop(&mut self) {
        self.0.close();
    }
}

/// Runs `machine` on its own thread: takes each event from `events`, steps the machine, executes the commands in order
/// with `execute` (which must not block: a slow port runs on a worker and answers with an event) and then publishes the
/// snapshot, so an observer that sees a state sees the effects of the step that led to it. The thread ends when the
/// machine is finished or every sender is gone.
pub fn run_owner<M: Machine>(
    name: &str,
    mut machine: M,
    events: Receiver<M::Event>,
    transitions: Transitions<M::Snapshot>,
    mut execute: impl FnMut(M::Command) + Send + 'static,
    mut on_step: impl FnMut(&M::Snapshot) + Send + 'static,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new().name(name.into()).spawn(move || {
        let _closing = CloseOnDrop(transitions.clone());
        while let Ok(event) = events.recv() {
            for command in machine.step(event) {
                execute(command);
            }
            let snapshot = machine.snapshot();
            on_step(&snapshot);
            transitions.publish(snapshot);
            if machine.finished() {
                break;
            }
        }
    })
}

/// Sends `tick()` to the machine every `pace`, but only once the owner has taken the last one (at most one tick waits, so
/// a slow owner is never flooded). Ends when the owner ends. The pace is how often the world is looked at; it bounds
/// nothing (ADR 0031).
pub fn spawn_ticker<E: Send + 'static, S: Clone + Send + 'static>(
    name: &str,
    pace: Duration,
    events: Sender<E>,
    tick: impl Fn() -> E + Send + 'static,
    transitions: Transitions<S>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new().name(name.into()).spawn(move || {
        loop {
            // pace: how often the world is looked at.
            std::thread::sleep(pace);
            let seen = transitions.steps();
            if events.send(tick()).is_err() || !transitions.wait_step_after(seen) {
                break;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    /// A counter that counts events and ends at 3.
    struct Counter(u32);

    impl Machine for Counter {
        type Event = u32;
        type Command = u32;
        type Snapshot = u32;

        fn step(&mut self, event: u32) -> Vec<u32> {
            self.0 += event;
            vec![self.0]
        }
        fn snapshot(&self) -> u32 {
            self.0
        }
        fn finished(&self) -> bool {
            self.0 >= 3
        }
    }

    #[test]
    fn the_owner_executes_commands_then_publishes_and_stops_when_finished() {
        let (tx, rx) = channel();
        let transitions = Transitions::new(0);
        let done = Arc::new(Mutex::new(Vec::new()));
        let seen = done.clone();
        let handle = run_owner("t", Counter(0), rx, transitions.clone(), move |c| lock(&seen).push(c), |_| {}).unwrap();
        tx.send(1).unwrap();
        assert_eq!(transitions.wait_until(|s| *s >= 1), 1);
        tx.send(2).unwrap();
        handle.join().unwrap();
        assert_eq!(*lock(&done), [1, 3], "commands run in order, before the snapshot that follows them");
        assert_eq!(transitions.current(), 3);
        assert_eq!(transitions.steps(), 2);
        // The owner ended: a waiter for a state that never comes stops waiting.
        assert_eq!(transitions.wait_until(|s| *s > 100), 3);
        assert!(transitions.wait_until_for(Duration::from_millis(1), |s| *s > 100).is_none());
    }

    #[test]
    fn a_panicking_owner_does_not_leave_waiters_waiting_for_ever() {
        struct Boom;
        impl Machine for Boom {
            type Event = ();
            type Command = ();
            type Snapshot = u8;
            fn step(&mut self, _: ()) -> Vec<()> {
                panic!("a bug in a step");
            }
            fn snapshot(&self) -> u8 {
                0
            }
            fn finished(&self) -> bool {
                false
            }
        }
        let (tx, rx) = channel();
        let transitions = Transitions::new(0u8);
        let handle = run_owner("t", Boom, rx, transitions.clone(), |()| (), |_| {}).unwrap();
        tx.send(()).unwrap();
        assert_eq!(transitions.wait_until(|s| *s == 9), 0, "the wait ended with the owner");
        assert!(handle.join().is_err());
    }

    #[test]
    fn the_ticker_sends_one_tick_at_a_time_and_ends_with_the_owner() {
        let (tx, rx) = channel();
        let transitions = Transitions::new(0);
        let owner = run_owner("t", Counter(0), rx, transitions.clone(), |_| (), |_| {}).unwrap();
        let ticker = spawn_ticker("tick", Duration::from_millis(1), tx, || 1, transitions.clone()).unwrap();
        owner.join().unwrap();
        ticker.join().unwrap();
        assert_eq!(transitions.current(), 3);
    }
}
