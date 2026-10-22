//! Per-app usage (ADR 0007): requests, tokens, busy time and errors per UTC day, for the dashboard and the daily
//! token budget. Bounded: days older than the retention are dropped, and at most [`MAX_APPS`] apps per day.

use crate::time::{day_start, Millis, DAY};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Counters of one app on one day (`UsageCounters` in `api.types.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Counters {
    /// Requests.
    pub requests: u64,
    /// Prompt tokens.
    pub prompt_tokens: u64,
    /// Generated tokens.
    pub completion_tokens: u64,
    /// Time spent serving.
    pub busy_ms: u64,
    /// Failed requests.
    pub errors: u64,
}

impl Counters {
    /// Tokens both ways.
    pub fn tokens(&self) -> u64 {
        self.prompt_tokens + self.completion_tokens
    }
}

/// Usage of every app per day.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    days: BTreeMap<Millis, BTreeMap<String, Counters>>,
}

/// Most apps tracked per day.
pub const MAX_APPS: usize = 256;

impl Usage {
    /// Records one request.
    pub fn record(&mut self, at: Millis, app: &str, prompt: u64, completion: u64, busy_ms: u64, failed: bool) {
        let day = self.days.entry(day_start(at)).or_default();
        if !day.contains_key(app) && day.len() >= MAX_APPS {
            return;
        }
        let c = day.entry(app.to_string()).or_default();
        c.requests += 1;
        c.prompt_tokens += prompt;
        c.completion_tokens += completion;
        c.busy_ms += busy_ms;
        c.errors += u64::from(failed);
    }

    /// An app's counters on the day of `at`.
    pub fn today(&self, at: Millis, app: &str) -> Counters {
        self.days.get(&day_start(at)).and_then(|d| d.get(app)).copied().unwrap_or_default()
    }

    /// Drops days older than `keep_days`.
    pub fn prune(&mut self, now: Millis, keep_days: u32) {
        let cutoff = day_start(now).saturating_sub(u64::from(keep_days) * DAY);
        self.days.retain(|d, _| *d >= cutoff);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_per_day_and_prunes() {
        let mut u = Usage::default();
        u.record(10, "imt", 5, 7, 100, false);
        u.record(20, "imt", 1, 1, 50, true);
        u.record(DAY + 1, "imt", 9, 9, 1, false);
        let c = u.today(30, "imt");
        assert_eq!((c.requests, c.tokens(), c.busy_ms, c.errors), (2, 14, 150, 1));
        assert_eq!(u.today(30, "other"), Counters::default());
        assert_eq!(u.today(DAY + 5, "imt").requests, 1);
        u.prune(3 * DAY, 1);
        assert_eq!(u.today(30, "imt").requests, 0);
        for i in 0..MAX_APPS + 3 {
            u.record(5 * DAY, &format!("a{i}"), 0, 0, 0, false);
        }
        assert_eq!(u.today(5 * DAY, &format!("a{}", MAX_APPS + 1)).requests, 0);
    }
}
