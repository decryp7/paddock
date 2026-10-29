//! Bounded file-system walking shared by the disk map and the file finder: an entry budget with a deadline, and
//! modification times.

use dyno_core::time::Timestamp;
use std::fs::Metadata;
use std::time::{Instant, SystemTime};

/// What a measurement may spend.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    /// The most entries to look at.
    pub entries: usize,
    /// Stop at this time.
    pub deadline: Instant,
}

impl Budget {
    /// A budget of `entries` entries and `secs` seconds from now.
    pub fn new(entries: usize, secs: u64) -> Budget {
        Budget {
            entries,
            deadline: Instant::now() + std::time::Duration::from_secs(secs),
        }
    }

    /// Spends one entry; false when the budget is used up.
    pub fn spend(&mut self) -> bool {
        if self.entries == 0 || Instant::now() >= self.deadline {
            self.entries = 0;
            return false;
        }
        self.entries -= 1;
        true
    }

    /// Whether the budget ran out.
    pub fn exhausted(&self) -> bool {
        self.entries == 0 || Instant::now() >= self.deadline
    }
}

/// A modification time as a [`Timestamp`] (the epoch when the OS can't tell).
pub fn modified(meta: &Metadata) -> Timestamp {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(Timestamp(0), |d| Timestamp(i64::try_from(d.as_millis()).unwrap_or(i64::MAX)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_used_up_budget_says_so() {
        let mut few = Budget::new(2, 5);
        assert!(few.spend() && few.spend() && !few.spend() && few.exhausted());
        let mut expired = Budget::new(100, 0);
        assert!(!expired.spend() && expired.exhausted());
    }

    #[test]
    fn a_modification_time_is_a_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let meta = dir.path().symlink_metadata().unwrap();
        assert!(modified(&meta).millis() > 0);
    }
}
