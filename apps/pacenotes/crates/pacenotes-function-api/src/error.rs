//! The typed errors of the port. They carry no source text.

use serde::{Deserialize, Serialize};

use crate::budget::BudgetExceeded;

/// Why a host call failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum HostError {
    /// The caller cancelled.
    #[error("cancelled")]
    Cancelled,
    /// The call hit a limit of its budget; the error says which and where to change it.
    #[error("{0}")]
    BudgetExceeded(BudgetExceeded),
    /// The thing asked for does not exist.
    #[error("not found")]
    NotFound,
    /// A capability is not available (no index yet, no Pitwall); the text is the next step for the user.
    #[error("unavailable: {0}")]
    Unavailable(String),
    /// The request is outside what the function may ask (a path outside the workspace, a call it did not declare, a model
    /// call from background work).
    #[error("refused: {0}")]
    Refused(String),
    /// The host failed in a way the function cannot fix.
    #[error("host error: {0}")]
    Internal(String),
}

/// Why a tool call failed. A function never panics across the port: a fault becomes [`FunctionError::Internal`] (or, in
/// process, an attributed panic caught by the guard).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum FunctionError {
    /// The function has no such tool.
    #[error("unknown tool {0:?}")]
    UnknownTool(String),
    /// The arguments are not valid for the tool; the text says what to fix.
    #[error("invalid arguments: {0}")]
    InvalidArguments(String),
    /// The caller cancelled.
    #[error("cancelled")]
    Cancelled,
    /// The call hit a limit of its budget.
    #[error("{0}")]
    BudgetExceeded(BudgetExceeded),
    /// A host call the function needed failed.
    #[error("a host call failed: {0}")]
    Host(HostError),
    /// A fault inside the function.
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<HostError> for FunctionError {
    /// A cancelled or over-budget host call ends the tool the same way; anything else is wrapped.
    fn from(e: HostError) -> Self {
        match e {
            HostError::Cancelled => FunctionError::Cancelled,
            HostError::BudgetExceeded(b) => FunctionError::BudgetExceeded(b),
            other => FunctionError::Host(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::BudgetKind;

    #[test]
    fn cancel_and_budget_pass_through_and_the_rest_is_wrapped() {
        let b = BudgetExceeded {
            kind: BudgetKind::Items,
            limit: 3,
        };
        assert_eq!(FunctionError::from(HostError::Cancelled), FunctionError::Cancelled);
        assert_eq!(FunctionError::from(HostError::BudgetExceeded(b)), FunctionError::BudgetExceeded(b));
        assert_eq!(FunctionError::from(HostError::NotFound), FunctionError::Host(HostError::NotFound));
    }

    #[test]
    fn messages_name_the_problem() {
        assert!(FunctionError::UnknownTool("x".into()).to_string().contains("unknown tool"));
        assert!(
            HostError::Unavailable("set up Pitwall".into())
                .to_string()
                .contains("set up Pitwall")
        );
        let b = HostError::BudgetExceeded(BudgetExceeded {
            kind: BudgetKind::Bytes,
            limit: 9,
        });
        assert!(b.to_string().contains("9 bytes"));
    }
}
