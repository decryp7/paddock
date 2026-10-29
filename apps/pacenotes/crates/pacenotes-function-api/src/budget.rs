//! [`Budget`]: the bound every call across the port carries.

use serde::{Deserialize, Serialize};

/// The most a single call may be given.
pub const MAX_MILLIS: u32 = 10 * 60 * 1000;
/// The most items (rows, hits, symbols) a single call may be asked for.
pub const MAX_ITEMS: u32 = 100_000;
/// The most bytes a single reply may carry.
pub const MAX_BYTES: u32 = 64 * 1024 * 1024;

/// What a budget runs out of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BudgetKind {
    /// Time.
    Millis,
    /// Items.
    Items,
    /// Bytes.
    Bytes,
}

/// Why a budget is not valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BudgetError {
    /// A limit is 0 or above its maximum.
    #[error("the {0:?} limit must be between 1 and its maximum")]
    OutOfRange(BudgetKind),
}

/// The limits of one call: wall time the caller will wait, items and bytes the reply may hold. A call that would exceed
/// one stops and says which ([`HostError::BudgetExceeded`](crate::HostError::BudgetExceeded)); it never runs unbounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    millis: u32,
    items: u32,
    bytes: u32,
}

impl Budget {
    /// A budget; each limit must be 1 up to its maximum.
    pub fn new(millis: u32, items: u32, bytes: u32) -> Result<Budget, BudgetError> {
        for (kind, value, max) in [
            (BudgetKind::Millis, millis, MAX_MILLIS),
            (BudgetKind::Items, items, MAX_ITEMS),
            (BudgetKind::Bytes, bytes, MAX_BYTES),
        ] {
            if value == 0 || value > max {
                return Err(BudgetError::OutOfRange(kind));
            }
        }
        Ok(Budget { millis, items, bytes })
    }

    /// The default for an interactive call: 5 seconds, 1000 items, 1 MiB.
    pub const INTERACTIVE: Budget = Budget {
        millis: 5_000,
        items: 1_000,
        bytes: 1 << 20,
    };

    /// Milliseconds the caller will wait.
    pub fn millis(&self) -> u32 {
        self.millis
    }

    /// Items the reply may hold.
    pub fn items(&self) -> u32 {
        self.items
    }

    /// Bytes the reply may hold.
    pub fn bytes(&self) -> u32 {
        self.bytes
    }

    /// Checks a reply size against the budget: the first limit it exceeds, if any.
    pub fn exceeded_by(&self, items: usize, bytes: usize) -> Option<BudgetKind> {
        if items > self.items as usize {
            Some(BudgetKind::Items)
        } else if bytes > self.bytes as usize {
            Some(BudgetKind::Bytes)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_are_required_and_bounded() {
        assert!(Budget::new(1, 1, 1).is_ok());
        assert_eq!(Budget::new(0, 1, 1), Err(BudgetError::OutOfRange(BudgetKind::Millis)));
        assert_eq!(Budget::new(1, MAX_ITEMS + 1, 1), Err(BudgetError::OutOfRange(BudgetKind::Items)));
        assert_eq!(Budget::new(1, 1, MAX_BYTES + 1), Err(BudgetError::OutOfRange(BudgetKind::Bytes)));
        assert!(Budget::new(MAX_MILLIS, MAX_ITEMS, MAX_BYTES).is_ok());
    }

    #[test]
    fn replies_are_checked_against_items_then_bytes() {
        let b = Budget::new(10, 2, 100).unwrap();
        assert_eq!(b.exceeded_by(2, 100), None);
        assert_eq!(b.exceeded_by(3, 0), Some(BudgetKind::Items));
        assert_eq!(b.exceeded_by(0, 101), Some(BudgetKind::Bytes));
        assert_eq!((b.millis(), b.items(), b.bytes()), (10, 2, 100));
        assert_eq!(Budget::INTERACTIVE.millis(), 5_000);
    }
}
