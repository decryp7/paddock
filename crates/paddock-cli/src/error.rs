use crate::Exit;

/// A failed command: what to tell the user, the next step, and the exit code to end with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    /// The exit code.
    pub exit: Exit,
    /// What went wrong.
    pub message: String,
    /// What to do about it (empty when there is nothing the user can do).
    pub hint: String,
}

impl CliError {
    /// An error with an explicit exit code.
    pub fn new(exit: Exit, message: impl Into<String>) -> CliError {
        CliError {
            exit,
            message: message.into(),
            hint: String::new(),
        }
    }

    /// A general failure (exit 1).
    pub fn failure(message: impl Into<String>) -> CliError {
        CliError::new(Exit::Failure, message)
    }

    /// A wrong command line (exit 2); say what to write.
    pub fn usage(message: impl Into<String>) -> CliError {
        CliError::new(Exit::Usage, message)
    }

    /// `what` is not running (exit 3), with the step that starts it.
    pub fn not_running(what: &str, start_with: &str) -> CliError {
        CliError::new(Exit::NotRunning, format!("{what} is not running.")).with_hint(start_with)
    }

    /// A refusal (exit 4).
    pub fn refused(message: impl Into<String>) -> CliError {
        CliError::new(Exit::Refused, message)
    }

    /// Stopped by Ctrl+C (exit 130).
    pub fn interrupted(message: impl Into<String>) -> CliError {
        CliError::new(Exit::Interrupted, message)
    }

    /// Adds the next step.
    pub fn with_hint(mut self, hint: impl Into<String>) -> CliError {
        self.hint = hint.into();
        self
    }

    /// The text for standard error: the message, then the hint on the same paragraph.
    pub fn text(&self) -> String {
        if self.hint.is_empty() {
            self.message.clone()
        } else {
            format!("{} {}", self.message, self.hint)
        }
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text())
    }
}

impl std::error::Error for CliError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_pick_the_exit_code() {
        assert_eq!(CliError::failure("x").exit, Exit::Failure);
        assert_eq!(CliError::usage("x").exit, Exit::Usage);
        assert_eq!(CliError::refused("x").exit, Exit::Refused);
        assert_eq!(CliError::interrupted("x").exit, Exit::Interrupted);
        let e = CliError::not_running("Pitwall", "Start the app, then try again.");
        assert_eq!(e.exit, Exit::NotRunning);
        assert_eq!(
            e.to_string(),
            "Pitwall is not running. Start the app, then try again."
        );
        assert_eq!(CliError::failure("boom").to_string(), "boom");
    }
}
