//! The rules, one file each. A rule answers one tool call: `None` allows, `Some` refuses with the reason.

use super::decision::Denial;
use super::input::ToolCall;
use super::lex::Script;

mod no_force_push;
mod no_pgrep_wait;

/// One guard rule.
pub trait Rule {
    /// The id logged on a refusal and used by tests.
    fn id(&self) -> &'static str;
    /// Decides the call. `script` is the parsed command of a Bash call, `None` for any other tool.
    fn check(&self, call: &ToolCall, script: Option<&Script>) -> Option<Denial>;
}

/// Every rule, in the order they are asked. A new rule is a new file and one line here.
pub fn all() -> Vec<Box<dyn Rule>> {
    vec![Box::new(no_pgrep_wait::NoPgrepWait), Box::new(no_force_push::NoForcePush)]
}
