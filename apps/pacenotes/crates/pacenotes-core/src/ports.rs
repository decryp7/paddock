//! The ports the core owns (ADR 0002). Adapters (gix, SQLite, Model2Vec, a language-server process, Pitwall) implement
//! them elsewhere; the core and the function crates depend on these traits only.
//!
//! Every method that can block takes a [`CancelToken`] and returns [`Cancelled`](HistoryError::Cancelled) when it is
//! cancelled before or while it works. Errors are typed and carry no source text (logs hold no user content by default).
//! Each port has a contract test in [`crate::contract`] that every adapter's own tests call.

use paddock_jobs::CancelToken;
use serde::{Deserialize, Serialize};

use crate::ids::{ChunkId, FileId, SymbolId, SymbolKind};
use crate::location::{Location, Position, Span};
use crate::pack::LspFeature;
use crate::path::RepoPath;
use crate::rev::{Generation, Rev};
use crate::tier::Confidence;

// ---- History -------------------------------------------------------------------------------------------------------

/// Why a [`History`] call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HistoryError {
    /// The path does not exist at that revision. An empty file is not "not found".
    #[error("{0} does not exist at that revision")]
    NotFound(RepoPath),
    /// The file is larger than the caller's cap.
    #[error("{path} is larger than {limit} bytes")]
    TooLarge {
        /// The file.
        path: RepoPath,
        /// The cap that was given.
        limit: u64,
    },
    /// The path is on the secret denylist: its content is never served (ADR 0005).
    #[error("{0} is withheld (secret denylist)")]
    Denied(RepoPath),
    /// The revision is unknown to the repository.
    #[error("unknown revision {0}")]
    UnknownRev(String),
    /// Cancelled by the caller.
    #[error("cancelled")]
    Cancelled,
    /// The backend failed (for example an unreadable object store); the text names the cause, not file content. It is for logs:
    /// no consumer serves it to a client (ADR 0004).
    #[error("history backend: {0}")]
    Backend(String),
}

/// How a file changed between two revisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// New file.
    Added,
    /// Content changed.
    Modified,
    /// File removed.
    Deleted,
    /// Moved from another path (content may also have changed).
    Renamed {
        /// The old path.
        from: RepoPath,
    },
}

/// One changed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    /// The path after the change (the old path for a deletion).
    pub path: RepoPath,
    /// What happened.
    pub kind: ChangeKind,
}

/// The files that changed between two revisions.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ChangeSet {
    /// Sorted by path, at most the caller's limit.
    pub changes: Vec<FileChange>,
    /// True when more files changed than `changes` holds. A walk that was cut (it stops after `2 * limit + 2` raw changes)
    /// may return **fewer than `limit` files** and may split a rename into an added and a deleted entry, because the other half
    /// was not reached; `truncated` is the signal that the list is a prefix of the changes, not all of them.
    pub truncated: bool,
    /// How many changed files are not listed because their names are not valid [`RepoPath`]s (a backslash, a control
    /// character): counted, never silently dropped.
    pub skipped_names: u32,
}

/// Read-only access to the repository's history. The adapter (gix, in process) never runs the `git` binary on the analysed
/// repository and never touches the working tree.
pub trait History: Send + Sync {
    /// The revision `HEAD` points at.
    fn head(&self, cancel: &CancelToken) -> Result<Rev, HistoryError>;

    /// The content of `path` at `rev`, at most `max_bytes` long.
    fn read_at(&self, rev: &Rev, path: &RepoPath, max_bytes: u64, cancel: &CancelToken) -> Result<Vec<u8>, HistoryError>;

    /// The files that changed from `from` to `to`, sorted by path, at most `limit` of them.
    fn changed_between(&self, from: &Rev, to: &Rev, limit: usize, cancel: &CancelToken) -> Result<ChangeSet, HistoryError>;
}

// ---- Store (read side) ---------------------------------------------------------------------------------------------

/// Why a [`Store`] call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// The index is being rebuilt or is damaged; the caller shows tier 1 meanwhile.
    #[error("the index is not available: {0}")]
    Unavailable(String),
    /// Cancelled by the caller.
    #[error("cancelled")]
    Cancelled,
    /// The backend failed.
    #[error("store backend: {0}")]
    Backend(String),
}

/// A symbol as the index stores it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolRecord {
    /// The symbol's id.
    pub id: SymbolId,
    /// The file it is in.
    pub file: FileId,
    /// What it is.
    pub kind: SymbolKind,
    /// Its qualified name.
    pub qualified_name: String,
    /// Where its declaration is.
    pub span: Span,
    /// How sure the index is that this is a real definition.
    pub confidence: Confidence,
}

/// The most symbols one page holds, whatever limit a caller asks for.
pub const MAX_SYMBOLS_PER_PAGE: usize = 100_000;

/// Why a page or reply is not valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{what} holds {len} entries; at most {max} are allowed")]
pub struct BoundError {
    /// What was too long.
    pub what: &'static str,
    /// Its length.
    pub len: usize,
    /// The bound.
    pub max: usize,
}

/// The wire form of a [`SymbolPage`].
#[derive(Serialize, Deserialize)]
pub(crate) struct SymbolPageWire {
    pub(crate) symbols: Vec<SymbolRecord>,
    pub(crate) truncated: bool,
}

impl TryFrom<SymbolPageWire> for SymbolPage {
    type Error = BoundError;
    fn try_from(w: SymbolPageWire) -> Result<SymbolPage, BoundError> {
        SymbolPage::new(w.symbols, w.truncated)
    }
}

/// A bounded page of symbols. Built through [`SymbolPage::new`], also when read from the wire.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(try_from = "SymbolPageWire")]
pub struct SymbolPage {
    symbols: Vec<SymbolRecord>,
    truncated: bool,
}

impl SymbolPage {
    /// A page of `symbols` (ordered by span start, at most [`MAX_SYMBOLS_PER_PAGE`]); `truncated` says the file declares more.
    pub fn new(symbols: Vec<SymbolRecord>, truncated: bool) -> Result<SymbolPage, BoundError> {
        if symbols.len() > MAX_SYMBOLS_PER_PAGE {
            return Err(BoundError {
                what: "a symbol page",
                len: symbols.len(),
                max: MAX_SYMBOLS_PER_PAGE,
            });
        }
        Ok(SymbolPage { symbols, truncated })
    }

    /// The symbols, ordered by span start, at most the caller's limit.
    pub fn symbols(&self) -> &[SymbolRecord] {
        &self.symbols
    }

    /// True when the file declares more symbols than the page holds.
    pub fn truncated(&self) -> bool {
        self.truncated
    }
}

/// The read side of the index. The one writer is a separate actor in `pacenotes-index`. Within one generation reads are
/// consistent: two calls that see the same [`Generation`] see the same data. Every call may touch disk, so each takes a
/// [`CancelToken`] and ends with [`StoreError::Cancelled`] when it is cancelled.
pub trait Store: Send + Sync {
    /// The current index generation.
    fn generation(&self, cancel: &CancelToken) -> Result<Generation, StoreError>;

    /// The id of the file now at `path`, or `None` when the index has no such file.
    fn file_id(&self, path: &RepoPath, cancel: &CancelToken) -> Result<Option<FileId>, StoreError>;

    /// At most `limit` of the symbols declared in `file`, ordered by span start; empty for an unknown file.
    fn symbols_in_file(&self, file: FileId, limit: usize, cancel: &CancelToken) -> Result<SymbolPage, StoreError>;
}

// ---- Embedder and VectorIndex --------------------------------------------------------------------------------------

/// Why a string is not a [`ModelId`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a model id (letters, digits and `._/-`, 1 to 128 characters)")]
pub struct ModelIdError(String);

/// The id of an embedding model, recorded with every vector: vectors of different models never mix.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ModelId(String);

impl ModelId {
    /// Parses a model id.
    pub fn parse(text: &str) -> Result<ModelId, ModelIdError> {
        let ok = (1..=128).contains(&text.len())
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b'-'));
        if ok {
            Ok(ModelId(text.to_owned()))
        } else {
            Err(ModelIdError(text.chars().take(40).collect()))
        }
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ModelId {
    type Error = ModelIdError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        ModelId::parse(&value)
    }
}

impl From<ModelId> for String {
    fn from(id: ModelId) -> String {
        id.0
    }
}

/// Why an [`Embedder`] call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EmbedError {
    /// The model is not installed or not reachable; the text says what to do.
    #[error("the embedding model is unavailable: {0}")]
    Unavailable(String),
    /// More than [`MAX_EMBED_TEXTS`] texts, or a text longer than [`MAX_EMBED_TEXT_BYTES`].
    #[error("too much to embed at once (at most {MAX_EMBED_TEXTS} texts of {MAX_EMBED_TEXT_BYTES} bytes)")]
    TooLarge,
    /// Cancelled by the caller.
    #[error("cancelled")]
    Cancelled,
    /// The backend failed.
    #[error("embedder backend: {0}")]
    Backend(String),
}

/// The most texts one [`Embedder::embed`] call takes.
pub const MAX_EMBED_TEXTS: usize = 256;
/// The longest text, in bytes, one [`Embedder::embed`] call takes; chunkers cut below it.
pub const MAX_EMBED_TEXT_BYTES: usize = 64 * 1024;

/// Turns text into vectors (Model2Vec in process, Pitwall, or nothing).
pub trait Embedder: Send + Sync {
    /// The model that produces the vectors.
    fn model(&self) -> &ModelId;

    /// The length of every vector.
    fn dimension(&self) -> usize;

    /// One vector per text, in the order of `texts`, each `dimension()` long. At most [`MAX_EMBED_TEXTS`] texts of at most
    /// [`MAX_EMBED_TEXT_BYTES`] each, else [`EmbedError::TooLarge`] before any work.
    fn embed(&self, texts: &[&str], cancel: &CancelToken) -> Result<Vec<Vec<f32>>, EmbedError>;
}

/// Why a [`VectorIndex`] call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VectorError {
    /// The vector comes from another model than the index holds.
    #[error("the index holds vectors of {index}, not {given}")]
    ModelMismatch {
        /// The index's model.
        index: String,
        /// The offered model.
        given: String,
    },
    /// The vector has the wrong length.
    #[error("expected {expected} dimensions, got {got}")]
    DimensionMismatch {
        /// The index's dimension.
        expected: usize,
        /// The offered length.
        got: usize,
    },
    /// A value is NaN or infinite.
    #[error("a vector holds a value that is not finite")]
    NotFinite,
    /// Cancelled by the caller.
    #[error("cancelled")]
    Cancelled,
    /// The backend failed.
    #[error("vector index backend: {0}")]
    Backend(String),
}

/// A nearest-neighbour hit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VectorHit {
    /// The chunk.
    pub chunk: ChunkId,
    /// Cosine similarity, higher is closer.
    pub score: f32,
}

/// The vector store behind semantic search.
pub trait VectorIndex: Send + Sync {
    /// The model whose vectors the index holds.
    fn model(&self) -> &ModelId;

    /// The length of every vector.
    fn dimension(&self) -> usize;

    /// Adds or replaces the vector of `chunk`. A vector of another model, of the wrong length or with a non-finite value
    /// is refused.
    fn insert(&mut self, chunk: ChunkId, model: &ModelId, vector: &[f32]) -> Result<(), VectorError>;

    /// Removes `chunk`; true when it was there.
    fn remove(&mut self, chunk: ChunkId) -> Result<bool, VectorError>;

    /// The `k` nearest chunks by cosine similarity, best first; equal scores are ordered by chunk id, so the result is
    /// deterministic.
    fn nearest(&self, query: &[f32], k: usize, cancel: &CancelToken) -> Result<Vec<VectorHit>, VectorError>;
}

// ---- LanguageServer ------------------------------------------------------------------------------------------------

/// Why a [`LanguageServer`] call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LspError {
    /// The server is not running, was stopped or is quarantined; the caller falls back to tier 2.
    #[error("the language server is unavailable: {0}")]
    Unavailable(String),
    /// The server's reply was malformed or too large.
    #[error("the language server sent a bad reply: {0}")]
    BadReply(String),
    /// The server did not answer in time.
    #[error("the language server did not answer in time")]
    TimedOut,
    /// Cancelled by the caller.
    #[error("cancelled")]
    Cancelled,
}

/// A question for a language server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspRequest {
    /// Which LSP feature to use.
    pub feature: LspFeature,
    /// The file the question is about.
    pub path: RepoPath,
    /// The position in it, when the feature needs one.
    pub position: Option<Position>,
}

/// The most locations one reply holds.
pub const MAX_LSP_LOCATIONS: usize = 10_000;

/// The wire form of an [`LspReply`].
#[derive(Serialize, Deserialize)]
pub(crate) struct LspReplyWire {
    pub(crate) locations: Vec<Location>,
    pub(crate) truncated: bool,
    pub(crate) outside_workspace: u32,
}

impl TryFrom<LspReplyWire> for LspReply {
    type Error = BoundError;
    fn try_from(w: LspReplyWire) -> Result<LspReply, BoundError> {
        if w.locations.len() > MAX_LSP_LOCATIONS {
            return Err(BoundError {
                what: "a language-server reply",
                len: w.locations.len(),
                max: MAX_LSP_LOCATIONS,
            });
        }
        Ok(LspReply {
            locations: w.locations,
            truncated: w.truncated,
            outside_workspace: w.outside_workspace,
        })
    }
}

/// A language server's answer: places in the repository, all [`Confidence::Exact`] by definition. At most
/// [`MAX_LSP_LOCATIONS`], also when read from the wire.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(try_from = "LspReplyWire")]
pub struct LspReply {
    locations: Vec<Location>,
    truncated: bool,
    outside_workspace: u32,
}

impl LspReply {
    /// A reply of `locations`, cut at [`MAX_LSP_LOCATIONS`] with `truncated` set.
    pub fn bounded(locations: Vec<Location>) -> LspReply {
        LspReply::with_outside(locations, 0)
    }

    /// Like [`LspReply::bounded`], recording how many places the server named outside the workspace (dropped).
    pub fn with_outside(mut locations: Vec<Location>, outside_workspace: u32) -> LspReply {
        let truncated = locations.len() > MAX_LSP_LOCATIONS;
        locations.truncate(MAX_LSP_LOCATIONS);
        LspReply {
            locations,
            truncated,
            outside_workspace,
        }
    }

    /// The places the server named inside the workspace.
    pub fn locations(&self) -> &[Location] {
        &self.locations
    }

    /// True when the server named more places than the reply holds.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// How many places the server named outside the workspace (standard library, dependencies); they are dropped, not
    /// an error.
    pub fn outside_workspace(&self) -> u32 {
        self.outside_workspace
    }
}

/// A running language server behind a supervisor (start on demand, timeouts, restart, quarantine).
pub trait LanguageServer: Send + Sync {
    /// Asks the server.
    fn request(&self, request: &LspRequest, cancel: &CancelToken) -> Result<LspReply, LspError>;
}

// ---- Llm -----------------------------------------------------------------------------------------------------------

/// The most tokens one call may be given.
pub const MAX_TOKENS: u32 = 32_768;

/// Why a token budget is not valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a token budget is between 1 and {MAX_TOKENS}")]
pub struct TokenBudgetError;

/// The most tokens a call may use: 1 to [`MAX_TOKENS`]. Reading one from the wire goes through [`TokenBudget::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32")]
pub struct TokenBudget(u32);

impl TokenBudget {
    /// A budget of `tokens`.
    pub fn new(tokens: u32) -> Result<TokenBudget, TokenBudgetError> {
        if (1..=MAX_TOKENS).contains(&tokens) {
            Ok(TokenBudget(tokens))
        } else {
            Err(TokenBudgetError)
        }
    }

    /// The number of tokens.
    pub fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for TokenBudget {
    type Error = TokenBudgetError;
    fn try_from(value: u32) -> Result<Self, TokenBudgetError> {
        TokenBudget::new(value)
    }
}

/// Why an [`Llm`] call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LlmError {
    /// No model is reachable (Pitwall is not set up); the text is the next step for the user.
    #[error("local AI is unavailable: {0}")]
    Unavailable(String),
    /// Cancelled by the caller.
    #[error("cancelled")]
    Cancelled,
    /// The backend failed.
    #[error("llm backend: {0}")]
    Backend(String),
}

/// A model's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Completion {
    /// The text.
    pub text: String,
    /// Tokens used, never more than the budget.
    pub tokens_used: u32,
    /// The model that answered, recorded with whatever is derived from the text.
    pub model: ModelId,
}

/// A local model (Pitwall), used only for bounded, user-started requests (family rule 46).
pub trait Llm: Send + Sync {
    /// Completes `prompt` within `budget`; the reply names the model.
    fn complete(&self, prompt: &str, budget: TokenBudget, cancel: &CancelToken) -> Result<Completion, LlmError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_ids_are_validated_and_convert() {
        assert!(ModelId::parse("minishlab/potion-code-16M-v2").is_ok());
        for bad in ["", "has space", &"a".repeat(129), "semi;colon"] {
            assert!(ModelId::parse(bad).is_err(), "{bad:?}");
        }
        let id = ModelId::try_from("a/b".to_owned()).unwrap();
        assert_eq!(String::from(id.clone()), "a/b");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"a/b\"");
        assert!(serde_json::from_str::<ModelId>("\"no no\"").is_err());
        assert!(ModelId::parse("no no").unwrap_err().to_string().contains("not a model id"));
    }

    #[test]
    fn token_budgets_are_bounded_and_validated_on_the_wire() {
        assert!(TokenBudget::new(0).is_err() && TokenBudget::new(MAX_TOKENS + 1).is_err());
        assert_eq!(TokenBudget::new(5).unwrap().get(), 5);
        assert!(serde_json::from_str::<TokenBudget>("0").is_err());
        assert_eq!(serde_json::from_str::<TokenBudget>("7").unwrap().get(), 7);
        assert!(TokenBudget::try_from(MAX_TOKENS + 1).is_err());
    }

    #[test]
    fn lsp_replies_are_cut_at_the_bound() {
        let loc = Location {
            path: RepoPath::parse("a.rs").unwrap(),
            span: Span::lines(1, 1).unwrap(),
        };
        let reply = LspReply::bounded(vec![loc.clone(); MAX_LSP_LOCATIONS + 1]);
        assert_eq!((reply.locations().len(), reply.truncated()), (MAX_LSP_LOCATIONS, true));
        assert!(!LspReply::bounded(vec![loc.clone()]).truncated());
        assert_eq!(LspReply::with_outside(vec![loc], 3).outside_workspace(), 3);
    }
}
