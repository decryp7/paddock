//! What `dev fetch` can fail with, typed; the command edge turns it into a message that names the next step.

use paddock_cli::CliError;
use paddock_jobs::JobError;
use std::fmt;

/// Why a fetch did not finish.
#[derive(Debug, PartialEq, Eq)]
pub enum FetchError {
    /// The command line asked for something that cannot be done.
    Usage(String),
    /// The work failed; `hint` says what to do next.
    Failed {
        /// What went wrong.
        message: String,
        /// The next step, when there is one.
        hint: Option<String>,
    },
    /// Stopped on request (Ctrl+C); nothing half-done was left in place.
    Cancelled,
}

impl FetchError {
    /// A failure without a hint.
    pub fn failed(message: impl Into<String>) -> FetchError {
        FetchError::Failed {
            message: message.into(),
            hint: None,
        }
    }

    /// A failure with the next step.
    pub fn with_hint(message: impl Into<String>, hint: impl Into<String>) -> FetchError {
        FetchError::Failed {
            message: message.into(),
            hint: Some(hint.into()),
        }
    }

    /// An I/O failure on `what` (a path or an action).
    pub fn io(what: impl fmt::Display, error: &std::io::Error) -> FetchError {
        FetchError::failed(format!("{what}: {error}."))
    }
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchError::Usage(m) | FetchError::Failed { message: m, .. } => f.write_str(m),
            FetchError::Cancelled => f.write_str("Cancelled."),
        }
    }
}

impl std::error::Error for FetchError {}

impl From<JobError> for FetchError {
    fn from(e: JobError) -> Self {
        match e {
            JobError::Cancelled(_) => FetchError::Cancelled,
            other => FetchError::failed(sentence(&other.to_string())),
        }
    }
}

impl From<FetchError> for CliError {
    fn from(e: FetchError) -> Self {
        match e {
            FetchError::Usage(m) => CliError::usage(m),
            FetchError::Cancelled => CliError::interrupted("Cancelled; nothing half-done was left behind."),
            FetchError::Failed { message, hint: None } => CliError::failure(message),
            FetchError::Failed { message, hint: Some(h) } => CliError::failure(message).with_hint(h),
        }
    }
}

/// `text` ending in a full stop.
pub fn sentence(text: &str) -> String {
    let t = text.trim_end();
    if t.ends_with(['.', '!', '?']) {
        t.to_string()
    } else {
        format!("{t}.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_cli::Exit;
    use paddock_jobs::CancelReason;

    #[test]
    fn job_errors_map_to_cancel_or_a_sentence() {
        assert_eq!(FetchError::from(JobError::Cancelled(CancelReason::User)), FetchError::Cancelled);
        assert_eq!(
            FetchError::from(JobError::Failed("disk full".into())),
            FetchError::failed("disk full.")
        );
        assert_eq!(FetchError::from(JobError::Failed("done.".into())), FetchError::failed("done."));
        assert!(matches!(
            FetchError::from(JobError::Panicked("x".into())),
            FetchError::Failed { .. }
        ));
    }

    #[test]
    fn cli_errors_keep_the_exit_kind_and_the_hint() {
        let u: CliError = FetchError::Usage("bad".into()).into();
        assert_eq!(u.exit, Exit::Usage);
        let c: CliError = FetchError::Cancelled.into();
        assert_eq!(c.exit, Exit::Interrupted);
        let f: CliError = FetchError::with_hint("no", "do this").into();
        assert_eq!((f.message.as_str(), f.hint.as_str()), ("no", "do this"));
        let g: CliError = FetchError::failed("plain").into();
        assert!(g.hint.is_empty());
        assert_eq!(FetchError::Cancelled.to_string(), "Cancelled.");
        assert_eq!(FetchError::Usage("u".into()).to_string(), "u");
        let io = std::io::Error::other("boom");
        assert_eq!(FetchError::io("x.txt", &io).to_string(), "x.txt: boom.");
    }
}
