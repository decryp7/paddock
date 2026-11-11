//! no-unbounded-wait (ER-32): a `while` or `until` loop that sleeps and has no bound.
//!
//! `until [ -f done ]; do sleep 5; done` waits forever when the file never appears. ER-32 wants every wait to end at a maximum
//! duration: a `timeout` in front of the command, or `./dev wait ... --max <duration>`. This rule reads the command: a `sleep`
//! between a `while` or `until` keyword and its `done`, in a call that has no `timeout` or `gtimeout` command and no `--max` option
//! anywhere. It is coarse on purpose (the bound may sit in the condition, around the loop or in a nested shell); a loop whose end is
//! guaranteed some other way takes a `timeout` too, which costs nothing.

use crate::hook::decision::Denial;
use crate::hook::input::ToolCall;
use crate::hook::lex::{Item, Script, basename};
use crate::hook::rules::Rule;

/// Refuses a sleeping `while` or `until` loop in a call with no `timeout` and no `--max`.
pub struct NoUnboundedWait;

const REASON: &str = "Blocked: this wait loop has no maximum duration (ER-32). Put `timeout <seconds>` in front of it, or wait with \
`./dev wait --pid/--plock/--file/--ci-run ... --max <duration>`, which always ends at --max with the next step.";

impl Rule for NoUnboundedWait {
    fn id(&self) -> &'static str {
        "no-unbounded-wait"
    }

    fn check(&self, _call: &ToolCall, script: Option<&Script>) -> Option<Denial> {
        let items = &script?.items;
        let mut loops: Vec<usize> = Vec::new();
        let mut depth = 0usize;
        let mut sleeps = false;
        let mut bounded = false;
        for item in items {
            match item {
                Item::Enter => loops.push(depth),
                Item::Leave => depth = loops.pop().unwrap_or(0),
                Item::Sep(_) => {}
                Item::Cmd(cmd) => {
                    if cmd.opens_condition() {
                        depth += 1;
                    }
                    if depth > 0 && cmd.command().first().is_some_and(|w| basename(&w.text) == "sleep") {
                        sleeps = true;
                    }
                    if cmd.words.first().is_some_and(|w| w.text == "done") {
                        depth = depth.saturating_sub(1);
                    }
                    if cmd
                        .words
                        .iter()
                        .any(|w| matches!(basename(&w.text), "timeout" | "gtimeout") || w.text == "--max" || w.text.starts_with("--max="))
                    {
                        bounded = true;
                    }
                }
            }
        }
        (sleeps && !bounded).then(|| Denial {
            rule: self.id(),
            reason: REASON.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call() -> ToolCall {
        ToolCall::parse(r#"{"tool_name":"Bash","tool_input":{}}"#).unwrap()
    }

    fn denies(cmd: &str) -> bool {
        let script = Script::parse(cmd).unwrap();
        NoUnboundedWait.check(&call(), Some(&script)).is_some()
    }

    #[test]
    fn a_sleeping_until_or_while_loop_without_a_bound_is_refused() {
        for line in [
            "until [ -f /tmp/done ]; do sleep 5; done",
            "while [ ! -f /tmp/done ]; do sleep 1; done",
            "while true; do echo waiting; sleep 2; done",
            "until curl -sf localhost:3000; do\n  sleep 1\ndone",
            "cd /x && until test -f y; do /bin/sleep 1; done",
            "bash -c 'until test -f y; do sleep 1; done'",
            "until test -f y; do sleep 1; done; echo ok",
            "while ./dev wait --pid 1; do sleep 1; done",
        ] {
            assert!(denies(line), "{line}");
        }
    }

    #[test]
    fn a_timeout_or_max_bounds_the_wait() {
        for line in [
            "timeout 60 bash -c 'until test -f y; do sleep 1; done'",
            "timeout 60s sh -c 'while true; do sleep 1; done'",
            "gtimeout 60 bash -c 'until test -f y; do sleep 1; done'",
            "until ./dev wait --file y --line done --max 60s; do sleep 1; done",
            "./dev wait --plock build --max 10m",
            "until x --max=5; do sleep 1; done",
        ] {
            assert!(!denies(line), "{line}");
        }
    }

    #[test]
    fn other_loops_and_other_sleeps_are_allowed() {
        for line in [
            "sleep 5",
            "for i in 1 2 3; do sleep 1; done",
            "while read l; do echo $l; done < f",
            "until test -f y; do :; done",
            "while a; do :; done; sleep 1",
            "echo 'until x; do sleep 1; done'",
            "cat <<EOF\nuntil x; do sleep 1; done\nEOF",
        ] {
            assert!(!denies(line), "{line}");
        }
    }

    #[test]
    fn it_ignores_other_tools_and_names_the_next_step() {
        let edit = ToolCall::parse(r#"{"tool_name":"Edit","tool_input":{"file_path":"/x"}}"#).unwrap();
        assert!(NoUnboundedWait.check(&edit, None).is_none());
        let script = Script::parse("until a; do sleep 1; done").unwrap();
        let d = NoUnboundedWait.check(&call(), Some(&script)).unwrap();
        assert_eq!(d.rule, "no-unbounded-wait");
        assert!(d.reason.contains("timeout") && d.reason.contains("--max"));
    }
}
