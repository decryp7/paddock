//! Per-app permissions and limits (ADR 0007): which models an app may use, how urgent its jobs may be, whether it
//! may use tools and the extended APIs, and its request and token budgets. Pure: usage and time come in as values.

use crate::ids::ModelId;
use crate::scheduler::Priority;
use crate::time::{Millis, MINUTE};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// An app's budgets.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppLimits {
    /// Requests per minute (`None`: no limit).
    pub requests_per_min: Option<u32>,
    /// Prompt plus completion tokens per UTC day.
    pub tokens_per_day: Option<u64>,
    /// Requests running at once.
    pub max_concurrent: Option<u32>,
}

/// What an app may do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppPermissions {
    /// The models it may use (`None`: all).
    pub models: Option<Vec<ModelId>>,
    /// The most urgent priority it may ask for.
    pub max_priority: Priority,
    /// It may use Pitwall's hosted tools (MCP).
    pub tools: bool,
    /// It may use `/x/…`.
    pub extended_apis: bool,
    /// Its budgets.
    pub limits: AppLimits,
}

impl Default for AppPermissions {
    fn default() -> Self {
        AppPermissions {
            models: None,
            max_priority: Priority::Interactive,
            tools: false,
            extended_apis: true,
            limits: AppLimits::default(),
        }
    }
}

/// Why a request is refused (403 or 429).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Denied {
    /// The model is not allowed for this app.
    #[error("this app may not use the model {0}; allow it in Pitwall ▸ Apps & keys")]
    Model(ModelId),
    /// Tools are not allowed.
    #[error("this app may not use Pitwall's tools; allow them in Pitwall ▸ Apps & keys")]
    Tools,
    /// The extended APIs are not allowed.
    #[error("this app may not use Pitwall's extended APIs (/x/); allow them in Pitwall ▸ Apps & keys")]
    Extended,
    /// Too many requests this minute.
    #[error("rate limit: at most {0} requests per minute for this app; retry in a moment")]
    Rate(u32),
    /// Daily token budget used up.
    #[error("this app has used its {0} tokens for today; the budget resets at midnight UTC")]
    Tokens(u64),
}

impl Denied {
    /// Whether this is a rate/budget refusal (HTTP 429) rather than a permission one (403).
    pub fn is_rate(&self) -> bool {
        matches!(self, Denied::Rate(_) | Denied::Tokens(_))
    }
}

impl AppPermissions {
    /// Checks a request's model and features; returns the priority it runs at.
    pub fn authorize(&self, model: Option<&ModelId>, wanted: Priority, tools: bool, extended: bool) -> Result<Priority, Denied> {
        if let (Some(allowed), Some(m)) = (&self.models, model) {
            if !allowed.contains(m) {
                return Err(Denied::Model(m.clone()));
            }
        }
        if tools && !self.tools {
            return Err(Denied::Tools);
        }
        if extended && !self.extended_apis {
            return Err(Denied::Extended);
        }
        Ok(wanted.capped(self.max_priority))
    }
}

/// A sliding one-minute window of request times.
#[derive(Debug, Clone, Default)]
pub struct RateWindow {
    times: VecDeque<Millis>,
}

impl RateWindow {
    /// Records a request at `now` if the limit allows it.
    pub fn admit(&mut self, now: Millis, per_min: Option<u32>) -> Result<(), Denied> {
        while self.times.front().is_some_and(|t| now.saturating_sub(*t) >= MINUTE) {
            self.times.pop_front();
        }
        if let Some(limit) = per_min {
            if self.times.len() >= limit as usize {
                return Err(Denied::Rate(limit));
            }
        }
        // Bounded even without a limit: only the last minute is kept, capped at 100k entries.
        if self.times.len() >= 100_000 {
            self.times.pop_front();
        }
        self.times.push_back(now);
        Ok(())
    }

    /// Requests in the last minute.
    pub fn count(&self) -> usize {
        self.times.len()
    }
}

/// Checks the daily token budget against today's use so far.
pub fn check_tokens(used_today: u64, per_day: Option<u64>) -> Result<(), Denied> {
    match per_day {
        Some(limit) if used_today >= limit => Err(Denied::Tokens(limit)),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(s: &str) -> ModelId {
        ModelId::parse(s).unwrap()
    }

    #[test]
    fn permissions() {
        let all = AppPermissions::default();
        assert_eq!(
            all.authorize(Some(&m("x")), Priority::Interactive, false, true),
            Ok(Priority::Interactive)
        );
        let p = AppPermissions {
            models: Some(vec![m("a")]),
            max_priority: Priority::Background,
            tools: false,
            extended_apis: false,
            limits: AppLimits::default(),
        };
        assert_eq!(
            p.authorize(Some(&m("a")), Priority::Interactive, false, false),
            Ok(Priority::Background)
        );
        assert_eq!(p.authorize(None, Priority::Batch, false, false), Ok(Priority::Batch));
        assert_eq!(
            p.authorize(Some(&m("b")), Priority::Batch, false, false),
            Err(Denied::Model(m("b")))
        );
        assert_eq!(p.authorize(None, Priority::Batch, true, false), Err(Denied::Tools));
        assert_eq!(p.authorize(None, Priority::Batch, false, true), Err(Denied::Extended));
        for d in [Denied::Model(m("b")), Denied::Tools, Denied::Extended] {
            assert!(!d.is_rate());
            assert!(d.to_string().contains("Apps & keys"));
        }
        let json: AppPermissions = serde_json::from_str(r#"{"maxPriority":"batch","limits":{"requestsPerMin":5}}"#).unwrap();
        assert_eq!(json.max_priority, Priority::Batch);
        assert_eq!(json.limits.requests_per_min, Some(5));
        assert!(json.extended_apis);
    }

    #[test]
    fn rate_window() {
        let mut w = RateWindow::default();
        assert!(w.admit(0, Some(2)).is_ok());
        assert!(w.admit(10, Some(2)).is_ok());
        let e = w.admit(20, Some(2)).unwrap_err();
        assert!(e.is_rate() && e.to_string().contains("2 requests per minute"));
        assert!(w.admit(MINUTE, Some(2)).is_ok(), "the first one aged out");
        assert_eq!(w.count(), 2);
        let mut u = RateWindow::default();
        for i in 0..100_001 {
            u.admit(i / 1000, None).unwrap();
        }
        assert_eq!(u.count(), 100_000);
    }

    #[test]
    fn token_budget() {
        assert!(check_tokens(10, None).is_ok());
        assert!(check_tokens(9, Some(10)).is_ok());
        let e = check_tokens(10, Some(10)).unwrap_err();
        assert!(e.is_rate() && e.to_string().contains("midnight"));
    }
}
