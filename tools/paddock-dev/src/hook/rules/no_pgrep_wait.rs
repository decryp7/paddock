//! no-pgrep-wait (ER-32): a wait loop that polls `pgrep -f`.
//!
//! `pgrep -f PATTERN` matches the waiting shell's own command line, so `while pgrep -f PATTERN; do sleep 5; done` never ends.
//! Ported from `.claude/hooks/no-pgrep-wait.sh`, whose regexes are the oracle (`oracle_tests`). The shell hook matched the text
//! `(while|until) ... pgrep -f` with no `;` in between; this rule reads the command: a `pgrep` with `-f` or `--full` inside the
//! condition of a `while` or `until` loop, that is, in the commands from the keyword up to the loop's `do`.

use crate::hook::decision::Denial;
use crate::hook::input::ToolCall;
use crate::hook::lex::{Item, Script, Sep, Simple, basename};
use crate::hook::rules::Rule;
use regex::Regex;
use std::sync::OnceLock;

/// Refuses a wait loop that polls `pgrep -f` (it matches the loop's own command line and never ends).
pub struct NoPgrepWait;

const REASON: &str = "Blocked: don't wait with pgrep -f (it matches its own command line). Wait on the job's own completion \
notification, plock's '<name> done' line in that job's output, or `./dev wait --pid/--plock/--file/--ci-run ... --max <duration>`.";

impl NoPgrepWait {
    fn denial(&self) -> Denial {
        Denial {
            rule: self.id(),
            reason: REASON.to_string(),
        }
    }
}

/// Where the walk over the commands stands: inside a loop condition, and how many loops deep.
#[derive(Clone, Copy, Default)]
struct State {
    in_condition: bool,
    loops: usize,
}

impl Rule for NoPgrepWait {
    fn id(&self) -> &'static str {
        "no-pgrep-wait"
    }

    fn check(&self, _call: &ToolCall, script: Option<&Script>) -> Option<Denial> {
        let items = &script?.items;
        let mut state = State::default();
        let mut saved: Vec<State> = Vec::new();
        for (idx, item) in items.iter().enumerate() {
            match item {
                Item::Enter => saved.push(state),
                Item::Leave => state = saved.pop().unwrap_or_default(),
                Item::Sep(_) => {}
                Item::Cmd(cmd) => {
                    if cmd.is_do() {
                        state.in_condition = false;
                    }
                    if cmd.opens_condition() {
                        state.in_condition = true;
                        state.loops += 1;
                    } else if cmd.opens_for() {
                        state.loops += 1;
                    }
                    if cmd.words.first().is_some_and(|w| w.text == "done") {
                        state.loops = state.loops.saturating_sub(1);
                    }
                    if !polls_full_command_line(cmd) {
                        continue;
                    }
                    // In a condition it is the loop's test. In a body it is the test when `|| break`, `&& break` or `if ...; then
                    // break` follows it.
                    if state.in_condition || (state.loops > 0 && decides_the_loop(items, idx, cmd)) {
                        return Some(self.denial());
                    }
                }
            }
        }
        None
    }

    fn legacy(&self, text: &str) -> Option<Denial> {
        legacy_pattern().is_some_and(|p| p.is_match(text)).then(|| self.denial())
    }
}

/// The shell hook's pattern, on text.
fn legacy_pattern() -> Option<&'static Regex> {
    static PATTERN: OnceLock<Option<Regex>> = OnceLock::new();
    PATTERN
        .get_or_init(|| Regex::new(r"(while|until)[^;]*pgrep[[:space:]]+(-[[:alnum:]]*[[:space:]]+)*-[[:alnum:]]*f").ok())
        .as_ref()
}

const LEAVES: &[&str] = &["break", "exit", "return"];

/// What follows the poll at its own level ends the loop on its result:
/// - an `&&`/`||` chain with a `break`, `exit` or `return` in it (`pgrep -f x || break`, `pgrep -f x && sleep 1 || break`);
/// - the poll is an `if` whose `then` breaks;
/// - the next command tests `$?` and that chain or `then` breaks (`pgrep -f x; [ $? -ne 0 ] && break`).
///
/// Nested pieces after the poll (substitutions) and the closings of the pieces the poll sits in are skipped, so
/// `[ -z "$(pgrep -f x)" ] || break` is seen.
fn decides_the_loop(items: &[Item], idx: usize, cmd: &Simple) -> bool {
    let mut following: Vec<&Item> = Vec::new();
    let mut nested = 0usize;
    for item in &items[idx + 1..] {
        match item {
            Item::Enter => nested += 1,
            Item::Leave => nested = nested.saturating_sub(1),
            other if nested == 0 => following.push(other),
            _ => {}
        }
        if following.len() >= 12 {
            break;
        }
    }
    let separated = |at: usize, seps: &[Sep]| matches!(following.get(at), Some(Item::Sep(s)) if seps.contains(s));
    let then_leaves =
        |at: usize| matches!(following.get(at), Some(Item::Cmd(c)) if c.words.first().is_some_and(|w| w.text == "then") && leaves(c));
    // `|| break`, `&& echo up && sleep 1 || break`
    let chain_from = |start: usize| {
        let mut at = start;
        while separated(at, &[Sep::And, Sep::Or]) {
            match following.get(at + 1) {
                Some(Item::Cmd(c)) if leaves(c) => return true,
                Some(Item::Cmd(_)) => at += 2,
                _ => return false,
            }
        }
        false
    };
    if chain_from(0) {
        return true;
    }
    if !separated(0, &[Sep::Semi, Sep::Newline]) {
        return false;
    }
    if opens_if(cmd) && then_leaves(1) {
        return true;
    }
    match following.get(1) {
        Some(Item::Cmd(next)) if next.words.iter().any(|w| w.text.contains("$?")) => {
            chain_from(2) || (opens_if(next) && separated(2, &[Sep::Semi, Sep::Newline]) && then_leaves(3))
        }
        _ => false,
    }
}

fn leaves(c: &Simple) -> bool {
    c.command().first().is_some_and(|w| LEAVES.contains(&w.text.as_str()))
}

fn opens_if(cmd: &Simple) -> bool {
    cmd.words
        .iter()
        .take_while(|w| w.assignment || matches!(w.text.as_str(), "do" | "then" | "else" | "elif" | "if" | "!" | "{"))
        .any(|w| w.text == "if" || w.text == "elif")
}

/// The command runs `pgrep` or `pkill` with `--full` (or an abbreviation of at least `--fu`) or a short flag cluster that has `f`
/// (`-f`, `-fl`, `-af`), anywhere among its options and arguments: `pgrep -u me -f x` and `pgrep x -f` poll the command line too.
/// (The old regex stopped at the first option with a value; closed on purpose.)
fn polls_full_command_line(cmd: &Simple) -> bool {
    cmd.words
        .iter()
        .enumerate()
        .filter(|(_, w)| matches!(basename(&w.text), "pgrep" | "pkill"))
        .any(|(k, _)| {
            cmd.words[k + 1..].iter().any(|w| {
                let t = w.text.as_str();
                match t.strip_prefix("--") {
                    Some(long) => {
                        let name = long.split('=').next().unwrap_or(long);
                        name.len() >= 2 && "full".starts_with(name)
                    }
                    None => t
                        .strip_prefix('-')
                        .is_some_and(|b| b.chars().take_while(char::is_ascii_alphanumeric).any(|c| c == 'f')),
                }
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

    #[test]
    fn a_pgrep_poll_in_a_loop_body_that_decides_the_loop_is_refused() {
        for line in [
            "while true; do pgrep -f x || break; sleep 5; done",
            "for i in 1 2 3; do pgrep -f x >/dev/null || break; sleep 1; done",
            "for i in $(seq 100); do pgrep -f x && break; sleep 1; done",
            "until false; do if ! pgrep -f x; then break; fi; sleep 1; done",
            "while :; do pgrep -f x || exit 0; sleep 1; done",
            "while :; do pgrep -f x || return; sleep 1; done",
            "while true; do [ -z \"$(pgrep -f x)\" ] || break; sleep 1; done",
            "while true; do sleep 1; if pgrep -fl x; then break; fi; done",
        ] {
            assert!(denies(line), "{line}");
        }
        for line in [
            "pgrep -f x || break",
            "for i in 1 2; do pgrep -f x || echo none; done",
            "while a; do :; done; pgrep -f x || break",
            "while true; do pgrep x || break; done",
            "while true; do pgrep -f x; sleep 1; done",
            "for i in 1; do pgrep -f x; done; echo break",
            "for i in 1; do :; done\npgrep -f x || break",
        ] {
            assert!(!denies(line), "{line}");
        }
    }

    #[test]
    fn pkill_and_abbreviated_full_are_polls_too() {
        for line in [
            "while pkill -0 -f x; do sleep 1; done",
            "while pgrep --fu x; do :; done",
            "while pgrep --ful x; do :; done",
            "until ! pkill -0 --full x; do :; done",
        ] {
            assert!(denies(line), "{line}");
        }
        for line in [
            "while pkill x; do sleep 1; done",
            "while pgrep --fullx x; do :; done",
            "while pgrep --f x; do :; done",
        ] {
            assert!(!denies(line), "{line}");
        }
    }

    #[test]
    fn the_text_fallback_decides_what_the_lexer_cannot_read() {
        let legacy = |t: &str| NoPgrepWait.legacy(t).is_some();
        for yes in [
            "while pgrep -f x; do sleep 1; done",
            "until ! pgrep -af x; do :; done",
            "x'; while pgrep -f y",
            "while\npgrep -f x\ndo :",
        ] {
            assert!(legacy(yes), "{yes}");
        }
        for no in ["pgrep -f x", "while pgrep x; do :; done", "while a; do pgrep -f x; done"] {
            assert!(!legacy(no), "{no}");
        }
    }

    #[test]
    fn a_poll_whose_exit_status_decides_the_loop_is_refused() {
        for line in [
            "while :; do pgrep -f x; [ $? -ne 0 ] && break; sleep 1; done",
            "while :; do pgrep -f x\n[ $? -ne 0 ] && break\nsleep 1\ndone",
            "while :; do pgrep -f x; if [ $? -ne 0 ]; then break; fi; sleep 1; done",
            "while :; do pgrep -f x >/dev/null; [ \"$?\" = 1 ] && exit 0; done",
            "while :; do pgrep -f x && sleep 1 || break; done",
            "while :; do pgrep -f x && echo up && sleep 1 || break; done",
            "for i in 1 2 3; do pgrep -f x || break; done",
        ] {
            assert!(denies(line), "{line}");
        }
        for line in [
            "while :; do pgrep -f x; echo $?; sleep 1; done",
            "while :; do pgrep -f x; [ $? -ne 0 ] && echo gone; sleep 1; done",
            "while :; do pgrep -f x && sleep 1; done",
            "while :; do pgrep -f x; sleep 1; done; [ $? -ne 0 ] && break",
            "while :; do pgrep x; [ $? -ne 0 ] && break; done",
        ] {
            assert!(!denies(line), "{line}");
        }
    }
}
