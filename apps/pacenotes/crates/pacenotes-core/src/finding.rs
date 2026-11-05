//! [`FindingState`]: the workflow state of a review finding, with the legal moves between states.

use serde::{Deserialize, Serialize};

/// A move between two finding states that the workflow does not allow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    /// The finding is already in that state.
    #[error("the finding is already {0:?}")]
    Unchanged(FindingState),
    /// The workflow has no such move.
    #[error("a finding cannot go from {from:?} to {to:?}")]
    Illegal {
        /// The current state.
        from: FindingState,
        /// The requested state.
        to: FindingState,
    },
}

/// Where a finding stands. A state is one value, never a set of flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingState {
    /// New, nobody has looked at it.
    Open,
    /// A person agreed that it is a real problem.
    Confirmed,
    /// A person said it is not an issue.
    Dismissed,
    /// The code no longer has the problem.
    Fixed,
    /// Silenced by a suppression with a reason.
    Suppressed,
}

impl FindingState {
    /// The states reachable in one move.
    pub fn next_states(self) -> &'static [FindingState] {
        use FindingState::{Confirmed, Dismissed, Fixed, Open, Suppressed};
        match self {
            Open => &[Confirmed, Dismissed, Fixed, Suppressed],
            Confirmed => &[Dismissed, Fixed, Suppressed],
            // A finding comes back to Open when the dismissal, the fix or the suppression no longer holds.
            Dismissed | Fixed | Suppressed => &[Open],
        }
    }

    /// The state after moving to `to`, or why that move is not allowed.
    pub fn transition(self, to: FindingState) -> Result<FindingState, TransitionError> {
        if self == to {
            return Err(TransitionError::Unchanged(self));
        }
        if self.next_states().contains(&to) {
            Ok(to)
        } else {
            Err(TransitionError::Illegal { from: self, to })
        }
    }

    /// True while the finding still counts against a CI threshold (it is neither dismissed, fixed nor suppressed).
    pub fn is_active(self) -> bool {
        matches!(self, FindingState::Open | FindingState::Confirmed)
    }
}

#[cfg(test)]
mod tests {
    use super::FindingState::*;
    use super::*;

    const ALL: [FindingState; 5] = [Open, Confirmed, Dismissed, Fixed, Suppressed];

    #[test]
    fn the_table_is_exactly_the_documented_moves() {
        let mut legal = 0;
        for from in ALL {
            for to in ALL {
                match from.transition(to) {
                    Ok(s) => {
                        assert_eq!(s, to);
                        legal += 1;
                    }
                    Err(TransitionError::Unchanged(s)) => assert_eq!((from, to), (s, s)),
                    Err(TransitionError::Illegal { .. }) => {}
                }
            }
        }
        assert_eq!(legal, 4 + 3 + 1 + 1 + 1);
    }

    #[test]
    fn a_closed_finding_cannot_be_confirmed_directly() {
        assert_eq!(
            Fixed.transition(Confirmed),
            Err(TransitionError::Illegal {
                from: Fixed,
                to: Confirmed
            })
        );
        assert_eq!(Fixed.transition(Open), Ok(Open));
        assert_eq!(Open.transition(Open), Err(TransitionError::Unchanged(Open)));
    }

    #[test]
    fn only_open_and_confirmed_count() {
        assert_eq!(ALL.iter().filter(|s| s.is_active()).count(), 2);
    }
}
