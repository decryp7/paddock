//! The port the assistant talks to. The Pitwall adapter ([`crate::pitwall`]) is the family's provider; an app with an engine
//! of its own (Debrief's embedded llama.cpp) registers another implementation behind the same trait (decision D5). Tests use
//! scripted fakes. Nothing here knows about networks.

use crate::types::{ChatRequest, ChatResponse};
use paddock_pitwall::AiState;

/// Why a provider could not answer.
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderError {
    /// The request's cancel flag was set before any answer existed.
    Cancelled,
    /// The prompt does not fit the model's context; it was not truncated silently.
    ContextOverflow,
    /// The engine is not usable now (Pitwall missing, not running, not paired, no suitable model...). The UI shows the
    /// shared needs-AI state for it instead of an error.
    Unavailable(AiState),
    /// The engine failed; the text says what and what to do.
    Failed(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderError::Cancelled => f.write_str("Stopped."),
            ProviderError::ContextOverflow => {
                f.write_str("The conversation is too long for the model. Start a new conversation.")
            }
            ProviderError::Unavailable(state) => f.write_str(&state.message("the app")),
            ProviderError::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ProviderError {}

/// Contract: `chat` streams generated text through `on_delta`, honours `req.cancel` between tokens (returning
/// [`StopReason::Cancelled`](crate::StopReason::Cancelled) with the partial text, or [`ProviderError::Cancelled`] when there
/// is none), never panics on bad input, and returns [`ProviderError::ContextOverflow`] rather than truncating the prompt
/// silently. It blocks: the assistant calls it on a worker thread (rule 10).
pub trait LlmProvider: Send + Sync {
    /// A stable id (`pitwall`).
    fn id(&self) -> &str;
    /// What the user sees ("Pitwall (qwen3-8b)").
    fn display_name(&self) -> String;
    /// Tokens the model can hold (prompt plus answer); 0 when unknown.
    fn context_length(&self) -> usize;
    /// Streams one answer.
    fn chat(
        &self,
        req: &ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<ChatResponse, ProviderError>;
}
