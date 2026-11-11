//! no-unbounded-wait (ER-32): a `while` or `until` loop that sleeps and nothing ends but its own condition.
//!
//! `until [ -f done ]; do sleep 5; done` waits forever when the file never appears. ER-32 wants every wait to end at a maximum
//! duration. A bound counts only when it bounds the loop: a `timeout` (or `gtimeout`) command whose nested shell (`bash -c '...'`, or
//! a heredoc to a shell) holds the whole loop, so that the loop is cut off when the time is up. A `timeout` on one command inside
//! the loop (`until timeout 2 curl x; do sleep 1; done`) bounds a single poll, and `--max` on a poll bounds nothing here: both are
//! refused. The way to wait with a maximum is `./dev wait ... --max`, which has no loop in the command, or `for i in $(seq N)`, which
//! is not a `while` or `until` loop.
//!
//! The rule reads each loop through [`crate::hook::loops`]: a `sleep` in the condition or the body of a `while` or `until` loop
//! (including its substitutions, backticks and nested shells, and after a `for` loop inside it), when the loop is not
//! [`Loop::timeout_wrapped`]. A command the lexer cannot read gets the text check ([`Rule::legacy`]): a `while` or `until` before a
//! `sleep` and no `timeout`/`gtimeout` word, which may refuse a harmless line but never lets a loop through for a stray quote.
//!
//! Known gap (the lexer reads a heredoc as data unless the command is a shell): `cat <<EOF | bash` with a sleeping loop in the body
//! is allowed. Variables and functions that build the loop are not read either (ADR 0083 D7).

use crate::hook::decision::Denial;
use crate::hook::input::ToolCall;
use crate::hook::lex::{Item, Script, basename};
use crate::hook::loops::{Kind, Loop, loops};
use crate::hook::rules::Rule;
use regex::Regex;
use std::sync::OnceLock;

/// Refuses a sleeping `while` or `until` loop that no `timeout` wraps.
pub struct NoUnboundedWait;

const REASON: &str = "Blocked: this wait loop has no maximum duration (ER-32); a timeout or --max on one poll does not bound the \
loop. Wrap the whole loop: `timeout 300 bash -c 'until ...; do sleep 5; done'`, or replace it with `./dev wait ... --max <duration>`; \
for a bounded retry use `for i in $(seq N); do ...; sleep 1; done`.";

impl NoUnboundedWait {
    fn denial(&self) -> Denial {
        Denial {
            rule: self.id(),
            reason: REASON.to_string(),
        }
    }
}

impl Rule for NoUnboundedWait {
    fn id(&self) -> &'static str {
        "no-unbounded-wait"
    }

    fn check(&self, _call: &ToolCall, script: Option<&Script>) -> Option<Denial> {
        let script = script?;
        let sleeps_unbounded = |lp: &Loop| {
            lp.kind != Kind::For
                && !lp.timeout_wrapped
                && lp.commands().any(|at| match &script.items[at] {
                    Item::Cmd(cmd) => cmd.command().first().is_some_and(|w| basename(&w.text) == "sleep"),
                    _ => false,
                })
        };
        loops(script).iter().any(sleeps_unbounded).then(|| self.denial())
    }

    fn legacy(&self, text: &str) -> Option<Denial> {
        static PATTERNS: OnceLock<Option<(Regex, Regex)>> = OnceLock::new();
        let (sleeping_loop, bound) = PATTERNS
            .get_or_init(|| {
                Some((
                    Regex::new(r"(?s)\b(while|until)\b.*\bsleep\b").ok()?,
                    Regex::new(r"\bg?timeout\b").ok()?,
                ))
            })
            .as_ref()?;
        (sleeping_loop.is_match(text) && !bound.is_match(text)).then(|| self.denial())
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
    fn a_sleeping_until_or_while_loop_without_a_wrapping_timeout_is_refused() {
        for line in [
            "until [ -f /tmp/done ]; do sleep 5; done",
            "while [ ! -f /tmp/done ]; do sleep 1; done",
            "while true; do echo waiting; sleep 2; done",
            "until curl -sf localhost:3000; do\n  sleep 1\ndone",
            "cd /x && until test -f y; do /bin/sleep 1; done",
            "bash -c 'until test -f y; do sleep 1; done'",
            "until test -f y; do sleep 1; done; echo ok",
            // a bound on one poll or one command is not a bound on the wait
            "until timeout 2 curl -sf localhost; do sleep 1; done",
            "while true; do timeout 5 curl x; sleep 1; done",
            "until ./dev wait --file y --line done --max 60s; do sleep 1; done",
            "until x --max=5; do sleep 1; done",
            "while true; do timeout 5 bash -c 'sleep 1'; done",
            // a for loop's `done` does not close the while loop
            "while true; do for i in 1 2; do :; done; sleep 1; done",
            "until x; do for i in 1; do for j in 1; do :; done; done; sleep 1; done",
            // the timeout must be the command, not a word of another command
            "echo timeout; while true; do sleep 1; done",
            "echo timeout 60 bash -c 'x'; until a; do sleep 1; done",
            // the sleep is in the condition, a substitution, a backtick or a nested shell of the loop
            "while sleep 1; do :; done",
            "until test -f y; do x=$(sleep 1); done",
            "until test -f y; do x=`sleep 1`; done",
            "until test -f y; do bash -c 'sleep 1'; done",
            "until test -f y; do\n:\nsleep 1\ndone",
        ] {
            assert!(denies(line), "{line}");
        }
    }

    #[test]
    fn a_timeout_around_the_whole_loop_bounds_the_wait() {
        for line in [
            "timeout 60 bash -c 'until test -f y; do sleep 1; done'",
            "timeout 60s sh -c 'while true; do sleep 1; done'",
            "gtimeout 60 bash -c 'until test -f y; do sleep 1; done'",
            "timeout -s KILL 60 bash -c 'until test -f y; do sleep 1; done'",
            "sudo timeout 60 bash -c 'until test -f y; do sleep 1; done'",
            "timeout 60 bash <<EOF\nuntil test -f y; do sleep 1; done\nEOF",
            "timeout 60 bash -c 'while a; do for i in 1; do :; done; sleep 1; done'",
        ] {
            assert!(!denies(line), "{line}");
        }
    }

    #[test]
    fn other_loops_and_other_sleeps_are_allowed() {
        for line in [
            "sleep 5",
            "for i in 1 2 3; do sleep 1; done",
            "for i in $(seq 30); do test -f y && break; sleep 1; done",
            "while read l; do echo $l; done < f",
            "until test -f y; do :; done",
            "while a; do :; done; sleep 1",
            "while a; do for i in 1; do :; done; done; sleep 1",
            "echo 'until x; do sleep 1; done'",
            "cat <<EOF\nuntil x; do sleep 1; done\nEOF",
            "./dev wait --file y --line done --max 60s",
        ] {
            assert!(!denies(line), "{line}");
        }
    }

    #[test]
    fn a_command_the_lexer_cannot_read_gets_the_text_check() {
        let legacy = |t: &str| NoUnboundedWait.legacy(t).is_some();
        assert!(legacy("echo 'x; until a; do sleep 1; done"));
        assert!(legacy("while a; do sleep 1; done # '"));
        assert!(!legacy("timeout 60 bash -c 'until a; do sleep 1; done"));
        assert!(!legacy("echo 'x; sleep 1"));
        assert!(!legacy("until a; do :; done # '"));
    }

    #[test]
    fn decide_refuses_it_end_to_end_with_the_rule_id() {
        let call = ToolCall::parse(r#"{"tool_name":"Bash","tool_input":{"command":"until test -f y; do sleep 1; done"}}"#).unwrap();
        let out = crate::hook::decide(&call);
        assert_eq!(out.denial.map(|d| d.rule), Some("no-unbounded-wait"));
        let ok =
            ToolCall::parse(r#"{"tool_name":"Bash","tool_input":{"command":"timeout 60 bash -c 'until test -f y; do sleep 1; done'"}}"#)
                .unwrap();
        assert!(crate::hook::decide(&ok).denial.is_none());
        let unread = ToolCall::parse(r#"{"tool_name":"Bash","tool_input":{"command":"until a; do sleep 1; done; echo 'x"}}"#).unwrap();
        assert_eq!(crate::hook::decide(&unread).denial.map(|d| d.rule), Some("no-unbounded-wait"));
    }

    #[test]
    fn it_ignores_other_tools_and_names_the_next_step() {
        let edit = ToolCall::parse(r#"{"tool_name":"Edit","tool_input":{"file_path":"/x"}}"#).unwrap();
        assert!(NoUnboundedWait.check(&edit, None).is_none());
        let script = Script::parse("until a; do sleep 1; done").unwrap();
        let d = NoUnboundedWait.check(&call(), Some(&script)).unwrap();
        assert_eq!(d.rule, "no-unbounded-wait");
        assert!(d.reason.contains("timeout 300 bash -c") && d.reason.contains("./dev wait") && d.reason.contains("seq N"));
    }
}
