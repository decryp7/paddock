//! The lifeline: the process whose life bounds a long-lived helper (ER-51, rule 89).
//!
//! A program under test can leave a helper running on purpose: `dev ci slot acquire` starts a detached slot holder that keeps its
//! slot until its job folder is removed or a release is asked for. When the test that started it is ended first (nextest stops a test
//! past its bound, or the run is interrupted), neither happens, and the holder lived on for its whole lifetime cap (3.5 h): orphaned
//! `ci slot hold` processes were found four times after timed-out runs. Every test command (`paddock_testkit::TestCommand`) names its
//! test process in [`VAR`]; children inherit the environment (ER-47), so a helper anywhere below it can read the pid and end itself
//! when that process has ended. The helper takes the pid's start time when it starts (the test is running then: it is waiting on
//! the command), and from then on acts on that identity, never on the bare pid (rule 53).
//!
//! Production never sets it: a CI job's holder is bounded by its job folder, its release and its cap, as before.

/// The variable that names the lifeline process by its pid (decimal).
pub const VAR: &str = "PADDOCK_LIFELINE_PID";

/// The pid in a value of [`VAR`], or `None` for an empty, malformed or zero value (no lifeline).
#[must_use]
pub fn parse(value: &str) -> Option<u32> {
    value.trim().parse::<u32>().ok().filter(|pid| *pid > 0)
}

/// The lifeline of this process, from its environment.
#[must_use]
pub fn from_env() -> Option<u32> {
    std::env::var(VAR).ok().as_deref().and_then(parse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pid_is_read_and_anything_else_is_no_lifeline() {
        assert_eq!(parse("4242"), Some(4242));
        assert_eq!(parse(" 7\n"), Some(7));
        for bad in ["", "0", "-1", "x", "12x", "4294967296"] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
    }
}
