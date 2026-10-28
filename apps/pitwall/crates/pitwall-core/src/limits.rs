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
    /// Requests per UTC day.
    pub requests_per_day: Option<u64>,
    /// Prompt plus completion tokens per UTC calendar month.
    pub tokens_per_month: Option<u64>,
    /// The most of the machine's RAM (percent, 1 to 100) the model serving one of its requests may occupy.
    pub max_memory_percent: Option<u8>,
}

impl AppLimits {
    /// Checks the values a user typed (the engine re-validates what the UI sent).
    pub fn validate(&self) -> Result<(), String> {
        let zero = |name: &str, v: Option<u64>| match v {
            Some(0) => Err(format!("{name} must be at least 1; leave it empty for no limit")),
            _ => Ok(()),
        };
        zero("requests per day", self.requests_per_day)?;
        zero("tokens per month", self.tokens_per_month)?;
        match self.max_memory_percent {
            Some(p) if !(1..=100).contains(&p) => Err("the memory share must be between 1 and 100 percent".into()),
            _ => Ok(()),
        }
    }
}

/// What an app has used so far in the current UTC day and month (from [`crate::usage::Usage::budget_use`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BudgetUse {
    /// Tokens today.
    pub tokens_today: u64,
    /// Requests today.
    pub requests_today: u64,
    /// Tokens this month.
    pub tokens_month: u64,
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
    /// Its identical deterministic requests may be answered from the response cache (ADR 0019; off until the user
    /// turns it on for the app).
    pub cache_responses: bool,
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
            cache_responses: false,
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
    /// Daily request budget used up.
    #[error("this app has made its {0} requests for today; the budget resets at midnight UTC")]
    RequestsDay(u64),
    /// Monthly token budget used up.
    #[error("this app has used its {0} tokens for this month; the budget resets on the 1st at midnight UTC")]
    TokensMonth(u64),
    /// The model needs more memory than the app's share.
    #[error(
        "the model needs {need} but this app may use at most {percent}% of the machine's memory ({allowed}); use a smaller model or raise the share in Pitwall ▸ Apps & keys",
        need = paddock_units::format_bytes(*need_mib << 20, paddock_units::Kind::Memory),
        allowed = paddock_units::format_bytes(*allowed_mib << 20, paddock_units::Kind::Memory)
    )]
    Memory {
        /// The share (percent).
        percent: u8,
        /// What the model occupies.
        need_mib: u64,
        /// What the share allows.
        allowed_mib: u64,
    },
}

impl Denied {
    /// Whether this is a rate/budget refusal (HTTP 429) rather than a permission one (403).
    pub fn is_rate(&self) -> bool {
        matches!(
            self,
            Denied::Rate(_) | Denied::Tokens(_) | Denied::RequestsDay(_) | Denied::TokensMonth(_)
        )
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

    /// Requests in the minute before `now` (the window is only trimmed when a request arrives).
    pub fn count_since(&self, now: Millis) -> usize {
        self.times.iter().filter(|t| now.saturating_sub(**t) < MINUTE).count()
    }
}

/// Checks the daily token budget against today's use so far.
pub fn check_tokens(used_today: u64, per_day: Option<u64>) -> Result<(), Denied> {
    match per_day {
        Some(limit) if used_today >= limit => Err(Denied::Tokens(limit)),
        _ => Ok(()),
    }
}

/// Checks every budget (daily tokens, monthly tokens, daily requests) against the use so far.
pub fn check_budget(used: &BudgetUse, limits: &AppLimits) -> Result<(), Denied> {
    check_tokens(used.tokens_today, limits.tokens_per_day)?;
    match limits.tokens_per_month {
        Some(limit) if used.tokens_month >= limit => return Err(Denied::TokensMonth(limit)),
        _ => {}
    }
    match limits.requests_per_day {
        Some(limit) if used.requests_today >= limit => Err(Denied::RequestsDay(limit)),
        _ => Ok(()),
    }
}

/// Refuses a model whose footprint exceeds the app's share of the machine's memory.
pub fn check_memory(model_bytes: u64, machine_bytes: u64, percent: Option<u8>) -> Result<(), Denied> {
    const MIB: u64 = 1 << 20;
    match percent {
        Some(p) => {
            let allowed = allowed_bytes(machine_bytes, p);
            if model_bytes > allowed {
                Err(Denied::Memory {
                    percent: p,
                    need_mib: model_bytes.div_ceil(MIB),
                    allowed_mib: allowed / MIB,
                })
            } else {
                Ok(())
            }
        }
        None => Ok(()),
    }
}

fn allowed_bytes(machine_bytes: u64, percent: u8) -> u64 {
    (u128::from(machine_bytes) * u128::from(percent.min(100)) / 100) as u64
}

/// Which limit a status describes (`LimitKind` in `api.types.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LimitKind {
    /// Requests in the last minute.
    RequestsPerMin,
    /// Requests today.
    RequestsPerDay,
    /// Tokens today.
    TokensPerDay,
    /// Tokens this month.
    TokensPerMonth,
    /// Peak memory today against the share.
    MemoryShare,
}

/// Where a limit stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LimitState {
    /// Below 80%.
    Ok,
    /// 80% or more: the app is close to being refused.
    Warning,
    /// 100% or more: requests are refused (or, for the memory share, the model is too large).
    Exceeded,
}

/// The percentage at which a limit starts to warn.
pub const WARN_PERCENT: u64 = 80;

/// One configured limit and how much of it is used (`LimitStatus` in `api.types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitStatus {
    /// Which limit.
    pub kind: LimitKind,
    /// Used so far (requests, tokens or bytes).
    pub used: u64,
    /// The limit in the same unit.
    pub limit: u64,
    /// Used as a percentage of the limit, capped at 999.
    pub percent: u32,
    /// Where it stands.
    pub state: LimitState,
    /// An explanation for a warning or an exceeded limit.
    pub message: Option<String>,
}

impl LimitKind {
    /// The start of the period the limit counts in (it resets there), `None` for the sliding minute.
    pub fn period_start(self, now: Millis) -> Option<Millis> {
        match self {
            LimitKind::RequestsPerMin => None,
            LimitKind::RequestsPerDay | LimitKind::TokensPerDay | LimitKind::MemoryShare => Some(crate::time::day_start(now)),
            LimitKind::TokensPerMonth => Some(crate::time::month_start(now)),
        }
    }

    fn what(self) -> &'static str {
        match self {
            LimitKind::RequestsPerMin => "requests per minute",
            LimitKind::RequestsPerDay => "requests today",
            LimitKind::TokensPerDay => "tokens today",
            LimitKind::TokensPerMonth => "tokens this month",
            LimitKind::MemoryShare => "memory share",
        }
    }
}

fn status(kind: LimitKind, used: u64, limit: u64) -> LimitStatus {
    let percent = if limit == 0 {
        999
    } else {
        (u128::from(used) * 100 / u128::from(limit)).min(999) as u32
    };
    let state = if u64::from(percent) >= 100 {
        LimitState::Exceeded
    } else if u64::from(percent) >= WARN_PERCENT {
        LimitState::Warning
    } else {
        LimitState::Ok
    };
    let message = match (state, kind) {
        (LimitState::Ok, _) => None,
        (LimitState::Warning, _) => Some(format!(
            "{percent}% of the {} limit ({limit}) is used; requests are refused at 100%.",
            kind.what()
        )),
        (LimitState::Exceeded, LimitKind::MemoryShare) => Some(
            "A model this app used was larger than its memory share; requests for it are refused. Use a smaller model or raise the share."
                .into(),
        ),
        (LimitState::Exceeded, _) => Some(format!(
            "The {} limit ({limit}) is reached; requests are refused until it resets.",
            kind.what()
        )),
    };
    LimitStatus {
        kind,
        used,
        limit,
        percent,
        state,
        message,
    }
}

/// What an app has used against each limit that is set.
pub struct StatusInput {
    /// Today and this month.
    pub used: BudgetUse,
    /// Requests in the last minute.
    pub requests_last_minute: u64,
    /// The largest footprint among the app's requests today (bytes).
    pub peak_memory_today: u64,
    /// The machine's RAM (bytes).
    pub machine_bytes: u64,
}

/// The status of every configured limit, for the app detail view and the 80% warnings.
pub fn statuses(limits: &AppLimits, i: &StatusInput) -> Vec<LimitStatus> {
    let mut v = vec![];
    if let Some(l) = limits.requests_per_min {
        v.push(status(LimitKind::RequestsPerMin, i.requests_last_minute, u64::from(l)));
    }
    if let Some(l) = limits.requests_per_day {
        v.push(status(LimitKind::RequestsPerDay, i.used.requests_today, l));
    }
    if let Some(l) = limits.tokens_per_day {
        v.push(status(LimitKind::TokensPerDay, i.used.tokens_today, l));
    }
    if let Some(l) = limits.tokens_per_month {
        v.push(status(LimitKind::TokensPerMonth, i.used.tokens_month, l));
    }
    if let Some(p) = limits.max_memory_percent {
        // Exceeded only when a request's model was strictly larger than the share; equal is allowed.
        let allowed = allowed_bytes(i.machine_bytes, p);
        let mut st = status(LimitKind::MemoryShare, i.peak_memory_today, allowed);
        if i.peak_memory_today <= allowed && st.state == LimitState::Exceeded {
            st.state = LimitState::Warning;
            st.message = Some("A model used today fills the whole memory share.".into());
        }
        v.push(st);
    }
    v
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
            cache_responses: false,
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
        assert_eq!(w.count_since(MINUTE + 9), 2);
        assert_eq!(w.count_since(MINUTE + 10), 1, "the one at 10 ms ages out");
        assert_eq!(w.count_since(5 * MINUTE), 0);
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

    #[test]
    fn budgets_at_the_boundary() {
        let l = AppLimits {
            tokens_per_day: Some(100),
            tokens_per_month: Some(1000),
            requests_per_day: Some(10),
            ..AppLimits::default()
        };
        let u = |t, r, m| BudgetUse {
            tokens_today: t,
            requests_today: r,
            tokens_month: m,
        };
        assert!(check_budget(&u(99, 9, 999), &l).is_ok());
        assert_eq!(check_budget(&u(100, 0, 0), &l), Err(Denied::Tokens(100)));
        assert_eq!(check_budget(&u(0, 0, 1000), &l), Err(Denied::TokensMonth(1000)));
        assert_eq!(check_budget(&u(0, 10, 0), &l), Err(Denied::RequestsDay(10)));
        assert!(check_budget(&u(u64::MAX, u64::MAX, u64::MAX), &AppLimits::default()).is_ok());
        for d in [Denied::RequestsDay(1), Denied::TokensMonth(1)] {
            assert!(d.is_rate() && d.to_string().contains("resets"));
        }
    }

    #[test]
    fn memory_share() {
        let gib = 1u64 << 30;
        assert!(check_memory(4 * gib, 16 * gib, None).is_ok());
        assert!(check_memory(4 * gib, 16 * gib, Some(25)).is_ok(), "exactly the share is allowed");
        let e = check_memory(4 * gib + 1, 16 * gib, Some(25)).unwrap_err();
        assert!(!e.is_rate());
        let t = e.to_string();
        assert!(t.contains("25%") && t.contains("needs 4 GB") && t.contains("(4 GB)"), "{t}");
        assert!(check_memory(1, 0, Some(100)).is_err());
    }

    #[test]
    fn validation() {
        assert!(AppLimits::default().validate().is_ok());
        let bad = |l: AppLimits| l.validate().unwrap_err();
        assert!(bad(AppLimits {
            requests_per_day: Some(0),
            ..Default::default()
        })
        .contains("requests per day"));
        assert!(bad(AppLimits {
            tokens_per_month: Some(0),
            ..Default::default()
        })
        .contains("tokens per month"));
        assert!(bad(AppLimits {
            max_memory_percent: Some(0),
            ..Default::default()
        })
        .contains("memory share"));
        assert!(bad(AppLimits {
            max_memory_percent: Some(101),
            ..Default::default()
        })
        .contains("memory share"));
        assert!(AppLimits {
            max_memory_percent: Some(100),
            requests_per_day: Some(1),
            ..Default::default()
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn statuses_warn_at_80_percent() {
        let l = AppLimits {
            requests_per_min: Some(10),
            tokens_per_day: Some(100),
            requests_per_day: Some(50),
            tokens_per_month: Some(1000),
            max_memory_percent: Some(50),
            ..Default::default()
        };
        let i = StatusInput {
            used: BudgetUse {
                tokens_today: 79,
                requests_today: 50,
                tokens_month: 800,
            },
            requests_last_minute: 3,
            peak_memory_today: 100,
            machine_bytes: 200,
        };
        let s = statuses(&l, &i);
        let by = |k| s.iter().find(|x| x.kind == k).unwrap();
        assert_eq!(by(LimitKind::RequestsPerMin).state, LimitState::Ok);
        assert!(by(LimitKind::RequestsPerMin).message.is_none());
        assert_eq!(
            (by(LimitKind::TokensPerDay).percent, by(LimitKind::TokensPerDay).state),
            (79, LimitState::Ok)
        );
        assert_eq!(by(LimitKind::TokensPerMonth).state, LimitState::Warning);
        assert!(by(LimitKind::TokensPerMonth).message.as_ref().unwrap().contains("80%"));
        assert_eq!(by(LimitKind::RequestsPerDay).state, LimitState::Exceeded);
        assert!(by(LimitKind::RequestsPerDay)
            .message
            .as_ref()
            .unwrap()
            .contains("refused until it resets"));
        let mem = by(LimitKind::MemoryShare);
        assert_eq!(
            (mem.state, mem.percent),
            (LimitState::Warning, 100),
            "filling the share exactly is allowed"
        );
        let over = statuses(
            &l,
            &StatusInput {
                peak_memory_today: 101,
                ..i
            },
        );
        let mem = over.iter().find(|x| x.kind == LimitKind::MemoryShare).unwrap();
        assert_eq!(mem.state, LimitState::Exceeded);
        assert!(mem.message.as_ref().unwrap().contains("memory share"));
        assert!(statuses(&AppLimits::default(), &i).is_empty());
        assert_eq!(status(LimitKind::TokensPerDay, 5, 0).percent, 999);
        assert_eq!(status(LimitKind::TokensPerDay, u64::MAX, 1).percent, 999);
    }

    #[test]
    fn periods() {
        let t = 40 * crate::time::DAY + 5;
        assert_eq!(LimitKind::RequestsPerMin.period_start(t), None);
        assert_eq!(LimitKind::TokensPerDay.period_start(t), Some(40 * crate::time::DAY));
        assert_eq!(LimitKind::MemoryShare.period_start(t), Some(40 * crate::time::DAY));
        assert_eq!(LimitKind::TokensPerMonth.period_start(t), Some(31 * crate::time::DAY));
        assert_eq!(LimitKind::RequestsPerDay.period_start(t), Some(40 * crate::time::DAY));
    }
}
