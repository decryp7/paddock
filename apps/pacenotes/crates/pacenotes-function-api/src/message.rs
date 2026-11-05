//! The messages across the port. All plain data; all postcard-compatible (externally tagged enums only).

use pacenotes_core::ports::{ChangeSet, ModelId, SymbolRecord, TokenBudget};
use pacenotes_core::{FileId, RepoPath, Rev};
use serde::{Deserialize, Serialize};

use crate::budget::Budget;
use crate::error::FunctionError;

/// Why a model call is being made. The host refuses [`Origin::Background`] (family rule 46: no LLM for bulk or background
/// work; a model answers only a bounded, user-started request).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Origin {
    /// A person asked, and is waiting.
    UserRequest,
    /// A job running on its own (indexing, a scheduled review).
    Background,
}

/// The longest query a [`HostCall::FindSymbols`] takes.
pub const MAX_QUERY_BYTES: usize = 1024;
/// The longest prompt a [`HostCall::Complete`] takes.
pub const MAX_PROMPT_BYTES: usize = 256 * 1024;

/// What a function asks of the host. New calls are added, never changed. How much a call may return (items, bytes) and how
/// long it may take (ticks) is the request's [`Budget`], the one source of those bounds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostCall {
    /// The id of the file now at `path`.
    FileId {
        /// The path.
        path: RepoPath,
    },
    /// The symbols declared in a file (at most the budget's items).
    SymbolsInFile {
        /// The file.
        file: FileId,
    },
    /// Symbols whose name matches `query` (identifier search), best first, at most the budget's items.
    FindSymbols {
        /// The query text, at most [`MAX_QUERY_BYTES`].
        query: String,
    },
    /// A file's content at a revision, at most the budget's bytes.
    ReadAt {
        /// The revision.
        rev: Rev,
        /// The path.
        path: RepoPath,
    },
    /// The files that changed between two revisions, at most the budget's items.
    ChangedBetween {
        /// The older revision.
        from: Rev,
        /// The newer revision.
        to: Rev,
    },
    /// A bounded request to the local model (Pitwall); only for user-started work.
    Complete {
        /// The prompt, at most [`MAX_PROMPT_BYTES`].
        prompt: String,
        /// Why it is being asked; the host refuses [`Origin::Background`].
        origin: Origin,
        /// The most tokens.
        tokens: TokenBudget,
    },
}

/// Why a host request is not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RequestError {
    /// A query or prompt is longer than its bound.
    #[error("the {what} is {len} bytes; at most {max} are accepted")]
    TooLong {
        /// Which field.
        what: &'static str,
        /// Its length.
        len: usize,
        /// The bound.
        max: usize,
    },
}

/// The wire form of a [`HostRequest`].
#[derive(Serialize, Deserialize)]
pub(crate) struct HostRequestWire {
    pub(crate) id: u64,
    pub(crate) call: HostCall,
    pub(crate) budget: Budget,
}

impl TryFrom<HostRequestWire> for HostRequest {
    type Error = RequestError;
    fn try_from(w: HostRequestWire) -> Result<HostRequest, RequestError> {
        HostRequest::new(w.id, w.call, w.budget)
    }
}

/// A host call with its id and budget. In process the cancel token travels beside it; out of process a cancel frame with
/// the same id does. Validated when built and when read from the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "HostRequestWire")]
pub struct HostRequest {
    id: u64,
    call: HostCall,
    budget: Budget,
}

impl HostRequest {
    /// A request; the query and prompt bounds are checked.
    pub fn new(id: u64, call: HostCall, budget: Budget) -> Result<HostRequest, RequestError> {
        match &call {
            HostCall::FindSymbols { query } if query.len() > MAX_QUERY_BYTES => {
                return Err(RequestError::TooLong {
                    what: "query",
                    len: query.len(),
                    max: MAX_QUERY_BYTES,
                });
            }
            HostCall::Complete { prompt, .. } if prompt.len() > MAX_PROMPT_BYTES => {
                return Err(RequestError::TooLong {
                    what: "prompt",
                    len: prompt.len(),
                    max: MAX_PROMPT_BYTES,
                });
            }
            _ => {}
        }
        Ok(HostRequest { id, call, budget })
    }

    /// Correlates the reply.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The call.
    pub fn call(&self) -> &HostCall {
        &self.call
    }

    /// The most the call may use.
    pub fn budget(&self) -> &Budget {
        &self.budget
    }
}

/// The answer to a host call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostReply {
    /// The id of a file, or none.
    FileId(Option<FileId>),
    /// Symbols.
    Symbols(Vec<SymbolRecord>),
    /// File content.
    Content(Vec<u8>),
    /// Changed files.
    Changes(ChangeSet),
    /// A model's text and the tokens it used, with the model that answered.
    Completion {
        /// The text.
        text: String,
        /// Tokens used.
        tokens_used: u32,
        /// The model.
        model: ModelId,
    },
}

/// A request to run a tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionRequest {
    /// Correlates the reply.
    pub id: u64,
    /// The tool name.
    pub tool: String,
    /// The arguments as JSON text (postcard cannot carry a JSON value); the function validates them against its schema.
    pub arguments: String,
    /// The most the tool may use.
    pub budget: Budget,
}

/// A tool's result, as JSON text the host or the core renders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolOutput {
    /// The result as JSON text.
    pub json: String,
    /// True when the function stopped at its budget and the result is partial.
    pub partial: bool,
}

/// The reply to a [`FunctionRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionReply {
    /// The request's id.
    pub id: u64,
    /// The outcome.
    pub result: Result<ToolOutput, FunctionError>,
}

impl FunctionReply {
    /// A failure reply.
    pub fn failed(id: u64, error: FunctionError) -> FunctionReply {
        FunctionReply { id, result: Err(error) }
    }
}
