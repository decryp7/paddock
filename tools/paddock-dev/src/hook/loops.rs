//! The loop structure of a parsed command, for the rules that judge a loop (ER-32, `no-pgrep-wait`, `no-unbounded-wait`).
//!
//! A rule that asks "is there a bound (or a poll) in this wait?" must ask it of one loop, not of the whole line: a `timeout 2 curl`
//! in a loop's condition bounds one poll, not the wait, and a `for` loop's `done` does not close the `while` around it. The old
//! `pgrep` regex and the first version of `no-unbounded-wait` both read their answer from anywhere in the line; this module is the one
//! place that knows where a loop starts, where its condition ends (at its `do`), where it ends (`done`), which loops enclose a command
//! and which `timeout` command wraps the nested shell a loop sits in.
//!
//! [`loops`] walks [`Script::items`] once. Nested pieces (`$(...)`, backticks, `bash -c`, heredocs fed to a shell) follow the command
//! that holds them between [`Item::Enter`] and [`Item::Leave`]: they start with the loops open at that point, and the loops they open
//! and close stay inside them. Indices in a [`Loop`] are positions in `script.items`, so a rule can look at what follows a command.

use crate::hook::lex::{Item, Script, Simple, basename};

/// Which loop keyword opened it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `while`.
    While,
    /// `until`.
    Until,
    /// `for` or `select`: a bounded list, no condition.
    For,
}

/// One loop of the command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loop {
    /// `while`, `until` or `for`.
    pub kind: Kind,
    /// The commands of its condition (from the keyword to its `do`), as indices into `script.items`; empty for a `for` loop. A command
    /// of a nested piece in the condition (`$(...)`) is in it too.
    pub condition: Vec<usize>,
    /// The commands from its `do` (for a `for` loop, from its own line) to its `done`, including those of the loops and nested pieces
    /// inside it.
    pub body: Vec<usize>,
    /// A `timeout` or `gtimeout` command wraps the nested shell this loop is written in (`timeout 60 bash -c '...'`, or a heredoc to
    /// a shell), so the whole loop ends when the time is up.
    pub timeout_wrapped: bool,
}

impl Loop {
    /// Every command of the loop, condition first.
    pub fn commands(&self) -> impl Iterator<Item = usize> + '_ {
        self.condition.iter().chain(self.body.iter()).copied()
    }
}

/// What is open at one point of the walk.
#[derive(Clone, Default)]
struct Open {
    /// The ids (positions in the result) of the loops open, outermost first, and whether each is still in its condition.
    stack: Vec<(usize, bool)>,
    /// Inside a nested shell started by a `timeout` command.
    wrapped: bool,
}

/// The command names `timeout` runs, before the command: `timeout`, `gtimeout` (and their paths).
fn is_timeout(word: &str) -> bool {
    matches!(basename(word), "timeout" | "gtimeout")
}

/// The command runs `timeout` in command position: after assignments and wrappers (`sudo timeout 5 ...`), not as an argument
/// (`echo timeout`).
fn wraps_with_timeout(cmd: &Simple) -> bool {
    let skipped = cmd.words.len() - cmd.command().len();
    cmd.words[..skipped].iter().any(|w| is_timeout(&w.text))
}

fn keyword_kind(cmd: &Simple) -> Kind {
    for w in &cmd.words {
        match w.text.as_str() {
            "until" => return Kind::Until,
            "while" => return Kind::While,
            _ => {}
        }
    }
    Kind::While
}

/// Every loop of `script`, in the order they open.
pub fn loops(script: &Script) -> Vec<Loop> {
    let mut out: Vec<Loop> = Vec::new();
    let mut now = Open::default();
    let mut saved: Vec<Open> = Vec::new();
    let mut last_cmd: Option<&Simple> = None;
    for (idx, item) in script.items.iter().enumerate() {
        match item {
            Item::Enter => {
                saved.push(now.clone());
                if last_cmd.is_some_and(wraps_with_timeout) {
                    now.wrapped = true;
                }
            }
            Item::Leave => now = saved.pop().unwrap_or_default(),
            Item::Sep(_) => {}
            Item::Cmd(cmd) => {
                last_cmd = Some(cmd);
                if cmd.is_do()
                    && let Some(top) = now.stack.last_mut()
                {
                    top.1 = false;
                }
                if cmd.opens_condition() {
                    out.push(Loop {
                        kind: keyword_kind(cmd),
                        condition: Vec::new(),
                        body: Vec::new(),
                        timeout_wrapped: now.wrapped,
                    });
                    now.stack.push((out.len() - 1, true));
                } else if cmd.opens_for() {
                    out.push(Loop {
                        kind: Kind::For,
                        condition: Vec::new(),
                        body: Vec::new(),
                        timeout_wrapped: now.wrapped,
                    });
                    now.stack.push((out.len() - 1, false));
                }
                let closes = cmd.words.first().is_some_and(|w| w.text == "done");
                if closes {
                    now.stack.pop();
                } else {
                    let innermost = now.stack.len().checked_sub(1);
                    for (depth, (id, in_condition)) in now.stack.iter().enumerate() {
                        if Some(depth) == innermost && *in_condition {
                            out[*id].condition.push(idx);
                        } else {
                            out[*id].body.push(idx);
                        }
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> (Script, Vec<Loop>) {
        let script = Script::parse(src).unwrap();
        let l = loops(&script);
        (script, l)
    }

    fn words(script: &Script, at: &[usize]) -> Vec<String> {
        at.iter()
            .map(|&i| match &script.items[i] {
                Item::Cmd(c) => c.command().first().map(|w| w.text.clone()).unwrap_or_default(),
                _ => String::new(),
            })
            .collect()
    }

    #[test]
    fn a_while_loop_has_a_condition_up_to_do_and_a_body_to_done() {
        let (s, l) = parse("echo a; while test -f x; check y; do sleep 1; echo b; done; echo c");
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].kind, Kind::While);
        assert_eq!(words(&s, &l[0].condition), ["test", "check"]);
        assert_eq!(words(&s, &l[0].body), ["sleep", "echo"]);
        assert!(!l[0].timeout_wrapped);
    }

    #[test]
    fn until_and_for_are_told_apart() {
        let (s, l) = parse("until a; do :; done; for i in 1 2; do b; done");
        assert_eq!([l[0].kind, l[1].kind], [Kind::Until, Kind::For]);
        assert_eq!(words(&s, &l[0].condition), ["a"]);
        assert!(l[1].condition.is_empty());
        assert_eq!(words(&s, &l[1].body), ["for", "b"]);
    }

    #[test]
    fn a_nested_for_loop_does_not_close_the_while_around_it() {
        let (s, l) = parse("while a; do for i in 1; do b; done; sleep 1; done; after");
        assert_eq!(l.len(), 2);
        assert_eq!(words(&s, &l[0].body), ["for", "b", "sleep"]);
        assert_eq!(words(&s, &l[1].body), ["for", "b"]);
    }

    #[test]
    fn a_substitution_in_a_condition_belongs_to_the_condition_and_one_in_the_body_to_the_body() {
        let (s, l) = parse("while poll $(inner); do echo $(step); done");
        assert_eq!(words(&s, &l[0].condition), ["poll", "inner"]);
        assert_eq!(words(&s, &l[0].body), ["echo", "step"]);
    }

    #[test]
    fn loops_of_a_nested_shell_stay_in_it_and_the_commands_of_the_nested_shell_belong_to_the_loops_around() {
        let (s, l) = parse("while a; do bash -c 'for i in 1; do x; done'; y; done; z");
        assert_eq!(l.len(), 2);
        assert_eq!(words(&s, &l[0].body), ["bash", "for", "x", "y"]);
        let (s, l) = parse("bash -c 'while a; do :; done'; pgrep x");
        assert_eq!(words(&s, &l[0].body), [":"]);
    }

    #[test]
    fn a_timeout_wraps_the_loops_of_the_nested_shell_it_runs_and_no_others() {
        for (src, want) in [
            ("timeout 60 bash -c 'until a; do sleep 1; done'", true),
            ("gtimeout -s KILL 60 sh -c 'while a; do :; done'", true),
            ("sudo timeout 5 bash -c 'until a; do :; done'", true),
            ("timeout 60 bash <<EOF\nuntil a; do :; done\nEOF", true),
            ("timeout 60 bash -c 'x'; until a; do :; done", false),
            ("until a; do timeout 5 curl x; done", false),
            ("echo timeout 60 bash -c 'x'; bash -c 'until a; do :; done'", false),
            ("until timeout 2 curl x; do :; done", false),
            ("bash -c 'until a; do :; done'", false),
        ] {
            let (_, l) = parse(src);
            assert_eq!(l.last().map(|x| x.timeout_wrapped), Some(want), "{src}");
        }
    }

    #[test]
    fn a_loop_opened_in_a_wrapped_shell_is_wrapped_and_the_state_ends_with_the_shell() {
        let (_, l) = parse("timeout 5 bash -c 'while a; do :; done'; while b; do :; done");
        assert_eq!([l[0].timeout_wrapped, l[1].timeout_wrapped], [true, false]);
    }

    #[test]
    fn an_unclosed_loop_and_a_stray_done_do_not_panic() {
        let (_, l) = parse("while a; do b");
        assert_eq!(l.len(), 1);
        let (_, l) = parse("done; done; echo x");
        assert!(l.is_empty());
    }
}
