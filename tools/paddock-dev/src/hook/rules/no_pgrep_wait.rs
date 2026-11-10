//! no-pgrep-wait (ER-32): a wait loop that polls `pgrep -f`.
//!
//! `pgrep -f PATTERN` matches the waiting shell's own command line, so `while pgrep -f PATTERN; do sleep 5; done` never ends.
//! Ported from `.claude/hooks/no-pgrep-wait.sh`, whose regexes are the oracle (`oracle_tests`). The shell hook matched the text
//! `(while|until) ... pgrep -f` with no `;` in between; this rule reads the command: a `pgrep` with `-f` or `--full` inside the
//! condition of a `while` or `until` loop, that is, in the commands from the keyword up to the loop's `do`.

use crate::hook::decision::Denial;
use crate::hook::input::ToolCall;
use crate::hook::lex::{Item, Script, Simple, basename};
use crate::hook::rules::Rule;

/// Refuses a wait loop that polls `pgrep -f` (it matches the loop's own command line and never ends).
pub struct NoPgrepWait;

const REASON: &str = "Blocked: don't wait with pgrep -f (it matches its own command line). Wait on the job's own completion \
notification, plock's '<name> done' line in that job's output, or `./dev wait --pid/--plock/--file/--ci-run ... --max <duration>`.";

impl Rule for NoPgrepWait {
    fn id(&self) -> &'static str {
        "no-pgrep-wait"
    }

    fn check(&self, _call: &ToolCall, script: Option<&Script>) -> Option<Denial> {
        let script = script?;
        let mut in_condition = false;
        let mut saved: Vec<bool> = Vec::new();
        for item in &script.items {
            match item {
                Item::Enter => saved.push(in_condition),
                Item::Leave => in_condition = saved.pop().unwrap_or(false),
                Item::Sep(_) => {}
                Item::Cmd(cmd) => {
                    if cmd.is_do() {
                        in_condition = false;
                    }
                    if cmd.opens_condition() {
                        in_condition = true;
                    }
                    if in_condition && polls_full_command_line(cmd) {
                        return Some(Denial {
                            rule: self.id(),
                            reason: REASON.to_string(),
                        });
                    }
                }
            }
        }
        None
    }
}

/// The command runs `pgrep` with `--full` or a short flag cluster that has `f` (`-f`, `-fl`, `-af`), anywhere among its options
/// and arguments: `pgrep -u me -f x` and `pgrep x -f` poll the command line too. (The old regex stopped at the first option with
/// a value; closed on purpose.)
fn polls_full_command_line(cmd: &Simple) -> bool {
    cmd.words
        .iter()
        .enumerate()
        .filter(|(_, w)| basename(&w.text) == "pgrep")
        .any(|(k, _)| {
            cmd.words[k + 1..].iter().any(|w| {
                let t = w.text.as_str();
                t == "--full"
                    || t.strip_prefix('-')
                        .is_some_and(|b| !b.starts_with('-') && b.chars().take_while(char::is_ascii_alphanumeric).any(|c| c == 'f'))
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call() -> ToolCall {
        ToolCall::parse(r#"{"tool_name":"Bash","tool_input":{}}"#).unwrap()
    }

    fn denies(cmd: &str) -> bool {
        let script = Script::parse(cmd).unwrap();
        NoPgrepWait.check(&call(), Some(&script)).is_some()
    }

    #[test]
    fn the_flag_cluster_is_read_as_the_old_regex_read_it() {
        for yes in [
            "-f",
            "-fl",
            "-lf",
            "-af",
            "-x1f",
            "-n -o -f",
            "- -f",
            "-f=x",
            "-u me -f",
            "-u me -fl",
            "--full",
            "--full -l",
            "foo -f",
            "-- -f",
        ] {
            assert!(denies(&format!("while pgrep {yes} foo; do :; done")), "{yes}");
        }
        for no in ["foo", "-x foo", "-l", "-F foo", "-n 'a -f'", "-u me foo", "--fullx"] {
            assert!(!denies(&format!("while pgrep {no}; do :; done")), "{no}");
        }
    }

    #[test]
    fn the_condition_ends_at_do_and_not_at_a_semicolon() {
        assert!(denies("while a; pgrep -f x; do :; done"), "two commands of one condition");
        assert!(denies("while a\nb && pgrep -f x\ndo :\ndone"));
        assert!(!denies("while a; do pgrep -f x; done"), "the body is not the condition");
        assert!(!denies("while a; do :; done; pgrep -f x"), "after the loop");
        assert!(denies("while a; do :; done; until pgrep -f x; do :; done"), "a second loop");
    }

    #[test]
    fn a_substitution_in_a_condition_is_in_it_and_one_in_a_body_is_not() {
        assert!(denies("while [ -n \"$(pgrep -f x)\" ]; do :; done"));
        assert!(!denies("while a; do echo $(pgrep -f x); done"));
        assert!(denies("while a; do :; done; while [ \"$(pgrep -f x)\" ]; do :; done"));
    }

    #[test]
    fn nested_shells_are_read_and_their_loops_do_not_leak_out() {
        assert!(denies("bash -c 'while pgrep -f x; do sleep 1; done'"));
        assert!(denies("ssh h true; sh -lc \"until pgrep -f x; do sleep 1; done\""));
        // the nested loop's `do` must not end the outer condition
        assert!(denies("while bash -c 'for i in 1; do :; done'; pgrep -f x; do :; done"));
        assert!(!denies("bash -c 'while a; do :; done'; pgrep -f x"));
    }

    #[test]
    fn quoted_text_and_heredocs_are_not_commands() {
        assert!(!denies("echo 'while pgrep -f x; do :; done'"));
        assert!(!denies("cat <<EOF\nwhile pgrep -f x; do :; done\nEOF"));
        assert!(denies("bash <<EOF\nwhile pgrep -f x; do :; done\nEOF"));
    }

    #[test]
    fn it_ignores_other_tools_and_calls_without_a_command() {
        let edit = ToolCall::parse(r#"{"tool_name":"Edit","tool_input":{"file_path":"/x"}}"#).unwrap();
        assert!(NoPgrepWait.check(&edit, None).is_none());
    }

    #[test]
    fn the_reason_names_the_cause_and_the_next_step() {
        let script = Script::parse("while pgrep -f x; do :; done").unwrap();
        let d = NoPgrepWait.check(&call(), Some(&script)).unwrap();
        assert_eq!(d.rule, "no-pgrep-wait");
        assert!(d.reason.contains("matches its own command line") && d.reason.contains("./dev wait"));
    }
}
