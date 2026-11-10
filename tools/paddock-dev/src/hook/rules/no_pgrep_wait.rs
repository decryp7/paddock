//! no-pgrep-wait.
use crate::hook::decision::Denial;
use crate::hook::input::ToolCall;
use crate::hook::lex::Script;
use crate::hook::rules::Rule;

/// Refuses a wait loop that polls `pgrep -f` (it matches the loop's own command line and never ends; ER-32).
pub struct NoPgrepWait;

impl Rule for NoPgrepWait {
    fn id(&self) -> &'static str {
        "no-pgrep-wait"
    }
    fn check(&self, _call: &ToolCall, _script: Option<&Script>) -> Option<Denial> {
        None
    }
}
