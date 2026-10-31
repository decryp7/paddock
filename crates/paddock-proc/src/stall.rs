//! The report of a wait that is not going as it should (ADR 0031). A wait is never given up on only because time passed:
//! a stall says *why* it looks stuck and what the person can do, and the app maps it to its health chip and Retry.

use std::time::Duration;

/// Why a wait looks stuck, or is worth a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StallCause {
    /// Alive and not ready yet, nothing wrong seen: a note ("still starting"), the wait goes on.
    Slow,
    /// The process used this much CPU time and still did not become ready (spinning).
    Spinning(Duration),
    /// The process ran this long without using CPU or reporting progress (hung or deadlocked).
    Idle(Duration),
    /// Nothing about the process can be observed on this OS and the hang guard ran out.
    Hung(Duration),
    /// The OS or another app (App Tamer, a signal) has stopped the process: not stuck, it goes on when it is resumed.
    Paused,
    /// A supervisor (the login service, the OS) is meant to start the process and has not.
    WaitingForSupervisor,
    /// Another instance holds the single-instance lock and has not answered.
    HeldByOther,
}

impl StallCause {
    /// Whether the process itself looks stuck (the caller may end it), as opposed to a note or a wait on someone else.
    pub fn is_stuck(self) -> bool {
        matches!(
            self,
            StallCause::Spinning(_) | StallCause::Idle(_) | StallCause::Hung(_)
        )
    }
}

/// What the person (or the app on their behalf) can do about a stall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StallAction {
    /// Nothing yet: it may still finish.
    Wait,
    /// End the process that was started and start it again.
    Retry,
    /// Look at the log or the process's error output.
    ShowLog,
}

/// A stall with its cause, a sentence for the person and the suggested action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StallReport {
    /// Why.
    pub cause: StallCause,
    /// What to do about it.
    pub action: StallAction,
    /// A sentence in plain words, no jargon.
    pub message: String,
}

impl StallReport {
    /// The report for `cause` with the standard wording and action; `what` names the thing waited for ("the Dyno agent").
    pub fn new(cause: StallCause, what: &str) -> StallReport {
        let (action, message) = match cause {
            StallCause::Slow => (
                StallAction::Wait,
                format!(
                    "{what} is still starting. A slow disk or a busy computer can take a while."
                ),
            ),
            StallCause::Spinning(d) => (
                StallAction::Retry,
                format!(
                    "{what} has been busy for {} s without becoming ready.",
                    d.as_secs()
                ),
            ),
            StallCause::Idle(d) => (
                StallAction::Retry,
                format!(
                    "{what} has done nothing for {} s while starting.",
                    d.as_secs()
                ),
            ),
            StallCause::Hung(d) => (
                StallAction::ShowLog,
                format!(
                    "{what} has not become ready in {} s and cannot be observed.",
                    d.as_secs()
                ),
            ),
            StallCause::Paused => (
                StallAction::Wait,
                format!("{what} is paused by macOS or another app; it goes on when it is resumed."),
            ),
            StallCause::WaitingForSupervisor => (
                StallAction::Retry,
                format!("Waiting for {what}, which your system starts at login."),
            ),
            StallCause::HeldByOther => (
                StallAction::Retry,
                format!("Another copy of {what} holds the data folder and has not answered."),
            ),
        };
        StallReport {
            cause,
            action,
            message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cause_has_a_sentence_naming_the_thing_and_an_action() {
        let s = |n| Duration::from_secs(n);
        let causes = [
            (StallCause::Slow, StallAction::Wait, false),
            (StallCause::Spinning(s(120)), StallAction::Retry, true),
            (StallCause::Idle(s(60)), StallAction::Retry, true),
            (StallCause::Hung(s(300)), StallAction::ShowLog, true),
            (StallCause::Paused, StallAction::Wait, false),
            (StallCause::WaitingForSupervisor, StallAction::Retry, false),
            (StallCause::HeldByOther, StallAction::Retry, false),
        ];
        for (cause, action, stuck) in causes {
            let r = StallReport::new(cause, "the agent");
            assert_eq!(r.action, action, "{cause:?}");
            assert_eq!(cause.is_stuck(), stuck, "{cause:?}");
            assert!(r.message.contains("agent"), "{}", r.message);
        }
        assert_eq!(
            StallReport::new(StallCause::Paused, "the browser").message,
            "the browser is paused by macOS or another app; it goes on when it is resumed."
        );
        assert!(
            StallReport::new(StallCause::Spinning(s(120)), "x")
                .message
                .contains("120 s")
        );
    }
}
