//! Exit codes and the errors that map to them. Scripts rely on these numbers, so they are stable.

use crate::args::UsageError;
use pitwall_core::mgmt::{CommandError, ErrorCode};
use pitwall_loopback::LoopbackError;

/// The process exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// Done.
    Ok = 0,
    /// Something failed.
    Error = 1,
    /// The command line is wrong.
    Usage = 2,
    /// The server is not running.
    NotRunning = 3,
    /// The server refused: not enough memory, offline mode, busy, not available here.
    Refused = 4,
}

impl Exit {
    /// The number for `std::process::exit`.
    pub fn code(self) -> i32 {
        self as i32
    }
}

/// A failed command: what to print on stderr and which exit code to end with.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct CliError {
    /// The exit code.
    pub exit: Exit,
    /// What went wrong and what to do about it.
    pub message: String,
}

impl CliError {
    /// A general failure (exit 1).
    pub fn failure(message: impl Into<String>) -> CliError {
        CliError {
            exit: Exit::Error,
            message: message.into(),
        }
    }

    /// The server is not running (exit 3), with the next step.
    pub fn not_running(port: u16) -> CliError {
        CliError {
            exit: Exit::NotRunning,
            message: format!(
                "Pitwall is not running on 127.0.0.1:{port}. Start the Pitwall app (or the pitwall-server service), then try again. \
                 If it listens on another port, pass --port <n>."
            ),
        }
    }
}

impl From<UsageError> for CliError {
    fn from(e: UsageError) -> CliError {
        CliError {
            exit: Exit::Usage,
            message: e.0,
        }
    }
}

/// The exit code for a management error.
pub fn exit_for(code: ErrorCode) -> Exit {
    match code {
        ErrorCode::ServerUnavailable => Exit::NotRunning,
        ErrorCode::InsufficientMemory | ErrorCode::Offline | ErrorCode::Busy | ErrorCode::Conflict | ErrorCode::NotAvailable => {
            Exit::Refused
        }
        ErrorCode::Unauthorized
        | ErrorCode::Forbidden
        | ErrorCode::NotFound
        | ErrorCode::BadRequest
        | ErrorCode::NotImplemented
        | ErrorCode::Internal => Exit::Error,
    }
}

/// A next step for the errors a user can fix.
fn hint(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::Unauthorized => {
            " The admin key was refused: set PITWALL_ADMIN_KEY or --key-file <path> to the key in the Pitwall data folder (admin.key)."
        }
        ErrorCode::Forbidden => " Only the admin key may use this command.",
        ErrorCode::Offline => " Turn off offline mode in Pitwall > Settings > Privacy to download.",
        ErrorCode::InsufficientMemory => " Unload a model (pitwall ps, pitwall unload <model>) or pick a smaller quantisation.",
        ErrorCode::Busy => " Try again in a moment.",
        _ => "",
    }
}

impl From<CommandError> for CliError {
    fn from(e: CommandError) -> CliError {
        let message = format!("{}.{}", e.message.trim_end_matches('.'), hint(e.code));
        CliError {
            exit: exit_for(e.code),
            message,
        }
    }
}

/// A transport failure: nothing listening is "not running", everything else a plain failure.
pub fn from_loopback(e: &LoopbackError, port: u16) -> CliError {
    match e {
        LoopbackError::Connect(..) => CliError::not_running(port),
        LoopbackError::Timeout(d) => CliError::failure(format!(
            "Pitwall did not answer within {} s. It may be busy loading a model: try again, or raise --timeout.",
            d.as_secs()
        )),
        other => CliError::failure(format!("{other}. Is something else using port {port}?")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_numbers_are_stable() {
        assert_eq!(
            [Exit::Ok, Exit::Error, Exit::Usage, Exit::NotRunning, Exit::Refused].map(Exit::code),
            [0, 1, 2, 3, 4]
        );
    }

    #[test]
    fn management_errors_map_to_exit_codes() {
        use ErrorCode::*;
        for (c, want) in [
            (ServerUnavailable, Exit::NotRunning),
            (InsufficientMemory, Exit::Refused),
            (Offline, Exit::Refused),
            (Busy, Exit::Refused),
            (Conflict, Exit::Refused),
            (NotAvailable, Exit::Refused),
            (Unauthorized, Exit::Error),
            (Forbidden, Exit::Error),
            (NotFound, Exit::Error),
            (BadRequest, Exit::Error),
            (NotImplemented, Exit::Error),
            (Internal, Exit::Error),
        ] {
            assert_eq!(exit_for(c), want, "{c:?}");
        }
    }

    #[test]
    fn messages_carry_a_next_step() {
        let e: CliError = CommandError::new(ErrorCode::Unauthorized, "the key is wrong").into();
        assert_eq!(e.exit, Exit::Error);
        assert!(e.message.contains("PITWALL_ADMIN_KEY"));
        let e: CliError = CommandError::new(ErrorCode::InsufficientMemory, "needs 9 GB.").into();
        assert_eq!(e.exit, Exit::Refused);
        assert!(e.message.starts_with("needs 9 GB.") && e.message.contains("pitwall unload"));
        let e: CliError = CommandError::new(ErrorCode::NotFound, "no such model").into();
        assert_eq!(e.message, "no such model.");
        for c in [ErrorCode::Forbidden, ErrorCode::Offline, ErrorCode::Busy] {
            assert!(CliError::from(CommandError::new(c, "x")).message.len() > 3);
        }
    }

    #[test]
    fn transport_failures() {
        let e = from_loopback(&LoopbackError::Connect(11435, "refused".into()), 11435);
        assert_eq!(e.exit, Exit::NotRunning);
        assert!(e.message.contains("Start the Pitwall app") && e.message.contains("11435"));
        let e = from_loopback(&LoopbackError::Timeout(Duration::from_secs(30)), 11435);
        assert_eq!(e.exit, Exit::Error);
        assert!(e.message.contains("30 s") && e.message.contains("--timeout"));
        assert!(from_loopback(&LoopbackError::Http("bad".into()), 5).message.contains("port 5"));
        assert_eq!(CliError::from(UsageError("x".into())).exit, Exit::Usage);
    }
}
