//! One typed error for the whole crate. Messages tell the user what to do next and never contain
//! file contents or tokens.

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum AiError {
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Invalid(String),
    /// Pitwall serves no model for Debrief (not installed, not running, not paired, no model fits).
    #[error("Pitwall serves no model for Debrief yet: connect Debrief to Pitwall in Settings > AI")]
    NoModel,
    #[error("the model engine failed: {0}")]
    Engine(String),
    /// The browser (a page of the research or agent browser) failed: not open, not answering, changed under
    /// the step. The message is complete ("The browser could not ..."), so a nested job can pass it on as is.
    #[error("{0}")]
    Browser(String),
    #[error("the conversation does not fit the model's context window ({prompt} of {limit} tokens)")]
    ContextOverflow { prompt: usize, limit: usize },
    /// Images were given to a model that cannot read them (no image projector installed for it).
    #[error("the active model cannot read images: install its image support (projector) or choose a model marked \"Text + images\"")]
    NoVision,
    /// Pitwall has no model that fits the memory (or the request does not fit the model's memory).
    #[error("not enough memory: {0}")]
    Memory(String),
    #[error("the model produced an unusable answer: {0}")]
    Malformed(String),
    #[error("the assistant stopped after {0} tool rounds without an answer")]
    ToolLoopLimit(usize),
    #[error("network error: {0}")]
    Network(String),
    /// A request failed for a reason the user can act on (certificate, proxy, offline, timeout, HTTP status).
    #[error("{0}")]
    Connection(crate::net::NetError),
    #[error("this model needs a Hugging Face account: open {url}, accept the licence, then provide an access token")]
    Gated { url: String },
    #[error("not found: {0}")]
    NotFound(String),
    #[error("the download is corrupt: {0}")]
    Integrity(String),
    #[error("not enough disk space: {0}")]
    Disk(String),
    #[error("refused: {0}")]
    Refused(String),
    #[error("i/o error: {0}")]
    Io(String),
}

pub type Result<T> = std::result::Result<T, AiError>;

impl From<std::io::Error> for AiError {
    fn from(e: std::io::Error) -> Self {
        AiError::Io(e.to_string())
    }
}

impl AiError {
    /// Worth retrying after a pause (flaky network), as opposed to a definite answer.
    pub fn is_transient(&self) -> bool {
        match self {
            AiError::Network(_) => true,
            AiError::Connection(n) => n.is_transient(),
            _ => false,
        }
    }

    /// The host asked to slow down (HTTP 429/503): `Some(how long it asked to wait)`, `Some(None)` when it gave no time.
    pub fn throttle(&self) -> Option<Option<std::time::Duration>> {
        match self {
            AiError::Connection(n) if n.is_throttle() => Some(n.retry_after_secs.map(|s| std::time::Duration::from_secs(s.into()))),
            _ => None,
        }
    }
}
