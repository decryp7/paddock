use thiserror::Error;

/// Why a power request failed. Every variant says what the caller can do next.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PowerError {
    /// The request itself is wrong (an empty name or reason); fix the caller.
    #[error("invalid power request: {0}")]
    Invalid(String),
    /// This system cannot do it (no D-Bus session, no logind, an OS without the call); `reason` is shown to the user.
    #[error("not available here: {reason}")]
    Unavailable {
        /// Plain words for the user.
        reason: String,
    },
    /// The OS refused a call; `code` is its own result code (Win32 error, `IOReturn`), 0 when it gave none.
    #[error("{call} failed with code {code}: {detail}")]
    Os {
        /// The OS call that failed.
        call: &'static str,
        /// Its result code, 0 when the OS gave none.
        code: i64,
        /// The OS's own words, or a short description.
        detail: String,
    },
}
