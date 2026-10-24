//! The one error every command rejects with (plan X28): [`CommandError`] `{ code, message, retryable, hint?, field?,
//! cause?, kind? }`, camelCase on the wire and mirrored as `CommandError` in `@decryp7/paddock-ui/ipc` (generated from
//! this file by `ts-rs`, checked by [`crate::testing::sync_generated`]). The UI shows `message` (and `hint`, the next
//! step) and reacts to `code` and `retryable`, never to the text.
//!
//! The code list is short and general on purpose: a code means the same in every app. An app's own distinctions
//! (Pitcrew's `unknownRunner`, Debrief's `noModel`) travel in [`CommandError::kind`], a free string the app's UI knows.
//! Domain errors keep their own types inside the libraries and convert here, once, at the command edge.

use crate::guard::{Refusal, RefusalCode};
use serde::Serialize;
use std::fmt;

/// What kind of failure an error is (camelCase on the wire).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// The command failed; the message says why.
    Failed,
    /// A request the command refuses (a bad argument, a value out of range).
    Invalid,
    /// What the request names does not exist (any more).
    NotFound,
    /// A path the user did not choose: choose it again in a dialog.
    NotGranted,
    /// Something of the same kind is already running.
    Busy,
    /// Stopped on request.
    Cancelled,
    /// A file could not be read or written.
    Io,
    /// A bounded resource ran out (size, count, time).
    Limit,
    /// Not supported here (platform, format, feature).
    Unsupported,
    /// Off in this run (safe mode, a service that is not running, a missing component).
    Unavailable,
    /// Missing rights (administrator, access denied, a window that may not call the command).
    Denied,
    /// A program a tool runs is not installed.
    Missing,
    /// The call did not answer in time (the UI's call timeout, or a peer that stopped answering).
    Timeout,
    /// A bug inside the app (contained).
    Internal,
}

impl ErrorCode {
    /// Every code, in the order of the enum (also the order of the generated `ERROR_CODES` list).
    pub const ALL: [ErrorCode; 14] = [
        ErrorCode::Failed,
        ErrorCode::Invalid,
        ErrorCode::NotFound,
        ErrorCode::NotGranted,
        ErrorCode::Busy,
        ErrorCode::Cancelled,
        ErrorCode::Io,
        ErrorCode::Limit,
        ErrorCode::Unsupported,
        ErrorCode::Unavailable,
        ErrorCode::Denied,
        ErrorCode::Missing,
        ErrorCode::Timeout,
        ErrorCode::Internal,
    ];

    /// The camelCase name used on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Failed => "failed",
            ErrorCode::Invalid => "invalid",
            ErrorCode::NotFound => "notFound",
            ErrorCode::NotGranted => "notGranted",
            ErrorCode::Busy => "busy",
            ErrorCode::Cancelled => "cancelled",
            ErrorCode::Io => "io",
            ErrorCode::Limit => "limit",
            ErrorCode::Unsupported => "unsupported",
            ErrorCode::Unavailable => "unavailable",
            ErrorCode::Denied => "denied",
            ErrorCode::Missing => "missing",
            ErrorCode::Timeout => "timeout",
            ErrorCode::Internal => "internal",
        }
    }

    /// Whether trying the same call again can succeed: the default of [`CommandError::retryable`] for the code.
    pub fn retryable_by_default(self) -> bool {
        matches!(
            self,
            ErrorCode::Busy | ErrorCode::Unavailable | ErrorCode::Timeout | ErrorCode::Io
        )
    }
}

/// The error a command rejects with.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    /// What kind of failure.
    pub code: ErrorCode,
    /// For the user, as is: what happened.
    pub message: String,
    /// Whether the same call can succeed later (the UI offers Retry).
    pub retryable: bool,
    /// The next step for the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub hint: Option<String>,
    /// The argument a refusal is about.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub field: Option<String>,
    /// The service outage this is a symptom of (`health:store`): service health reports it once and no panel repeats it.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cause: Option<String>,
    /// The app's own distinction within the code (`noModel`, `unknownRunner`); the UI of that app matches on it.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub kind: Option<String>,
}

macro_rules! constructors {
    ($($(#[$doc:meta])* $name:ident => $code:ident),* $(,)?) => {
        $($(#[$doc])* pub fn $name(message: impl Into<String>) -> Self { Self::new(ErrorCode::$code, message) })*
    };
}

impl CommandError {
    /// An error with a code and a message; `retryable` takes the code's default.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        CommandError {
            code,
            message: message.into(),
            retryable: code.retryable_by_default(),
            hint: None,
            field: None,
            cause: None,
            kind: None,
        }
    }

    constructors! {
        /// [`ErrorCode::Failed`].
        failed => Failed,
        /// [`ErrorCode::Invalid`]: a refused argument.
        invalid => Invalid,
        /// [`ErrorCode::NotFound`].
        not_found => NotFound,
        /// [`ErrorCode::NotGranted`].
        not_granted => NotGranted,
        /// [`ErrorCode::Busy`].
        busy => Busy,
        /// [`ErrorCode::Cancelled`].
        cancelled => Cancelled,
        /// [`ErrorCode::Io`].
        io => Io,
        /// [`ErrorCode::Limit`].
        limit => Limit,
        /// [`ErrorCode::Unsupported`].
        unsupported => Unsupported,
        /// [`ErrorCode::Unavailable`].
        unavailable => Unavailable,
        /// [`ErrorCode::Denied`].
        denied => Denied,
        /// [`ErrorCode::Missing`].
        missing => Missing,
        /// [`ErrorCode::Timeout`].
        timeout => Timeout,
        /// [`ErrorCode::Internal`]: something that went wrong in the app.
        internal => Internal,
    }

    /// Overrides whether the call can be retried.
    #[must_use]
    pub fn retry(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    /// Adds the next step for the user.
    #[must_use]
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Names the argument the error is about.
    #[must_use]
    pub fn field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    /// Names the service outage this is a symptom of.
    #[must_use]
    pub fn cause(mut self, cause: impl Into<String>) -> Self {
        self.cause = Some(cause.into());
        self
    }

    /// Adds the app's own distinction.
    #[must_use]
    pub fn kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CommandError {}

impl From<std::io::Error> for CommandError {
    /// A missing file is `notFound`, a refused one `denied`, anything else `io`; the message is the OS's.
    fn from(e: std::io::Error) -> Self {
        let code = match e.kind() {
            std::io::ErrorKind::NotFound => ErrorCode::NotFound,
            std::io::ErrorKind::PermissionDenied => ErrorCode::Denied,
            _ => ErrorCode::Io,
        };
        CommandError::new(code, e.to_string())
    }
}

impl From<String> for CommandError {
    /// A plain text error of an older helper is `failed`.
    fn from(message: String) -> Self {
        CommandError::failed(message)
    }
}

impl From<&str> for CommandError {
    /// A plain text error of an older helper is `failed`.
    fn from(message: &str) -> Self {
        CommandError::failed(message)
    }
}

impl From<Refusal> for CommandError {
    /// A call the guard refused: `denied` or `limit`, never retryable.
    fn from(r: Refusal) -> Self {
        let code = match r.code {
            RefusalCode::Denied => ErrorCode::Denied,
            RefusalCode::Limit => ErrorCode::Limit,
        };
        CommandError::new(code, r.message).retry(false)
    }
}
