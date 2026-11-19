//! Newer-only state (ADR 0034): a value that was read under a lock and is stored or pushed after the lock is let go carries the
//! generation it was read at, and a slot keeps it only when nothing newer has been stored. The generation is taken where the state is
//! read (under the lock that guards it), so the order of the generations is the order of the states, whichever thread stores last.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A counter of states: each call of [`Generation::next`] returns a number greater than every earlier one. Take it under the lock that
/// guards the state, in the same hold that reads or changes the state.
#[derive(Debug, Default)]
pub struct Generation(AtomicU64);

impl Generation {
    /// A counter that has issued nothing yet (the first number is 1; 0 is "before any state").
    pub const fn new() -> Generation {
        Generation(AtomicU64::new(0))
    }

    /// A new generation, greater than every one issued before.
    pub fn next(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// The newest generation issued so far (0 when none).
    pub fn current(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug)]
struct State<T> {
    /// The newest generation ever stored, kept after a [`NewerSlot::take`] so a late older value is still refused.
    newest: u64,
    value: Option<T>,
}

/// One value that is stored only when it is not older than the newest one stored so far. Used for values waiting to be pushed (the newest
/// replaces an older one that was not sent yet, an older one arriving late is dropped) and for installed state that a slow rebuild
/// replaces (a rebuild that read older inputs than the installed one cannot overwrite it).
#[derive(Debug)]
pub struct NewerSlot<T>(Mutex<State<T>>);

impl<T> Default for NewerSlot<T> {
    fn default() -> Self {
        NewerSlot(Mutex::new(State { newest: 0, value: None }))
    }
}

impl<T> NewerSlot<T> {
    /// An empty slot.
    pub fn new() -> NewerSlot<T> {
        NewerSlot::default()
    }

    /// Stores `value` read at `generation` unless a newer generation was stored before; true when it was kept. The same generation
    /// replaces (a value is its own newest).
    pub fn put(&self, generation: u64, value: T) -> bool {
        let mut s = lock(&self.0);
        if generation < s.newest {
            return false;
        }
        s.newest = generation;
        s.value = Some(value);
        true
    }

    /// The value waiting, if any; the slot is empty afterwards but remembers the newest generation.
    pub fn take(&self) -> Option<T> {
        lock(&self.0).value.take()
    }

    /// The newest generation stored so far (0 when none).
    pub fn newest(&self) -> u64 {
        lock(&self.0).newest
    }

    /// A copy of the value kept, if any, without emptying the slot.
    pub fn get(&self) -> Option<T>
    where
        T: Clone,
    {
        lock(&self.0).value.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn generations_only_grow() {
        let g = Generation::new();
        assert_eq!(g.current(), 0);
        let (a, b) = (g.next(), g.next());
        assert!(b > a && a > 0);
        assert_eq!(g.current(), b);
    }

    #[test]
    fn generations_are_unique_across_threads() {
        let g = Arc::new(Generation::new());
        let handles: Vec<_> = (0..4)
            .map(|_| {
                std::thread::spawn({
                    let g = g.clone();
                    move || (0..500).map(|_| g.next()).collect::<Vec<_>>()
                })
            })
            .collect();
        let mut all: Vec<u64> = handles.into_iter().flat_map(|h| h.join().unwrap()).collect();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), 2000);
    }

    /// The positive control: the older value put after the newer one is refused, and the newer one stays.
    #[test]
    fn an_older_generation_put_after_a_newer_one_is_refused() {
        let slot = NewerSlot::new();
        assert!(slot.put(2, "new"));
        assert!(!slot.put(1, "old"));
        assert_eq!(slot.take(), Some("new"));
    }

    /// The control for the control: without the generation check the old value would win, so the test above can fail.
    #[test]
    fn a_newer_generation_replaces_an_older_one() {
        let slot = NewerSlot::new();
        assert!(slot.put(1, "old"));
        assert!(slot.put(2, "new"));
        assert_eq!(slot.take(), Some("new"));
    }

    #[test]
    fn an_older_generation_is_still_refused_after_the_newer_one_was_taken() {
        let slot = NewerSlot::new();
        slot.put(5, "five");
        assert_eq!(slot.take(), Some("five"));
        assert!(!slot.put(4, "four"));
        assert_eq!(slot.take(), None);
        assert_eq!(slot.newest(), 5);
    }

    #[test]
    fn the_same_generation_replaces_and_get_does_not_empty() {
        let slot = NewerSlot::new();
        slot.put(3, "a");
        assert!(slot.put(3, "b"));
        assert_eq!(slot.get(), Some("b"));
        assert_eq!(slot.get(), Some("b"));
    }

    #[test]
    fn a_poisoned_lock_is_recovered() {
        let slot = Arc::new(NewerSlot::new());
        let s = slot.clone();
        let _ = std::thread::spawn(move || {
            let _g = s.0.lock().unwrap();
            panic!("poison");
        })
        .join();
        assert!(slot.put(1, 1));
        assert_eq!(slot.take(), Some(1));
    }
}
