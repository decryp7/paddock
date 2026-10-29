//! The messages across the port. All plain data; all postcard-compatible (externally tagged enums only).

use pacenotes_core::ports::{ChangeSet, ModelId, SymbolRecord};
use pacenotes_core::{FileId, RepoPath, Rev};
use serde::{Deserialize, Serialize};

use crate::budget::Budget;
use crate::error::FunctionError;

/// What a function asks of the host. New calls are added, never changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostCall {
    /// The id of the file now at `path`.
    FileId {
        /// The path.
        path: RepoPath,
    },
    /// The symbols declared in a file.
    SymbolsInFile {
        /// The file.
        file: FileId,
    },
    /// Symbols whose name matches `query` (identifier search), best first.
    FindSymbols {
        /// The query text.
        query: String,
        /// The most symbols to return.
        limit: u32,
    },
    /// A file's content at a revision.
    ReadAt {
        /// The revision.
        rev: Rev,
        /// The path.
        path: RepoPath,
        /// The most bytes to read.
        max_bytes: u64,
    },
    /// The files that changed between two revisions.
    ChangedBetween {
        /// The older revision.
        from: Rev,
        /// The newer revision.
        to: Rev,
        /// The most files to return.
        limit: u32,
    },
    /// A bounded request to the local model (Pitwall); only for user-started work.
    Complete {
        /// The prompt.
        prompt: String,
        /// The most tokens.
        max_tokens: u32,
    },
}

/// A host call with its id and budget. In process the cancel token travels beside it; out of process a cancel frame with
/// the same id does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostRequest {
    /// Correlates the reply.
    pub id: u64,
    /// The call.
    pub call: HostCall,
    /// The most the call may use.
    pub budget: Budget,
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
