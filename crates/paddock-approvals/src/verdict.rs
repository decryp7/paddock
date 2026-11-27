//! The one verdict on a thing that needs approval.

/// What the approvals say about one exception.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// An approval of the right approver names exactly this.
    Approved,
    /// No approval. `line_to_add` is the text the approver records; an agent never writes it for the user.
    Missing {
        /// The line (or record) to add.
        line_to_add: String,
    },
    /// An approval exists but does not cover this (another approver, count or digest).
    Mismatch {
        /// Why it does not.
        why: String,
    },
    /// The approval is no longer needed: the exception it approves is already on the target branch (or gone), so the line goes.
    Stale {
        /// Why.
        why: String,
    },
}
