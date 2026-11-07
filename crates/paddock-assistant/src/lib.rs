//! The assistant for the Paddock apps (plan W6, ADR 0013): a chat that can use the app's tools, over Pitwall.
//!
//! - [`LlmProvider`]: the port to a model (streamed deltas, cancel flag, typed errors). [`pitwall::PitwallProvider`] is the
//!   family's adapter over `paddock-pitwall`; an app with an engine of its own registers another one (decision D5).
//! - [`protocol`]: the prompted tool protocol (Hermes `<tool_call>`, think-block and tag filters, JSON-schema mode).
//! - [`AssistantTool`] and [`Tools`]: what the assistant can look at or do. Tools that write are offered only after a user
//!   click. The app may replace its set between turns ([`Assistant::set_tools`]: connections come and go).
//! - [`ApprovalPolicy`]: which calls wait for the user's choice (Allow once, Always allow, Deny). [`DefaultPolicy`] asks
//!   for every call of a tool that writes and for nothing else; an app sets its own in [`AssistantConfig::policy`].
//! - [`run`]: the tool loop with budgets (rounds, tool calls, tokens, and optional wall limits on an injected clock), Stop
//!   and malformed calls reported to the model and never run. Calls come from the prompted protocol or from an engine's
//!   native tool calls ([`ToolCall::native`]).
//! - [`ConversationStore`]: bounded, versioned persistence through `paddock-env` (damaged: defaults and one problem; newer:
//!   left untouched).
//! - [`Assistant`]: the service. Commands return at once; a run works on its own thread and reports through [`EventSink`]
//!   (`paddock://assistant`). With the `tauri` feature, `paddock_assistant::tauri` has the commands and their table.
//!
//! ```ignore
//! let link = paddock_pitwall::Link::new(cfg);
//! let store = Arc::new(ConversationStore::open(dirs.state().join("assistant.json"), clock.clone()));
//! let tools = Tools::new(vec![Arc::new(FindEvents), Arc::new(DeleteBookmarks)])?;      // the app's tools
//! let assistant = Assistant::new(Arc::new(PitwallProvider::new(link)), tools, store, clock, AssistantConfig::new("Pacenotes", SYSTEM));
//! paddock_assistant::tauri::manage(app.handle(), assistant);
//! ```

pub mod approval;
pub mod conversation;
pub mod event;
#[cfg(test)]
mod generated;
pub mod pitwall;
pub mod protocol;
pub mod provider;
pub mod run;
pub mod service;
#[cfg(feature = "tauri")]
pub mod tauri;
pub mod tool;
pub mod types;

pub use approval::{Approval, ApprovalChoice, ApprovalPolicy, CallInfo, DefaultPolicy, Verdict};
pub use conversation::{
    ContextItem, Conversation, ConversationStore, ConversationSummary, Conversations,
    MAX_CITATIONS, MAX_CONTEXT_CHARS, MAX_CONTEXT_ITEMS, MAX_CONVERSATIONS, MAX_DATA_BYTES,
    MAX_FILE_BYTES, MAX_MESSAGE_CHARS, MAX_MESSAGES, MAX_NOTE_CHARS, MAX_TITLE_CHARS, SaveError,
    StoreLimits, StoredError, StoredMessage, ToolOutcome,
};
pub use event::{AssistantEvent, EVENT, EndReason, Envelope, ErrorKind, EventSink};
pub use pitwall::PitwallProvider;
pub use provider::{LlmProvider, ProviderError, ProviderPhase};
pub use run::{Budgets, Decision, Gate, Refused};
pub use service::{
    Assistant, AssistantConfig, AssistantError, AssistantInfo, MAX_RUNS, MAX_USER_CHARS, SendAck,
    SendRequest,
};
pub use tool::{Access, AssistantTool, ToolBudget, ToolInfo, ToolOutput, Tools, ToolsError};
pub use types::{
    Cancel, ChatMessage, ChatRequest, ChatResponse, Role, StopReason, ToolCall, ToolSpec, Usage,
};
