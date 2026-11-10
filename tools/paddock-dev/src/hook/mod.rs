//! `dev hook pre-tool-use` and `dev hooks install` (ADR 0083 §2, ADR 0022, plan RT-4).
//!
//! Claude Code runs one command per guarded tool call and passes the call as JSON on standard input. This module is that
//! command: one process, every [`rules`] rule, one decision. The decision is Claude Code's documented one: nothing on standard
//! output allows, a `hookSpecificOutput` object with `permissionDecision: "deny"` refuses and gives the reason to the agent.
//!
//! Fail-open (ADR 0083 D7): input that is not JSON, or has no `tool_input`, or a command the [`lex`] cannot read, is allowed;
//! one line on standard error names the cause (never the command text) and the exit code stays 0. Every refusal also writes one
//! line to standard error naming the rule (ER-27).
//!
//! A command the [`lex`] cannot read (an unclosed quote) or that passes one of its limits (1 MiB, 16 nested levels, 10,000 nested
//! pieces) is not allowed blindly: every rule runs the old shell hook's text pattern on the raw text first
//! ([`rules::Rule::legacy`]), so what the shell hooks refused is still refused; only harmless unreadable text passes.
//!
//! # Residual risk (ADR 0083 D7: the guard reads commands, it is not a shell)
//!
//! Still allowed, because what runs is built at run time: variables and `$IFS` that make up a command (`X=-f; git push $X`,
//! `git${IFS}push`), aliases and functions defined in the same line and called there, `git -c alias.x=...` that expands to a push,
//! an interpreter (`python3 -c 'os.system(...)'`), and look-alike characters (`gіt`). `oracle_tests::RESIDUAL` lists them so that a
//! change that starts denying one is a decision. Quoting, line continuations, `$(...)`, backticks, `bash -c` and `eval` (after any
//! command word: `xargs sh -c`, `find -exec`, `env -S`), heredocs and here-strings fed to a shell, and expansions inside unquoted
//! heredoc bodies are read.
//!
//! The binary that runs here is an installed copy ([`install`]); the shim `tools/githooks/claude-pre-tool-use` never goes through
//! the `./dev` wrapper, which costs 150 ms or more per call.
//!
//! # Structure
//!
//! [`input`] parses the call, [`lex`] reads a Bash command, [`rules`] holds one file per rule behind the [`rules::Rule`] trait
//! (a new rule is a new file and one line in [`rules::all`]), [`decision`] prints the answer, [`install`] puts the binary and the
//! shim in place.

pub mod decision;
pub mod input;
pub mod install;
pub mod lex;
pub mod rules;

#[cfg(test)]
mod oracle_tests;

use decision::Denial;
use input::ToolCall;
use std::io::{Read, Write};

/// The most standard input read: a tool call larger than this is not read further and passes with a log line.
pub const MAX_INPUT: u64 = 8 << 20;

/// What deciding one tool call gave.
#[derive(Debug)]
pub struct Outcome {
    /// The refusal, if a rule refused.
    pub denial: Option<Denial>,
    /// Why the command could not be read, when it could not: the rules then ran the old shell hooks' text patterns on it.
    pub unread: Option<lex::LexError>,
}

/// Decides one tool call: the first rule that refuses wins. A command the lexer cannot read (an unclosed quote, a limit) gets the
/// shell hooks' own text patterns before it is allowed, so what they refused is still refused.
pub fn decide(call: &ToolCall) -> Outcome {
    let command = match (call.tool_name.as_str(), call.command.as_deref()) {
        ("Bash", Some(cmd)) => cmd,
        _ => {
            return Outcome {
                denial: rules::all().iter().find_map(|r| r.check(call, None)),
                unread: None,
            };
        }
    };
    match lex::Script::parse(command) {
        Ok(script) => Outcome {
            denial: rules::all().iter().find_map(|r| r.check(call, Some(&script))),
            unread: None,
        },
        Err(why) => Outcome {
            denial: rules::all().iter().find_map(|r| r.legacy(command)),
            unread: Some(why),
        },
    }
}

/// `dev hook pre-tool-use`: reads the call from `stdin`, writes the decision to `out` and log lines to `err`. Returns the exit
/// code, always 0: a refusal is the JSON, not the code (exit 2 would make Claude Code treat it as a blocking error).
pub fn pre_tool_use(stdin: &mut dyn Read, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut text = String::new();
    let mut raw = Vec::new();
    if let Err(e) = stdin.take(MAX_INPUT + 1).read_to_end(&mut raw) {
        let _ = writeln!(err, "paddock-dev hook: allowed, standard input could not be read ({})", e.kind());
        return 0;
    }
    if raw.len() as u64 > MAX_INPUT {
        let _ = writeln!(err, "paddock-dev hook: allowed, the input is larger than {MAX_INPUT} bytes");
        return 0;
    }
    text.push_str(&String::from_utf8_lossy(&raw));
    let call = match ToolCall::parse(&text) {
        Ok(c) => c,
        Err(why) => {
            let _ = writeln!(err, "paddock-dev hook: allowed, malformed input ({why})");
            return 0;
        }
    };
    let outcome = decide(&call);
    match (outcome.denial, outcome.unread) {
        (None, None) => {}
        (None, Some(why)) => {
            let _ = writeln!(err, "paddock-dev hook: allowed, the command could not be read ({why})");
        }
        (Some(denial), unread) => {
            let how = match unread {
                Some(why) => format!(", by the text check because the command could not be read ({why})"),
                None => String::new(),
            };
            let _ = writeln!(err, "paddock-dev hook: denied by {} ({}){how}", denial.rule, call.tool_name);
            let _ = writeln!(out, "{}", denial.json());
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(stdin: &[u8]) -> (i32, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = pre_tool_use(&mut &stdin[..], &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    #[test]
    fn malformed_input_is_allowed_with_one_log_line_that_does_not_echo_it() {
        for bad in [
            "",
            "garbage SECRET-TEXT",
            "[1]",
            "{}",
            r#"{"tool_name":"Bash"}"#,
            r#"{"tool_name":"Bash","tool_input":5}"#,
        ] {
            let (code, out, err) = run(bad.as_bytes());
            assert_eq!((code, out.as_str()), (0, ""), "{bad}");
            assert_eq!(err.lines().count(), 1, "{bad}");
            assert!(err.starts_with("paddock-dev hook: allowed, malformed input"), "{err}");
            assert!(!err.contains("SECRET"), "{err}");
        }
    }

    #[test]
    fn invalid_utf8_input_is_allowed() {
        let (code, out, err) = run(&[0xff, 0xfe, b'{']);
        assert_eq!((code, out.as_str()), (0, ""));
        assert!(err.contains("malformed input"));
    }

    #[test]
    fn input_over_the_limit_is_allowed_with_a_log_line() {
        let big = vec![b' '; MAX_INPUT as usize + 1];
        let (code, out, err) = run(&big);
        assert_eq!((code, out.as_str()), (0, ""));
        assert!(err.contains("larger than"), "{err}");
    }

    #[test]
    fn a_failing_stdin_is_allowed() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("boom"))
            }
        }
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(pre_tool_use(&mut Broken, &mut out, &mut err), 0);
        assert!(out.is_empty());
        assert!(String::from_utf8(err).unwrap().contains("could not be read"));
    }

    #[test]
    fn a_command_that_cannot_be_read_is_allowed_with_a_log_line() {
        let (code, out, err) = run(br#"{"tool_name":"Bash","tool_input":{"command":"echo 'unclosed"}}"#);
        assert_eq!((code, out.as_str()), (0, ""));
        assert!(err.contains("could not be read") && !err.contains("echo"), "{err}");
    }

    #[test]
    fn other_tools_and_a_bash_call_without_a_command_are_allowed_silently() {
        for call in [
            r#"{"tool_name":"Edit","tool_input":{"file_path":"/x"}}"#,
            r#"{"tool_name":"mcp__serena__serena_repl","tool_input":{}}"#,
            r#"{"tool_name":"Bash","tool_input":{}}"#,
        ] {
            assert_eq!(run(call.as_bytes()), (0, String::new(), String::new()), "{call}");
        }
    }

    fn denies_in(cmd: &str) -> (i32, String, String) {
        let input = serde_json::json!({"tool_name": "Bash", "tool_input": {"command": cmd}}).to_string();
        run(input.as_bytes())
    }

    #[test]
    fn a_command_the_lexer_cannot_read_is_checked_as_text_before_it_is_allowed() {
        let (code, out, err) = denies_in("echo 'unclosed; git push -f");
        assert_eq!(code, 0);
        assert!(out.contains("\"permissionDecision\":\"deny\""), "{out}");
        assert!(
            err.contains("denied by no-force-push") && err.contains("text check") && !err.contains("echo"),
            "{err}"
        );
        let (_, out, err) = denies_in("echo 'unclosed; while pgrep -f x; do sleep 1; done");
        assert!(out.contains("no-pgrep-wait") || out.contains("pgrep -f"), "{out}");
        assert!(err.contains("denied by no-pgrep-wait"));
        // harmless unreadable text is still allowed, with the log line
        let (code, out, err) = denies_in("echo 'unclosed; git push origin main");
        assert_eq!((code, out.as_str()), (0, ""));
        assert!(err.contains("could not be read"), "{err}");
    }

    #[test]
    fn exhausting_a_limit_falls_back_to_the_text_check_too() {
        let many = vec!["$(:)"; 10_001].join(" ");
        let (_, out, err) = denies_in(&format!("echo {many}; git push -f"));
        assert!(out.contains("deny") && err.contains("text check"), "{err}");
        let big = format!("{}; git push -f", "x".repeat(lex::MAX_SOURCE + 10));
        let (_, out, err) = denies_in(&big);
        assert!(out.contains("deny") && err.contains("text check"), "{err}");
        let (_, out, _) = denies_in(&format!("{}; git push origin main", "x".repeat(lex::MAX_SOURCE + 10)));
        assert_eq!(out, "");
    }
}
