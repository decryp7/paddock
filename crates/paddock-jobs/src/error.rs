use crate::cancel::{CancelReason, Cancelled};
use paddock_limits::LimitExceeded;
use std::fmt;

/// Why a job body ended without a result. Typed; the app edge turns it into a message.
#[derive(Debug)]
pub enum JobError {
    /// Stopped on request or at the deadline.
    Cancelled(CancelReason),
    /// The work failed; the text says what to fix.
    Failed(String),
    /// A limit stopped the work. The type is kept so the job's failure can offer the remedy (ADR 0019).
    Limit {
        /// The limit, as a type.
        limit: LimitExceeded,
        /// What a person reads, built where the limit's registry is known (the layer that made the error).
        message: String,
    },
    /// A worker panicked (contained; the message is the panic payload when it was text).
    Panicked(String),
}

impl fmt::Display for JobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JobError::Cancelled(r) => write!(f, "cancelled ({r})"),
            JobError::Failed(m) => f.write_str(m),
            JobError::Limit { message, .. } => f.write_str(message),
            JobError::Panicked(m) => write!(f, "internal error: {m}"),
        }
    }
}

impl std::error::Error for JobError {}

impl From<LimitExceeded> for JobError {
    fn from(l: LimitExceeded) -> Self {
        // No registry here: the message has the value and no remedy. A layer that knows its registry builds the variant itself.
        JobError::Limit {
            message: l.for_person(&paddock_limits::Registry::new(&[])),
            limit: l,
        }
    }
}

impl From<Cancelled> for JobError {
    fn from(c: Cancelled) -> Self {
        JobError::Cancelled(c.0)
    }
}

impl From<std::io::Error> for JobError {
    fn from(e: std::io::Error) -> Self {
        JobError::Failed(e.to_string())
    }
}

/// Text of a panic payload, for [`JobError::Panicked`].
pub(crate) fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}
