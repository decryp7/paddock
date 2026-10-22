//! The service's errors. Each one says what went wrong and what to do.

/// An error of the service binary.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The feature is not built yet.
    #[error("{0} is not implemented yet")]
    NotImplemented(&'static str),
    /// The command needs administrator rights.
    #[error("{0} needs administrator rights: start it from the Pitcrew app (Windows asks for permission) or an elevated prompt")]
    NeedsElevation(&'static str),
    /// The OS refused or failed an operation.
    #[error("{what} failed: {reason}")]
    Os {
        /// The operation.
        what: &'static str,
        /// The OS's reason.
        reason: String,
    },
    /// A file could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: String,
        /// The cause.
        source: std::io::Error,
    },
    /// A child process did not finish in time and was stopped.
    #[error("{program} did not finish within {secs} s and was stopped")]
    TimedOut {
        /// The program.
        program: String,
        /// The timeout.
        secs: u64,
    },
    /// This platform is not supported (yet).
    #[error("{0} is not supported on this platform yet")]
    Unsupported(&'static str),
}

impl Error {
    /// The process exit code for this error.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::NotImplemented(_) | Error::Unsupported(_) => 3,
            Error::NeedsElevation(_) => 4,
            _ => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_say_what_to_do() {
        let e = Error::NeedsElevation("Installing the service");
        assert!(e.to_string().contains("Pitcrew app"));
        assert_eq!(e.exit_code(), 4);
        assert_eq!(
            Error::NotImplemented("Supervision").to_string(),
            "Supervision is not implemented yet"
        );
        assert_eq!(Error::NotImplemented("x").exit_code(), 3);
        assert_eq!(
            Error::TimedOut {
                program: "gitea-runner".into(),
                secs: 10
            }
            .exit_code(),
            1
        );
    }
}
