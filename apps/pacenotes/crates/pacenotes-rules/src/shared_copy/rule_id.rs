//! A rule's identity: one of ours, or one a tool gave.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Why a rule id was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleIdError {
    /// Not a valid id of ours.
    #[error("{0:?} is not a rule id; use lowercase letters, digits and `._/-`, starting with a letter or digit, at most 96 characters")]
    Local(String),
    /// A tool name or rule id of an imported result is empty, too long or has control characters.
    #[error("the {part} of an imported rule is {problem}")]
    External {
        /// `tool` or `id`.
        part: &'static str,
        /// What is wrong.
        problem: &'static str,
    },
}

/// A rule of a tool we import results from. Bounded and lossless: the strings are exactly what the tool wrote (no case folding, no
/// trimming), so a result round-trips to the same `ruleId`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ExternalRuleId {
    tool: String,
    id: String,
}

/// The longest tool name.
pub const MAX_TOOL_BYTES: usize = 64;
/// The longest imported rule id.
pub const MAX_EXTERNAL_ID_BYTES: usize = 256;

impl ExternalRuleId {
    /// Checks the bounds and keeps the strings as given.
    pub fn new(tool: &str, id: &str) -> Result<ExternalRuleId, RuleIdError> {
        let check = |part: &'static str, text: &str, max: usize| {
            if text.is_empty() {
                Err(RuleIdError::External { part, problem: "empty" })
            } else if text.len() > max {
                Err(RuleIdError::External { part, problem: "too long" })
            } else if text.chars().any(char::is_control) {
                Err(RuleIdError::External {
                    part,
                    problem: "made of more than printable characters",
                })
            } else {
                Ok(())
            }
        };
        check("tool", tool, MAX_TOOL_BYTES)?;
        check("id", id, MAX_EXTERNAL_ID_BYTES)?;
        Ok(ExternalRuleId {
            tool: tool.to_owned(),
            id: id.to_owned(),
        })
    }

    /// The tool's name.
    pub fn tool(&self) -> &str {
        &self.tool
    }

    /// The tool's own id for the rule, exactly as written.
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// A rule id. An imported rule can never be mistaken for one of ours.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RuleId {
    /// One of ours (Pacenotes' rules, conformance's checks).
    Local(pacenotes_core::rule::RuleId),
    /// One a tool gave.
    External(ExternalRuleId),
}

impl RuleId {
    /// A local id.
    pub fn local(text: &str) -> Result<RuleId, RuleIdError> {
        pacenotes_core::rule::RuleId::parse(text)
            .map(RuleId::Local)
            .map_err(|_| RuleIdError::Local(text.chars().take(40).collect()))
    }

    /// An imported id.
    pub fn external(tool: &str, id: &str) -> Result<RuleId, RuleIdError> {
        ExternalRuleId::new(tool, id).map(RuleId::External)
    }
}

impl fmt::Display for RuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleId::Local(id) => f.write_str(id.as_str()),
            RuleId::External(e) => write!(f, "{}:{}", e.tool, e.id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_ids_follow_the_core_rule_and_external_ones_are_kept_exactly() {
        assert!(RuleId::local("no-unwrap").is_ok());
        assert!(matches!(RuleId::local("No Unwrap"), Err(RuleIdError::Local(_))));
        let e = RuleId::external("clippy", "clippy::Needless_Return ").unwrap();
        assert_eq!(e.to_string(), "clippy:clippy::Needless_Return ");
        assert!(matches!(&e, RuleId::External(x) if x.id() == "clippy::Needless_Return " && x.tool() == "clippy"));
        assert!(RuleId::external("", "x").is_err() && RuleId::external("t", "").is_err());
        assert!(RuleId::external(&"t".repeat(65), "x").is_err() && RuleId::external("t", &"x".repeat(257)).is_err());
        assert!(RuleId::external("t", &"x".repeat(256)).is_ok());
        assert!(RuleId::external("t", "a\nb").is_err());
        assert_ne!(RuleId::local("a").unwrap(), RuleId::external("tool", "a").unwrap());
    }
}
