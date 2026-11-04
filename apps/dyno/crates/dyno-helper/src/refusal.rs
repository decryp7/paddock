//! Why the helper service did not start, in the words and codes the outside can see (ER-27). In Refuse mode a refused start
//! leaves the service STOPPED until Repair, and the service has no console and, until the data folder is verified, no log
//! file: [`paddock_service::service_main`] writes the reason to the OS log (the Windows Event Log under the service's own
//! source, the journal, the unified log) and into the service's exit code, which `sc query` and the agent read back. This
//! file is the helper's table: the failure, its code and its words ([`StartError`]).

use dyno_core::helper_wire::StartFailure;
use std::fmt;

/// Why [`crate::serve::serve`] returned an error, in the table of [`StartFailure`] (the exit code) and words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartError {
    /// Which refusal.
    pub failure: StartFailure,
    /// The sentence for the user and the log, with the next step.
    pub message: String,
}

impl StartError {
    /// A start error of `failure` with `message`.
    pub fn new(failure: StartFailure, message: impl Into<String>) -> Self {
        StartError {
            failure,
            message: message.into(),
        }
    }

    /// The service-specific exit code the Service Control Manager keeps.
    pub fn exit_code(&self) -> u32 {
        self.failure.code()
    }
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for StartError {}

impl paddock_service::ServiceExit for StartError {
    fn exit_code(&self) -> u32 {
        self.failure.code()
    }

    fn next_step(&self) -> Option<String> {
        Some(self.failure.words().to_string())
    }
}

impl From<StartError> for String {
    fn from(e: StartError) -> String {
        e.message
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_service::ServiceExit;

    #[test]
    fn a_refusal_names_its_cause_the_exit_code_and_the_next_step() {
        let e = StartError::new(
            StartFailure::NotProtected,
            "The helper will not start: the data folder (C:\\ProgramData\\Dyno\\Helper) is not protected: Users may add files.",
        );
        assert_eq!(StartError::exit_code(&e), StartFailure::NotProtected.code());
        assert_eq!(e.to_string(), e.message);
        assert_eq!(String::from(e.clone()), e.message);
        let text = paddock_service::service_main::failure_text("DynoHelper", &e, e.failure.code(), e.next_step().as_deref());
        for part in ["exit code 3", "data folder", "Users may add files", "Next step", "Repair"] {
            assert!(text.contains(part), "{part} missing: {text}");
        }
    }

    #[test]
    fn a_start_error_is_the_service_exit_of_the_shared_entry_point_with_its_code_and_next_step() {
        for failure in [
            StartFailure::Other,
            StartFailure::NotProtected,
            StartFailure::BadConfig,
            StartFailure::Endpoint,
        ] {
            let e = StartError::new(failure, "the reason");
            assert_eq!(ServiceExit::exit_code(&e), failure.code());
            assert_ne!(ServiceExit::exit_code(&e), 0, "0 is a clean stop");
            assert_eq!(e.next_step().as_deref(), Some(failure.words()));
        }
    }
}
