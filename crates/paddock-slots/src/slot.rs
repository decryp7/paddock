use crate::SlotError;
use std::fmt;
use std::str::FromStr;

/// A slot name: `s1`, `s2`, ... (numbered from 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlotId(u32);

impl SlotId {
    /// Slot number `n`, or `None` for 0.
    pub fn new(n: u32) -> Option<SlotId> {
        (n > 0).then_some(SlotId(n))
    }

    /// The number, from 1.
    pub fn number(self) -> u32 {
        self.0
    }
}

impl fmt::Display for SlotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "s{}", self.0)
    }
}

impl FromStr for SlotId {
    type Err = SlotError;

    fn from_str(s: &str) -> Result<SlotId, SlotError> {
        s.strip_prefix('s')
            .filter(|n| n.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|n| n.parse().ok())
            .and_then(SlotId::new)
            .ok_or_else(|| SlotError::Invalid(s.to_string()))
    }
}

/// The port: try to take one slot without waiting. `Ok(None)` means somebody holds it. The guard releases the slot when
/// dropped (and the system does when the process dies).
pub trait SlotLocks {
    /// What holding a slot looks like.
    type Guard;

    /// Takes slot `id` if it is free.
    fn try_lock(&self, id: SlotId) -> Result<Option<Self::Guard>, SlotError>;
}

/// Takes the first free slot among `s1..=sMAX`, or `None` when all are busy (or `max` is 0).
pub fn acquire_first<L: SlotLocks>(
    locks: &L,
    max: u32,
) -> Result<Option<(SlotId, L::Guard)>, SlotError> {
    for n in 1..=max {
        let id = SlotId(n);
        if let Some(guard) = locks.try_lock(id)? {
            return Ok(Some((id, guard)));
        }
    }
    Ok(None)
}

/// Whether a slot is free or held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotState {
    /// The slot.
    pub id: SlotId,
    /// A holder has it.
    pub busy: bool,
}

/// Which of `s1..=sMAX` are busy. It tries each lock and lets go at once.
pub fn slot_states<L: SlotLocks>(locks: &L, max: u32) -> Result<Vec<SlotState>, SlotError> {
    (1..=max)
        .map(|n| {
            let id = SlotId(n);
            Ok(SlotState {
                id,
                busy: locks.try_lock(id)?.is_none(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeSet;

    struct Fake(RefCell<BTreeSet<u32>>, Option<u32>);

    impl SlotLocks for Fake {
        type Guard = u32;
        fn try_lock(&self, id: SlotId) -> Result<Option<u32>, SlotError> {
            if self.1 == Some(id.number()) {
                return Err(SlotError::Invalid("boom".into()));
            }
            Ok(self
                .0
                .borrow_mut()
                .insert(id.number())
                .then_some(id.number()))
        }
    }

    #[test]
    fn names_round_trip_and_bad_names_are_refused() {
        assert_eq!("s12".parse::<SlotId>().unwrap().number(), 12);
        assert_eq!(SlotId::new(3).unwrap().to_string(), "s3");
        assert!(SlotId::new(0).is_none());
        for bad in ["", "s", "s0", "1", "s-1", "sx", "S1", "s99999999999999"] {
            assert!(bad.parse::<SlotId>().is_err(), "{bad}");
        }
        assert!(SlotError::Invalid("x".into()).to_string().contains("s1"));
    }

    #[test]
    fn takes_the_first_free_slot_and_none_when_full() {
        let f = Fake(RefCell::new(BTreeSet::from([1, 3])), None);
        assert_eq!(acquire_first(&f, 3).unwrap().unwrap().0.to_string(), "s2");
        assert!(acquire_first(&f, 3).unwrap().is_none());
        assert_eq!(acquire_first(&f, 4).unwrap().unwrap().0.number(), 4);
        assert!(acquire_first(&f, 0).unwrap().is_none());
    }

    #[test]
    fn errors_propagate_and_states_list_busy_slots() {
        let f = Fake(RefCell::new(BTreeSet::new()), Some(2));
        assert!(acquire_first(&f, 2).is_ok_and(|x| x.is_some()));
        assert!(acquire_first(&f, 3).is_err());
        let f = Fake(RefCell::new(BTreeSet::from([2])), None);
        let s = slot_states(&f, 2).unwrap();
        assert_eq!(s.iter().map(|s| s.busy).collect::<Vec<_>>(), [false, true]);
        assert!(slot_states(&Fake(RefCell::new(BTreeSet::new()), Some(1)), 1).is_err());
    }
}
