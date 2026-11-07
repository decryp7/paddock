//! Who decides whether a tool call runs: the [`ApprovalPolicy`] port (ADR 0023 Decision 8, WP-2a).
//!
//! The loop asks the policy about every call that could run (a known, offered tool with readable arguments and budget
//! left); malformed calls, unknown tools and bad arguments are refused before the policy sees them. The policy answers
//! with a [`Verdict`]: run without asking, run on a remembered approval, or ask the user, with or without "Always allow".
//! The user's [`ApprovalChoice`] is then told back to the policy through [`ApprovalPolicy::record`], so a policy that
//! remembers (Pitwall's: ask the first time per tool, remember the answer, ask an action every call) keeps its own state.
//! The model can neither approve nor reach the approval path: a choice only comes from [`crate::Assistant::approve`].
//!
//! [`DefaultPolicy`] keeps the behaviour the family had before the port: read-only tools run without asking, every call of
//! a tool that writes asks, and nothing is remembered.

use crate::tool::Access;
use crate::types::{ToolCall, ToolSpec};
use serde::{Deserialize, Serialize};

/// What the user chose on an approval card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum ApprovalChoice {
    /// Run this call only.
    AllowOnce,
    /// Run it, and let the policy remember (offered only when the card says `remember`).
    AlwaysAllow,
    /// Do not run it; the model is told and the turn goes on.
    Deny,
}

/// How a call that ran was approved, shown on its card and kept with the conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum Approval {
    /// The policy let it run without asking.
    NotAsked,
    /// The user allowed this call once.
    Once,
    /// The user allowed it and asked to remember (this call is the first of many).
    Always,
    /// It ran on an approval the user gave earlier.
    Remembered,
}

/// What the policy says about one call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Run without asking ([`Approval::NotAsked`]).
    Run,
    /// Run on an earlier "Always allow" ([`Approval::Remembered`]).
    Remembered,
    /// Ask the user, with Allow once and Deny only (an action: ask every call).
    AskEveryTime,
    /// Ask the user, offering "Always allow" too.
    AskOrRemember,
}

impl Verdict {
    /// Whether the user is asked.
    pub fn asks(self) -> bool {
        matches!(self, Verdict::AskEveryTime | Verdict::AskOrRemember)
    }
}

/// The call the policy judges: the tool as it is offered this turn, its access and the parsed call.
#[derive(Debug, Clone, Copy)]
pub struct CallInfo<'a> {
    /// The tool's name, description and schema as the model saw them (a policy that remembers keys on them, so a changed
    /// tool is asked about again).
    pub tool: &'a ToolSpec,
    /// Whether it only looks or changes things.
    pub access: Access,
    /// The call: id, name and the arguments the card shows.
    pub call: &'a ToolCall,
}

/// Decides, per call, whether the user is asked; hears what the user chose. Implementations must not block and must not
/// panic; they are called on the turn's worker thread.
pub trait ApprovalPolicy: Send + Sync {
    /// Whether `call` runs, runs on a remembered approval, or waits for the user.
    fn check(&self, call: &CallInfo<'_>) -> Verdict;

    /// The user's choice on a call this policy asked about ([`Verdict::asks`]). Not called for a call that ran unasked, nor
    /// when the turn stopped or the wait ran out. The default remembers nothing.
    fn record(&self, call: &CallInfo<'_>, choice: ApprovalChoice) {
        let _ = (call, choice);
    }
}

/// Read-only tools run without asking; every call of a tool that writes asks, without "Always allow".
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultPolicy;

impl ApprovalPolicy for DefaultPolicy {
    fn check(&self, call: &CallInfo<'_>) -> Verdict {
        if call.access.writes() {
            Verdict::AskEveryTime
        } else {
            Verdict::Run
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_default_policy_asks_only_for_writes_and_remembers_nothing() {
        let spec = ToolSpec {
            name: "t".into(),
            description: String::new(),
            input_schema: json!({"type": "object"}),
        };
        let call = ToolCall {
            id: "c".into(),
            name: "t".into(),
            input: json!({}),
        };
        let at = |access| CallInfo {
            tool: &spec,
            access,
            call: &call,
        };
        assert_eq!(DefaultPolicy.check(&at(Access::ReadOnly)), Verdict::Run);
        assert_eq!(
            DefaultPolicy.check(&at(Access::Write)),
            Verdict::AskEveryTime
        );
        assert_eq!(
            DefaultPolicy.check(&at(Access::Destructive)),
            Verdict::AskEveryTime
        );
        DefaultPolicy.record(&at(Access::Write), ApprovalChoice::AlwaysAllow);
        assert_eq!(
            DefaultPolicy.check(&at(Access::Write)),
            Verdict::AskEveryTime
        );
        assert!(!Verdict::Run.asks() && !Verdict::Remembered.asks());
        assert!(Verdict::AskEveryTime.asks() && Verdict::AskOrRemember.asks());
    }
}
