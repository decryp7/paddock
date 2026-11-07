//! What a running turn tells the windows: the `paddock://assistant` event (deltas, tool calls and results, approvals,
//! done, error) and the sink that carries it.

use crate::approval::Approval;
use crate::conversation::ToolOutcome;
use crate::types::Usage;
use paddock_pitwall::AiState;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The event every window hears; the payload is an [`Envelope`].
pub const EVENT: &str = "paddock://assistant";

/// Why a turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum EndReason {
    /// The model answered.
    Answered,
    /// The user pressed Stop.
    Cancelled,
    /// The provider failed; what was generated until then is kept.
    Failed,
    /// The turn's tool-call budget was used up.
    BudgetToolCalls,
    /// The turn's round budget was used up.
    BudgetRounds,
    /// The turn's token budget was used up.
    BudgetTokens,
    /// The turn's time budget was used up.
    BudgetTime,
}

/// What kind of failure an [`AssistantEvent::Error`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// The engine is not usable (Pitwall missing...): the UI shows the shared needs-AI state.
    NeedsAi,
    /// A failure of this request.
    Failed,
}

/// One thing a turn did.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AssistantEvent {
    /// The turn started (the user's message is saved).
    Started {
        /// The conversation's title.
        title: String,
    },
    /// A piece of the answer, as text (never HTML).
    Delta {
        /// The text.
        text: String,
    },
    /// The model called a tool.
    ToolStarted {
        /// Answers `ToolFinished` and `ApprovalNeeded`.
        call_id: String,
        /// The tool's name (empty for a call that could not be read).
        name: String,
        /// The call as the user reads it.
        summary: String,
        /// The tool changes things.
        write: bool,
        /// What it changes cannot be undone (implies `write`).
        destructive: bool,
    },
    /// A call waits for the user's choice (`assistant_approve_tool`): the approval policy asked for it (by default, every
    /// call of a tool that writes).
    ApprovalNeeded {
        /// The call.
        call_id: String,
        /// The tool.
        name: String,
        /// What it would do.
        summary: String,
        /// What it would change cannot be undone: the UI makes Decline the default and Allow a danger button.
        destructive: bool,
        /// The card offers "Always allow" besides Allow once and Deny (an action asks every call, so it never does).
        remember: bool,
        /// The arguments.
        #[cfg_attr(test, ts(type = "unknown"))]
        input: Value,
    },
    /// A tool call ended.
    ToolFinished {
        /// The call.
        call_id: String,
        /// The tool.
        name: String,
        /// It did not fail.
        ok: bool,
        /// One line for the card.
        summary: String,
        /// The structured result for the app's renderer of this tool.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional, type = "unknown"))]
        data: Option<Value>,
        /// Set when the user's choice ended the call: they declined it, or stopped the turn while it waited.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        outcome: Option<ToolOutcome>,
        /// How the call was approved, when it ran (none for a call that was refused, declined or stopped).
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        approval: Option<Approval>,
    },
    /// Something the user should know that is not an error (a call that could not be read, a conversation that could not
    /// be saved).
    Notice {
        /// The sentence.
        message: String,
    },
    /// The turn ended.
    Done {
        /// The final answer (also what was generated until a stop or a budget).
        text: String,
        /// Tokens used in the turn.
        usage: Usage,
        /// Why it ended.
        end: EndReason,
        /// For the budgets: what happened and what to do.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        message: Option<String>,
    },
    /// The turn failed.
    Error {
        /// The kind.
        kind: ErrorKind,
        /// One sentence for the user.
        message: String,
        /// For `needsAi`: the state (the shared component renders it).
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional, type = "AiState"))]
        needs_ai: Option<AiState>,
    },
}

/// An event with its place: the conversation and the turn's sequence number (a window that reconnects can tell what it
/// missed).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    /// The conversation.
    pub conversation_id: String,
    /// 1, 2, ... within the turn.
    #[cfg_attr(test, ts(type = "number"))]
    pub seq: u64,
    /// What happened.
    #[serde(flatten)]
    pub event: AssistantEvent,
}

/// Where events go: the Tauri adapter sends them to every window; tests collect them.
pub trait EventSink: Send + Sync {
    /// Delivers one event; must not block.
    fn emit(&self, envelope: &Envelope);
}
