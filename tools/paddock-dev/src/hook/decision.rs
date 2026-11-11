//! The answer of a rule: a refusal with the reason the agent reads.

use serde_json::{Value, json};

/// A refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denial {
    /// The id of the rule that refused (`no-pgrep-wait`).
    pub rule: &'static str,
    /// What the agent reads: the cause and the next step (ER-27).
    pub reason: String,
}

impl Denial {
    /// The decision object of Claude Code's PreToolUse hook output.
    pub fn value(&self) -> Value {
        json!({"hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": self.reason,
        }})
    }

    /// [`Denial::value`] as one line of JSON.
    pub fn json(&self) -> String {
        self.value().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_refusal_is_the_documented_object() {
        let d = Denial {
            rule: "r",
            reason: "because".into(),
        };
        let v: Value = serde_json::from_str(&d.json()).unwrap();
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(v["hookSpecificOutput"]["permissionDecisionReason"], "because");
        assert!(!d.json().contains('\n'));
    }
}
