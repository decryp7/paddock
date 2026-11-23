//! no-plain-cargo-test (rule 89; ER-32, ER-51): every test run goes through the bounded runner.
//!
//! A plain `cargo test` has no per-test bound and no progress watchdog: on 2026-11-22/23 `worktree::list_clean` hung 3.5 h and
//! `ci_isolation` 18 min under `cargo test --test it`, and the slot holders those tests started were left running. Every Cargo
//! workspace of the repository has a `.config/nextest.toml` whose default profile stops a test that runs past its bound, so the way
//! to run tests is `cargo nextest run`. It also runs every test in a process of its own, so one test's process-wide change (a working
//! folder, an environment variable) never reaches another; under `cargo test` a test that moved its process into a temporary folder made
//! every later `git` start of the run read the whole temp folder (the 3.5 h hang).
//!
//! Refused: a simple command with a `cargo` word followed (after cargo's own options and a `+toolchain`) by `test` or its alias
//! `t`. Allowed: `cargo test --doc` (nextest does not run doctests; the push gate runs them the same way) and `cargo test --no-run`
//! (it builds and runs nothing). Any `cargo` word counts, so a wrapper (`timeout`, `env`, `xvfb-run`, `nice`) does not hide it; a
//! quoted `cargo` word is an argument (a commit message, a grep pattern), not a command, and is skipped. A command the lexer cannot
//! read gets the text check ([`Rule::legacy`]).

use crate::hook::decision::Denial;
use crate::hook::input::ToolCall;
use crate::hook::lex::{Script, Word, basename};
use crate::hook::rules::Rule;
use regex::Regex;
use std::sync::OnceLock;

/// Refuses a plain `cargo test`.
pub struct NoPlainCargoTest;

const REASON: &str = "Blocked: plain `cargo test` has no per-test bound and no progress watchdog (rule 89; a hung test ran 3.5 h). \
Run the tests through the bounded runner: `cargo nextest run` in the workspace (its .config/nextest.toml stops a test past its \
bound and runs each test in its own process; the same filters: `cargo nextest run --test it worktree::`). Doctests stay \
`cargo test --doc`.";

/// cargo's options that take the next word as their value when written apart (`-C dir`, `--config k=v`, `-Z flag`, `--color auto`).
const VALUED: &[&str] = &["-C", "--config", "-Z", "--color"];

impl NoPlainCargoTest {
    fn denial(&self) -> Denial {
        Denial {
            rule: self.id(),
            reason: REASON.to_string(),
        }
    }
}

impl Rule for NoPlainCargoTest {
    fn id(&self) -> &'static str {
        "no-plain-cargo-test"
    }

    fn check(&self, _call: &ToolCall, script: Option<&Script>) -> Option<Denial> {
        plain_cargo_test(script?).then(|| self.denial())
    }

    fn legacy(&self, text: &str) -> Option<Denial> {
        plain_cargo_test_text(text).then(|| self.denial())
    }
}

/// Whether a parsed command runs a plain `cargo test` (see the module): the one reading the hook and the CI check share (ER-22).
pub fn plain_cargo_test(script: &Script) -> bool {
    script.commands().any(|cmd| {
        cmd.words
            .iter()
            .enumerate()
            .filter(|(_, w)| !w.quoted && basename(&w.text) == "cargo")
            .any(|(k, _)| runs_plain_tests(&cmd.words[k + 1..]))
    })
}

/// Whether a command line runs a plain `cargo test`: read by the lexer, or by the text check when the lexer cannot read it. The CI
/// model's lines are checked with this (`ci_run::tests::no_ci_line_runs_a_plain_cargo_test`).
pub fn runs_plain_cargo_test(command: &str) -> bool {
    match Script::parse(command) {
        Ok(script) => plain_cargo_test(&script),
        Err(_) => plain_cargo_test_text(command),
    }
}

/// The text check of a command the lexer cannot read: `--doc` and `--no-run` anywhere after it let it through, as the reader does.
fn plain_cargo_test_text(text: &str) -> bool {
    static PATTERN: OnceLock<Option<Regex>> = OnceLock::new();
    let Some(p) = PATTERN
        .get_or_init(|| Regex::new(r"(^|[\s;&|(`/])cargo(\s+(\+\S+|-\S+))*\s+(test|t)(\s|$|[;&|)])").ok())
        .as_ref()
    else {
        return false;
    };
    p.find(text).is_some_and(|hit| {
        let rest = &text[hit.start()..];
        !rest.contains("--doc") && !rest.contains("--no-run")
    })
}

/// `words` follow a `cargo` word: a `+toolchain`, cargo's own options, then the subcommand and its arguments.
fn runs_plain_tests(words: &[Word]) -> bool {
    let mut k = 0;
    while let Some(word) = words.get(k) {
        let t = word.text.as_str();
        if t == "test" || t == "t" {
            // Up to a `--`: what follows it goes to the test binary (`-- --doc` is a test filter, not cargo's flag).
            let own = words[k + 1..].iter().take_while(|w| w.text != "--");
            return !own.into_iter().any(|w| w.text == "--doc" || w.text == "--no-run");
        }
        if !(t.starts_with('+') || (t.starts_with('-') && t.len() > 1)) {
            // Another subcommand (`build`, `nextest`, `clippy`): not a test run.
            return false;
        }
        k += 1;
        if VALUED.contains(&t) {
            k += 1;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call() -> ToolCall {
        ToolCall::parse(r#"{"tool_name":"Bash","tool_input":{}}"#).unwrap()
    }

    fn denies(cmd: &str) -> bool {
        let script = Script::parse(cmd).unwrap();
        NoPlainCargoTest.check(&call(), Some(&script)).is_some()
    }

    #[test]
    fn a_plain_cargo_test_is_refused_however_it_is_written() {
        for line in [
            "cargo test",
            "cargo test --test it worktree::list_clean",
            "cd tools/paddock-dev && cargo test --test it",
            "cargo t -p paddock-proc",
            "cargo +1.92.0 test",
            "cargo -C crates test --workspace",
            "cargo --config build.target-dir='/x' test",
            "cargo -Z unstable-options test",
            "CARGO_BUILD_JOBS=2 cargo test -p x",
            "timeout 600 cargo test",
            "env RUST_LOG=debug cargo test",
            "xvfb-run -a cargo test -p paddock-dialogs",
            "/Users/x/.cargo/bin/cargo test",
            "bash -c 'cargo test'",
            "x=$(cargo test 2>&1)",
            "cargo build && cargo test",
            "cargo test -- --nocapture",
            // after `--` the words go to the test binary: a filter named --doc is not cargo's flag
            "cargo test -- --doc",
            "cargo test --release -p debrief-store-duck --test bench -- --nocapture",
        ] {
            assert!(denies(line), "{line}");
        }
    }

    #[test]
    fn nextest_doctests_builds_and_other_cargo_commands_are_allowed() {
        for line in [
            "cargo nextest run",
            "cargo nextest run --test it worktree::",
            "cargo test --doc",
            "cargo test --doc -p paddock-testkit",
            "cargo test --no-run --all-features -p paddock-proc",
            "cargo build --tests",
            "cargo clippy --all-targets",
            "cargo --version",
            "git commit -m 'fix: cargo test hung'",
            "grep -rn 'cargo test' docs",
            "echo \"run cargo test\"",
            "cargo nextest run -- test",
        ] {
            assert!(!denies(line), "{line}");
        }
    }

    #[test]
    fn a_command_the_lexer_cannot_read_gets_the_text_check() {
        let legacy = |t: &str| NoPlainCargoTest.legacy(t).is_some();
        assert!(legacy("cargo test --test it; echo 'x"));
        assert!(legacy("cd x && cargo +stable test # '"));
        assert!(!legacy("cargo nextest run; echo 'x"));
        assert!(!legacy("cargo test --doc; echo 'x"));
        assert!(!legacy("echo 'x; cargo build"));
    }

    #[test]
    fn decide_refuses_it_end_to_end_and_names_the_bounded_runner() {
        let call = ToolCall::parse(r#"{"tool_name":"Bash","tool_input":{"command":"cargo test --test it"}}"#).unwrap();
        let d = crate::hook::decide(&call).denial.unwrap();
        assert_eq!(d.rule, "no-plain-cargo-test");
        assert!(d.reason.contains("cargo nextest run") && d.reason.contains("own process") && d.reason.contains("--doc"));
        let ok = ToolCall::parse(r#"{"tool_name":"Bash","tool_input":{"command":"cargo nextest run --test it"}}"#).unwrap();
        assert!(crate::hook::decide(&ok).denial.is_none());
        let edit = ToolCall::parse(r#"{"tool_name":"Edit","tool_input":{"file_path":"/x"}}"#).unwrap();
        assert!(NoPlainCargoTest.check(&edit, None).is_none());
    }
}
