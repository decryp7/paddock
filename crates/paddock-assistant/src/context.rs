//! How full the model's context is (ADR 0023 WP-4b: the composer's meter). The engine measures and judges; a pane only shows
//! what [`Assistant::context`](crate::Assistant::context) reports.
//!
//! After a turn of this session the count is the engine's own: the last model call's prompt plus its answer, which is what
//! the next question carries besides itself (tool definitions included, since they are part of every prompt). Before that,
//! or for a provider that reports no usage, it is an estimate of about four characters per token over the system prompt, the
//! tools offered and the history the model would be sent, and [`ContextUse::exact`] says so.

use serde::Serialize;

/// From this share of the model's context on (in percent), the chat is close to the limit.
pub const HIGH_PERCENT: u64 = 80;

/// How full the context is, in the engine's judgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum ContextLevel {
    /// The model's context length is not known: nothing to compare with.
    Unknown,
    /// Room to go on.
    Ok,
    /// At least [`HIGH_PERCENT`] of the context is used: a few more turns may not fit.
    High,
    /// The conversation fills the context: the next question cannot fit. It is never cut silently; a new chat is the way on.
    Full,
}

/// The conversation's share of the model's context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ContextUse {
    /// Tokens the next question carries before its own text.
    #[cfg_attr(test, ts(type = "number"))]
    pub used_tokens: u64,
    /// Tokens the model holds (prompt plus answer); 0 when unknown.
    #[cfg_attr(test, ts(type = "number"))]
    pub limit_tokens: u64,
    /// `used_tokens` is the engine's own count, not an estimate.
    pub exact: bool,
    /// How full that is.
    pub level: ContextLevel,
}

impl ContextUse {
    /// A use of `used` tokens out of `limit` (0: unknown), with its level.
    pub fn new(used: u64, limit: u64, exact: bool) -> ContextUse {
        ContextUse {
            used_tokens: used,
            limit_tokens: limit,
            exact,
            level: context_level(used, limit),
        }
    }
}

/// The level of `used` tokens out of `limit` (0: unknown).
pub fn context_level(used: u64, limit: u64) -> ContextLevel {
    if limit == 0 {
        ContextLevel::Unknown
    } else if used >= limit {
        ContextLevel::Full
    } else if u128::from(used) * 100 >= u128::from(limit) * u128::from(HIGH_PERCENT) {
        ContextLevel::High
    } else {
        ContextLevel::Ok
    }
}

/// About four characters per token, rounded up (the same rule as the turn's own estimate when a provider reports no usage).
pub fn estimate_tokens(chars: usize) -> u64 {
    (chars as u64).div_ceil(4)
}
