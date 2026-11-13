//! The rules, one file each. A rule answers one tool call: `None` allows, `Some` refuses with the reason.

use super::decision::Denial;
use super::input::ToolCall;
use super::lex::Script;

mod no_force_push;
mod no_kill_by_name;
mod no_pgrep_wait;
mod no_unbounded_wait;
mod no_unscoped_cleanup;

/// One guard rule.
pub trait Rule {
    /// The id logged on a refusal and used by tests.
    fn id(&self) -> &'static str;
    /// Decides the call. `script` is the parsed command of a Bash call, `None` for any other tool.
    fn check(&self, call: &ToolCall, script: Option<&Script>) -> Option<Denial>;
    /// The old shell hook's text patterns, run on the raw command when it could not be read (an unclosed quote, a limit): the
    /// hook then keeps the decisions the shell hooks gave instead of failing open on text they would have refused.
    fn legacy(&self, _text: &str) -> Option<Denial> {
        None
    }
}

/// Every rule, in the order they are asked. A new rule is a new file and one line here.
pub fn all() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(no_pgrep_wait::NoPgrepWait),
        Box::new(no_force_push::NoForcePush),
        Box::new(no_unbounded_wait::NoUnboundedWait),
        Box::new(no_unscoped_cleanup::NoUnscopedCleanup),
        Box::new(no_kill_by_name::NoKillByName),
    ]
}
