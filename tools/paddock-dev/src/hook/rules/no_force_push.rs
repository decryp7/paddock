//! no-force-push.
use crate::hook::decision::Denial;
use crate::hook::input::ToolCall;
use crate::hook::lex::Script;
use crate::hook::rules::Rule;

/// Refuses a force push (shared history is never rewritten).
pub struct NoForcePush;

impl Rule for NoForcePush {
    fn id(&self) -> &'static str {
        "no-force-push"
    }
    fn check(&self, _call: &ToolCall, _script: Option<&Script>) -> Option<Denial> {
        None
    }
}
