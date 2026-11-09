//! How full the model's context is (ADR 0023 WP-4b: the composer's meter). The engine measures and judges; a pane only shows
//! what [`Assistant::context`](crate::Assistant::context) reports.
//!
//! After a turn of this session the count is the engine's own: the last model call's prompt plus its answer, which is what
//! the next question carries besides itself (tool definitions included, since they are part of every prompt). Before that,
//! or for a provider that reports no usage, it is an estimate of about four characters per token over the system prompt, the
//! tools offered and the history the model would be sent, and [`ContextUse::exact`] says so. Attachments count through the
//! app's [`AssistantConfig::attachment_tokens`](crate::AssistantConfig::attachment_tokens) (the app keeps their bytes).
//!
//! The room for the conversation is the model's window less a reserve for the answer
//! ([`Budgets::max_answer_tokens`](crate::Budgets::max_answer_tokens)): a question that would not leave that room is refused
//! in words before it is sent ([`Assistant::send`](crate::Assistant::send)), never cut.

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
    /// Tokens the conversation may take: the model's window less [`reserve_tokens`](Self::reserve_tokens); 0 when the window is
    /// unknown.
    #[cfg_attr(test, ts(type = "number"))]
    pub limit_tokens: u64,
    /// Tokens kept free for the answer.
    #[cfg_attr(test, ts(type = "number"))]
    pub reserve_tokens: u64,
    /// `used_tokens` is the engine's own count, not an estimate.
    pub exact: bool,
    /// How full that is.
    pub level: ContextLevel,
}

impl ContextUse {
    /// A use of `used` tokens in a model `window` (0: unknown) that keeps `reserve` tokens for the answer, with its level.
    pub fn new(used: u64, window: u64, reserve: u64, exact: bool) -> ContextUse {
        let limit = if window == 0 {
            0
        } else {
            window.saturating_sub(reserve).max(1)
        };
        ContextUse {
            used_tokens: used,
            limit_tokens: limit,
            reserve_tokens: reserve,
            exact,
            level: context_level(used, limit),
        }
    }

    /// Whether `more` tokens fit besides what is used (an unknown window fits everything: the engine then says what overflowed).
    pub fn fits(&self, more: u64) -> bool {
        self.limit_tokens == 0 || self.used_tokens.saturating_add(more) <= self.limit_tokens
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
