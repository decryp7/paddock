/// What the process exits with. The numbers are a contract scripts rely on and never change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Exit {
    /// Done.
    Ok = 0,
    /// The work failed (a file, the store, a query, the server answered with an error).
    Failure = 1,
    /// The command line was wrong.
    Usage = 2,
    /// The program or server the command talks to is not running.
    NotRunning = 3,
    /// The server refused: not enough memory, offline mode, busy, not available here.
    Refused = 4,
    /// Done, but a `--fail-on-findings` style check found something (for CI).
    Findings = 5,
    /// Stopped by Ctrl+C or SIGTERM (128 + SIGINT).
    Interrupted = 130,
}

impl Exit {
    /// Every code.
    pub const ALL: [Exit; 7] = [
        Exit::Ok,
        Exit::Failure,
        Exit::Usage,
        Exit::NotRunning,
        Exit::Refused,
        Exit::Findings,
        Exit::Interrupted,
    ];

    /// The number for `std::process::exit`.
    pub fn code(self) -> i32 {
        self as i32
    }

    /// A short name for the JSON error object and for `--help` text.
    pub fn name(self) -> &'static str {
        match self {
            Exit::Ok => "ok",
            Exit::Failure => "failure",
            Exit::Usage => "usage",
            Exit::NotRunning => "not-running",
            Exit::Refused => "refused",
            Exit::Findings => "findings",
            Exit::Interrupted => "interrupted",
        }
    }

    /// The exit codes as a help section ("0 ok", "2 usage", ...), for [`crate::Help::exit_codes`].
    pub fn describe(self) -> &'static str {
        match self {
            Exit::Ok => "done",
            Exit::Failure => "the work failed",
            Exit::Usage => "the command line is wrong",
            Exit::NotRunning => "the program this talks to is not running",
            Exit::Refused => "refused (memory, offline mode, busy, not available here)",
            Exit::Findings => "done, and the check found something",
            Exit::Interrupted => "stopped by Ctrl+C",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_numbers_are_a_stable_contract() {
        let codes: Vec<i32> = Exit::ALL.iter().map(|e| e.code()).collect();
        assert_eq!(codes, [0, 1, 2, 3, 4, 5, 130]);
        let mut names: Vec<&str> = Exit::ALL.iter().map(|e| e.name()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), Exit::ALL.len());
        assert!(Exit::ALL.iter().all(|e| !e.describe().is_empty()));
    }
}
