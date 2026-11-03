//! Per-account concurrency caps for a system endpoint (ADR 0027 §2): at most one running request per account for the slow
//! kinds and a cap on all requests per account; more are refused at once as busy. A [`Permit`] frees its slot when it is
//! dropped, and reports how long it was held on the injected [`Clock`] (for the per-minute summary), so tests drive time
//! with a `FakeClock`.

use crate::identity::AccountId;
use paddock_jobs::time::Clock;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// What kind of request wants a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    /// A request that may take long (SMART, login items, firewall, extras, tool install): one at a time per account.
    Slow,
    /// Any other request: counted in the account's total only.
    Light,
}

/// Why a request got no slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    /// The account already has a slow request running.
    SlowRunning,
    /// The account already has the maximum number of requests running.
    TooMany,
}

impl fmt::Display for Busy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Busy::SlowRunning => "busy: this account already has a slow request running",
            Busy::TooMany => "busy: this account already has the most requests it may run at once",
        })
    }
}

impl std::error::Error for Busy {}

#[derive(Default)]
struct Usage {
    total: usize,
    slow: usize,
}

struct Inner {
    max_total: usize,
    clock: Arc<dyn Clock>,
    usage: Mutex<HashMap<AccountId, Usage>>,
}

impl Inner {
    /// Poison-tolerant: a panic in a handler must not turn every later request into a panic.
    fn usage(&self) -> MutexGuard<'_, HashMap<AccountId, Usage>> {
        self.usage.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// The caps of one endpoint. Cheap to clone; all clones share the counts.
#[derive(Clone)]
pub struct AccountLimits(Arc<Inner>);

impl AccountLimits {
    /// ADR 0027's defaults: four requests in all per account, one of them slow.
    pub fn new(clock: Arc<dyn Clock>) -> AccountLimits {
        AccountLimits::with_max_total(4, clock)
    }

    /// At most `max_total` requests at once per account (one slow at most).
    pub fn with_max_total(max_total: usize, clock: Arc<dyn Clock>) -> AccountLimits {
        AccountLimits(Arc::new(Inner {
            max_total,
            clock,
            usage: Mutex::new(HashMap::new()),
        }))
    }

    /// A slot for `account`, or why there is none. Never waits.
    pub fn try_acquire(&self, account: &AccountId, weight: Weight) -> Result<Permit, Busy> {
        let mut usage = self.0.usage();
        let u = usage.entry(account.clone()).or_default();
        if weight == Weight::Slow && u.slow >= 1 {
            return Err(Busy::SlowRunning);
        }
        if u.total >= self.0.max_total {
            return Err(Busy::TooMany);
        }
        u.total += 1;
        if weight == Weight::Slow {
            u.slow += 1;
        }
        Ok(Permit {
            limits: self.0.clone(),
            account: account.clone(),
            weight,
            since: self.0.clock.monotonic(),
        })
    }

    /// Requests running now for `account`.
    pub fn running(&self, account: &AccountId) -> usize {
        self.0.usage().get(account).map_or(0, |u| u.total)
    }
}

/// A slot; dropping it frees the slot.
#[must_use = "dropping a permit frees its slot at once"]
pub struct Permit {
    limits: Arc<Inner>,
    account: AccountId,
    weight: Weight,
    since: Duration,
}

impl Permit {
    /// How long the slot has been held, on the limits' clock.
    pub fn held(&self) -> Duration {
        self.limits.clock.monotonic().saturating_sub(self.since)
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let mut usage = self.limits.usage();
        if let Some(u) = usage.get_mut(&self.account) {
            u.total = u.total.saturating_sub(1);
            if self.weight == Weight::Slow {
                u.slow = u.slow.saturating_sub(1);
            }
            if u.total == 0 {
                usage.remove(&self.account);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_jobs::time::FakeClock;

    fn limits() -> (AccountLimits, Arc<FakeClock>) {
        let clock = Arc::new(FakeClock::new());
        (AccountLimits::new(clock.clone()), clock)
    }

    fn uid(n: u32) -> AccountId {
        AccountId::Uid(n)
    }

    #[test]
    fn one_slow_request_per_account_and_the_slot_returns_on_drop() {
        let (l, _) = limits();
        let first = l.try_acquire(&uid(501), Weight::Slow).unwrap();
        assert_eq!(
            l.try_acquire(&uid(501), Weight::Slow).err(),
            Some(Busy::SlowRunning)
        );
        // Another account has its own slot, and light requests are not blocked by a slow one.
        let other = l.try_acquire(&uid(502), Weight::Slow).unwrap();
        let light = l.try_acquire(&uid(501), Weight::Light).unwrap();
        drop(first);
        let again = l.try_acquire(&uid(501), Weight::Slow).unwrap();
        assert_eq!(l.running(&uid(501)), 2);
        drop((other, light, again));
        assert_eq!(l.running(&uid(501)) + l.running(&uid(502)), 0);
    }

    #[test]
    fn four_requests_in_all_per_account_then_busy() {
        let (l, _) = limits();
        let held: Vec<_> = (0..4)
            .map(|_| l.try_acquire(&uid(7), Weight::Light).unwrap())
            .collect();
        assert_eq!(
            l.try_acquire(&uid(7), Weight::Light).err(),
            Some(Busy::TooMany)
        );
        assert_eq!(
            l.try_acquire(&uid(7), Weight::Slow).err(),
            Some(Busy::TooMany)
        );
        assert!(
            l.try_acquire(&uid(8), Weight::Light).is_ok(),
            "caps are per account"
        );
        drop(held);
        assert!(l.try_acquire(&uid(7), Weight::Light).is_ok());
    }

    #[test]
    fn a_permit_reports_how_long_it_was_held_on_the_injected_clock() {
        let (l, clock) = limits();
        let p = l.try_acquire(&uid(1), Weight::Light).unwrap();
        assert_eq!(p.held(), Duration::ZERO);
        clock.advance(Duration::from_secs(20));
        assert_eq!(p.held(), Duration::from_secs(20));
    }

    #[test]
    fn a_poisoned_count_is_still_usable_and_the_texts_say_why() {
        let (l, _) = limits();
        let l2 = l.clone();
        let _ = std::thread::spawn(move || {
            let _guard = l2.0.usage();
            panic!("a handler died while counting");
        })
        .join();
        assert!(l.try_acquire(&uid(1), Weight::Light).is_ok());
        assert!(Busy::SlowRunning.to_string().contains("slow request"));
        assert!(Busy::TooMany.to_string().contains("most requests"));
    }
}
