//! no-kill-by-name (rule 53): never end a process by a name that was read earlier.
//!
//! Refused: `pkill`, `killall`, `taskkill` with `/IM` (any case, also `-IM`), PowerShell `Stop-Process -Name`, and a `kill` whose
//! pids come from a name lookup (`kill $(pgrep x)`, `kill $(pidof x)`, `pgrep x | xargs kill`). A name is reusable: it matches
//! other users' processes and other agents' tests (an agent ended its stress-test processes with `pkill` on 2026-11-12). Plain
//! `kill <pid>` with explicit numbers stays allowed: in practice it is the agent's own child. The refusal names the scoped way: the
//! pid you started yourself, by pid plus start time through `./dev` or `paddock_proc`. Like the other rules it answers every caller
//! and reads commands, not text (`bash -c`, `sudo`, `xargs` wrappers are seen through); a command the lexer cannot read gets the
//! text check ([`Rule::legacy`]).

use crate::hook::decision::Denial;
use crate::hook::input::ToolCall;
use crate::hook::lex::{Item, Script, Sep, Simple, Word, basename};
use crate::hook::rules::Rule;
use regex::Regex;
use std::sync::OnceLock;

/// Refuses name-based process kills.
pub struct NoKillByName;

const REASON: &str = "Blocked: don't end processes by name (rule 53): `pkill`, `killall`, `taskkill /IM`, `Stop-Process -Name` or a kill of \
`$(pgrep ...)` can hit another user's or agent's processes, and a name can be reused. Kill only what you started: keep the pid of \
your own child and `kill <pid>` it, or end it by pid plus start time (`./dev` tools, `paddock_proc::ProcessId`, `Supervised`).";

impl NoKillByName {
    fn denial(&self) -> Denial {
        Denial {
            rule: self.id(),
            reason: REASON.to_string(),
        }
    }
}

fn is_lookup(name: &str) -> bool {
    matches!(name, "pgrep" | "pidof")
}

fn sub_looks_up_names(src: &str) -> bool {
    static LOOKUP: OnceLock<Option<Regex>> = OnceLock::new();
    LOOKUP
        .get_or_init(|| Regex::new(r"(?:^|[\s;|&(`])(?:\S*/)?(?:pgrep|pidof)\b").ok())
        .as_ref()
        .is_some_and(|re| re.is_match(src))
}

/// The words of the command that runs: past the `xargs` wrapper and its options when there is one.
fn effective(cmd: &Simple) -> (&[Word], bool) {
    let words = cmd.command();
    if words.first().is_some_and(|w| basename(&w.text) == "xargs") {
        // xargs options that take a value in the next word
        let mut k = 1;
        while let Some(w) = words.get(k) {
            let t = w.text.as_str();
            if !t.starts_with('-') || t.len() < 2 {
                break;
            }
            k += if matches!(t, "-I" | "-n" | "-P" | "-s" | "-L" | "-d" | "-E" | "-a") {
                2
            } else {
                1
            };
        }
        // a wrapper after xargs (`xargs sudo kill`): reuse the skipping on the rest
        return (words.get(k..).unwrap_or(&[]), true);
    }
    (words, false)
}

fn name_based(cmd: &Simple, piped_from_lookup: bool) -> bool {
    let (words, xargs) = effective(cmd);
    // `xargs sudo pkill x`: skip wrappers that follow xargs
    let mut at = 0;
    while words.get(at).is_some_and(|w| {
        matches!(
            basename(&w.text),
            "sudo" | "env" | "nohup" | "nice" | "timeout" | "command" | "exec"
        )
    }) {
        at += 1;
    }
    let Some(first) = words.get(at) else { return false };
    let name = basename(&first.text).to_ascii_lowercase();
    let rest = &words[at + 1..];
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    match name {
        "pkill" | "killall" => true,
        "taskkill" => rest.iter().any(|w| matches!(w.text.to_ascii_lowercase().as_str(), "/im" | "-im")), // conformance: pid-kill — a command text the rule must refuse or allow; nothing here ends a process
        "kill" => words[at..].iter().any(|w| w.subs.iter().any(|s| sub_looks_up_names(s))) || (xargs && piped_from_lookup),
        "powershell" | "pwsh" => rest.iter().any(|w| ps_stop_by_name(&w.text)),
        "stop-process" => rest.iter().any(|w| ps_name_param(&w.text)),
        _ => false,
    }
}

/// `-Name` and its unambiguous prefixes (`-N`, `-Na`...), any case, also `-Name:x`.
fn ps_name_param(t: &str) -> bool {
    let l = t.to_ascii_lowercase();
    let param = l.split(':').next().unwrap_or("");
    param.len() >= 2 && param.starts_with('-') && "-name".starts_with(param)
}

/// A PowerShell `-Command` string that stops by name.
fn ps_stop_by_name(t: &str) -> bool {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)stop-process\b[^;|\n]*\s-n(?:a(?:me?)?)?\b|\bspps\b[^;|\n]*\s-n").ok())
        .as_ref()
        .is_some_and(|re| re.is_match(t))
}

impl Rule for NoKillByName {
    fn id(&self) -> &'static str {
        "no-kill-by-name"
    }

    fn check(&self, _call: &ToolCall, script: Option<&Script>) -> Option<Denial> {
        let script = script?;
        let mut prev_lookup = false;
        let mut piped = false;
        for item in &script.items {
            match item {
                Item::Cmd(cmd) => {
                    if name_based(cmd, piped && prev_lookup) {
                        return Some(self.denial());
                    }
                    prev_lookup = cmd.command().first().is_some_and(|w| is_lookup(basename(&w.text)));
                    piped = false;
                }
                Item::Sep(s) => piped = *s == Sep::Pipe,
                _ => {}
            }
        }
        None
    }

    fn legacy(&self, text: &str) -> Option<Denial> {
        static RE: OnceLock<Option<Regex>> = OnceLock::new();
        RE.get_or_init(|| {
            Regex::new(
                r"(?i)(?:^|[\s;|&(`'\x22/])(?:pkill|killall)\b|taskkill[^;&|\n]*\s[/-]im\b|stop-process[^;&|\n]*\s-n(?:a(?:me?)?)?\b",
            )
            .ok()
        })
        .as_ref()
        .is_some_and(|re| re.is_match(text))
        .then(|| self.denial())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call() -> ToolCall {
        ToolCall::parse(r#"{"tool_name":"Bash","tool_input":{}}"#).unwrap()
    }

    fn verdict(cmd: &str) -> Option<Denial> {
        let script = Script::parse(cmd).unwrap();
        NoKillByName.check(&call(), Some(&script))
    }

    #[test]
    fn name_based_kills_are_refused_in_every_shape() {
        for line in [
            "pkill stress-test",
            "pkill -f 'cargo test'",
            "pkill -P $$",
            "killall node",
            "/usr/bin/pkill -9 foo",
            "sudo pkill foo",
            "sudo -u me killall foo",
            "bash -c 'pkill foo'",
            "sh -c \"echo x; killall foo\"",
            "cd /x && pkill foo",
            "echo a; pkill foo; echo b",
            "FOO=1 pkill foo",
            "timeout 5 pkill foo",
            "echo foo | xargs pkill",
            "echo foo | xargs -n1 sudo killall",
            "taskkill /F /IM node.exe",
            "taskkill /im node.exe /f",
            "taskkill.exe -IM node.exe",
            "Stop-Process -Name node", // conformance: pid-kill — a command text the rule must refuse or allow; nothing here ends a process
            "Stop-Process -Name node -Force", // conformance: pid-kill — a command text the rule must refuse or allow; nothing here ends a process
            "powershell -Command \"Stop-Process -Name node\"",
            "pwsh -c 'Get-Process x | Stop-Process; Stop-Process -Name y'", // conformance: pid-kill — a command text the rule must refuse or allow; nothing here ends a process
            "kill $(pgrep stress)",
            "kill -9 `pidof stress`",
            "pgrep stress | xargs kill",
            "pidof stress | xargs -r kill -9",
        ] {
            let d = verdict(line).unwrap_or_else(|| panic!("allowed: {line}"));
            assert_eq!(d.rule, "no-kill-by-name");
            assert!(d.reason.contains("pid plus start time"), "{line}");
        }
    }

    #[test]
    fn scoped_kills_and_mentions_are_allowed() {
        for line in [
            "kill 1234",
            "kill -9 1234 5678",
            "kill -TERM $child",
            "kill $!",
            "kill $(cat my.pid)",
            "sudo kill 42",
            "taskkill /PID 1234 /F",
            "taskkill /F /PID 12",
            "Stop-Process -Id 12", // conformance: pid-kill — a command text the rule must refuse or allow; nothing here ends a process
            "powershell -Command \"Stop-Process -Id 12\"",
            "pgrep -l stress",
            "pgrep stress | wc -l",
            "echo pkill foo",
            "grep -n pkill notes.md",
            "git log --grep killall",
            "./dev wait --pid 12 --max 5m",
            "xargs kill < pids.txt",
        ] {
            assert!(verdict(line).is_none(), "refused: {line}");
        }
    }

    #[test]
    fn a_command_the_lexer_cannot_read_gets_the_text_check() {
        let legacy = |t: &str| NoKillByName.legacy(t).is_some();
        assert!(legacy("echo 'x; pkill foo"));
        assert!(legacy("killall node # '"));
        assert!(legacy("taskkill /F /IM node.exe # '"));
        assert!(legacy("Stop-Process -Name x # '")); // conformance: pid-kill — a command text the rule must refuse or allow; nothing here ends a process
        assert!(!legacy("kill 12 # '"));
        assert!(!legacy("taskkill /PID 12 # '"));
        assert!(!legacy("echo spkill # '"));
    }

    #[test]
    fn decide_refuses_it_end_to_end_with_the_rule_id() {
        let call = ToolCall::parse(r#"{"tool_name":"Bash","tool_input":{"command":"pkill stress"}}"#).unwrap();
        assert_eq!(crate::hook::decide(&call).denial.map(|d| d.rule), Some("no-kill-by-name"));
        let ok = ToolCall::parse(r#"{"tool_name":"Bash","tool_input":{"command":"kill 123"}}"#).unwrap();
        assert!(crate::hook::decide(&ok).denial.is_none());
        let unread = ToolCall::parse(r#"{"tool_name":"Bash","tool_input":{"command":"pkill x; echo 'x"}}"#).unwrap();
        assert_eq!(crate::hook::decide(&unread).denial.map(|d| d.rule), Some("no-kill-by-name"));
    }

    #[test]
    fn it_ignores_other_tools() {
        let edit = ToolCall::parse(r#"{"tool_name":"Edit","tool_input":{"file_path":"/x"}}"#).unwrap();
        assert!(NoKillByName.check(&edit, None).is_none());
    }
}
