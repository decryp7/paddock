//! Cancellable operations by the caller's `opId` (a model load or a benchmark). `cancel_op` wakes the operation; a cancel
//! that arrives before its operation registered is kept as a permit for a short time, then forgotten, so a stale cancel
//! can never hit a later operation that happens to reuse the id (ER-18). Pure over the time passed in.

use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Notify;

/// Most entries kept; running operations are never evicted, orphan permits are, oldest first.
pub const MAX_OPS: usize = 256;
/// How long a cancel waits for an operation that has not registered yet.
pub const ORPHAN_TTL_MS: u64 = 30_000;

struct Entry {
    wake: Arc<Notify>,
    /// `Some(deadline)` for a cancel that found no running operation; `None` while an operation runs.
    orphan_until: Option<u64>,
}

/// The table of operations by id.
#[derive(Default)]
pub struct Ops(BTreeMap<String, Entry>);

impl Ops {
    /// An operation `op` starts at `now`. Returns what to wait on; an unexpired cancel that came first is already stored
    /// in it as a permit.
    pub fn begin(&mut self, op: &str, now: u64) -> Arc<Notify> {
        if let Some(e) = self.0.get_mut(op) {
            if e.orphan_until.is_some_and(|until| until > now) {
                e.orphan_until = None;
                return e.wake.clone();
            }
            if e.orphan_until.is_none() {
                // Same id running twice: share the wake-up; the caller's cancel stops both.
                return e.wake.clone();
            }
        }
        let wake = Arc::new(Notify::new());
        self.0.insert(
            op.to_string(),
            Entry {
                wake: wake.clone(),
                orphan_until: None,
            },
        );
        wake
    }

    /// The operation ended.
    pub fn end(&mut self, op: &str) {
        self.0.remove(op);
    }

    /// Cancels `op`: wakes it when it runs, otherwise keeps a permit until `now + ORPHAN_TTL_MS`.
    pub fn cancel(&mut self, op: &str, now: u64) {
        if let Some(e) = self.0.get(op) {
            e.wake.notify_one();
            return;
        }
        self.0.retain(|_, e| e.orphan_until.is_none_or(|until| until > now));
        while self.0.len() >= MAX_OPS {
            let oldest = self.0.iter().filter_map(|(k, e)| e.orphan_until.map(|u| (u, k.clone()))).min();
            match oldest {
                Some((_, k)) => {
                    self.0.remove(&k);
                }
                // Only running operations fill the table: nothing to evict, and a permit for one more is not kept.
                None => return,
            }
        }
        let wake = Arc::new(Notify::new());
        wake.notify_one();
        self.0.insert(
            op.to_string(),
            Entry {
                wake,
                orphan_until: Some(now + ORPHAN_TTL_MS),
            },
        );
    }

    /// Entries held (running operations and unexpired permits).
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether nothing is held.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn woken(n: &Notify) -> bool {
        tokio::time::timeout(std::time::Duration::from_millis(0), n.notified())
            .await
            .is_ok()
    }

    #[tokio::test]
    async fn a_cancel_before_the_operation_stops_it_as_it_starts() {
        let mut ops = Ops::default();
        ops.cancel("a", 1_000);
        let wake = ops.begin("a", 1_001);
        assert!(woken(&wake).await);
    }

    #[tokio::test]
    async fn a_cancel_for_an_operation_that_ended_expires_and_does_not_hit_a_later_one_with_the_same_id() {
        let mut ops = Ops::default();
        // The work finished, then the cancel arrived: a permit is stored...
        ops.begin("load_model-1", 0);
        ops.end("load_model-1");
        ops.cancel("load_model-1", 10);
        // ...and a later operation with the same id after the TTL is not cancelled by it.
        let wake = ops.begin("load_model-1", 10 + ORPHAN_TTL_MS);
        assert!(!woken(&wake).await, "an expired permit is gone");
        // Within the TTL the permit still applies (the cancel was early, not stale).
        let mut ops = Ops::default();
        ops.cancel("x", 10);
        assert!(woken(&ops.begin("x", 10 + ORPHAN_TTL_MS - 1)).await);
    }

    #[tokio::test]
    async fn a_running_operation_is_woken_and_its_entry_ends_with_it() {
        let mut ops = Ops::default();
        let wake = ops.begin("a", 0);
        ops.cancel("a", 5);
        assert!(woken(&wake).await);
        ops.end("a");
        assert!(ops.is_empty());
    }

    #[test]
    fn orphan_permits_are_bounded_oldest_first_and_running_operations_are_never_evicted() {
        let mut ops = Ops::default();
        let _running: Vec<_> = (0..10).map(|n| ops.begin(&format!("run{n}"), 0)).collect();
        for n in 0..1_000u64 {
            ops.cancel(&format!("x{n}"), n);
            assert!(ops.len() <= MAX_OPS);
        }
        for n in 0..10 {
            ops.end(&format!("run{n}"));
        }
        assert!(ops.len() <= MAX_OPS);
        // The newest permits are kept, the oldest are gone.
        assert!(ops.0.contains_key("x999"));
        assert!(!ops.0.contains_key("x0"));
    }

    #[test]
    fn a_full_table_of_running_operations_keeps_all_of_them() {
        let mut ops = Ops::default();
        let _all: Vec<_> = (0..MAX_OPS).map(|n| ops.begin(&format!("run{n}"), 0)).collect();
        ops.cancel("late", 1);
        assert_eq!(ops.len(), MAX_OPS, "no running operation was dropped");
    }
}
