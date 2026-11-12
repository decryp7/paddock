//! no-kill-by-name (rule 53): never end a process by a name that was read earlier.
//!
//! Refused: `pkill`, `killall`, `killall5`, `taskkill` with `/IM` or `/FI "IMAGENAME eq ..."` (any case, also `-IM`), PowerShell
//! `Stop-Process -Name`, `kill -Name` and `Get-Process <name> | Stop-Process`, a `kill` whose pids come from a name lookup
//! (`kill $(pgrep x)`, `kill $(pidof x)`, `pgrep x | xargs kill`) and a `kill` of a negative pid (a whole process group, or every
//! process with `-1`). `pkill -P $$` and `pkill -P $BASHPID` (the shell's own children) are allowed; every other `-P` is refused. A name is reusable: it matches
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

/// The shell's own pid: `pkill -P` of it ends only this shell's children.
fn is_own_pid(v: &str) -> bool {
    matches!(v, "$$" | "$BASHPID")
}

/// `pkill -P <parent>` where every parent is the shell's own pid. `-P$$`, `-P $$`, `--parent $$` and `--parent=$$` all count.
fn only_own_children(rest: &[Word]) -> bool {
    let mut seen = false;
    let mut k = 0;
    while let Some(w) = rest.get(k) {
        let t = w.text.as_str();
        let value = if t == "-P" || t == "--parent" {
            k += 1;
            rest.get(k).map(|v| v.text.as_str())
        } else if let Some(v) = t.strip_prefix("--parent=") {
            Some(v)
        } else {
            t.strip_prefix("-P")
        };
        if let Some(v) = value {
            if !is_own_pid(v) {
                return false;
            }
            seen = true;
        }
        k += 1;
    }
    seen
}

/// A `kill` whose targets include a negative pid: after `--` any `-N`; before it a second `-N` (the first is the signal), or a
/// `-$var` after the signal. `kill -9 -1`, `kill -- -123`, `kill -TERM -$pgid`. `-$$` is the
/// script's own process group and is allowed.
fn kills_a_group(args: &[Word]) -> bool {
    let negative =
        |t: &str| t != "-$$" && t.len() > 1 && t.starts_with('-') && (t[1..].chars().all(|c| c.is_ascii_digit()) || t.starts_with("-$"));
    let mut signal_seen = false;
    let mut targets = false;
    let mut k = 0;
    while let Some(w) = args.get(k) {
        let t = w.text.as_str();
        k += 1;
        if targets {
            if negative(t) {
                return true;
            }
        } else if t == "--" {
            targets = true;
        } else if matches!(t, "-s" | "-n" | "-l" | "-L") {
            signal_seen = true;
            k += 1;
        } else if t.starts_with('-') && t.len() > 1 {
            if signal_seen && negative(t) {
                return true;
            }
            signal_seen = true;
        } else {
            targets = true;
        }
    }
    false
}

/// `Get-Process` (or `gps`) naming no process id: by name, or all of them. Fed into `Stop-Process` it ends processes by name.
fn get_process_by_name(cmd: &Simple) -> bool {
    let words = cmd.command();
    let Some(first) = words.first() else { return false };
    matches!(first.text.to_ascii_lowercase().as_str(), "get-process" | "gps")
        && !words[1..].iter().any(|w| {
            let l = w.text.to_ascii_lowercase();
            l.len() > 1 && "-id".starts_with(&l)
        })
}

/// `taskkill /IM name` or `/FI "IMAGENAME eq name"`.
fn taskkill_by_name(args: &[Word]) -> bool {
    args.iter().any(|w| matches!(w.text.to_ascii_lowercase().as_str(), "/im" | "-im"))
        || args.windows(2).any(|p| {
            matches!(p[0].text.to_ascii_lowercase().as_str(), "/fi" | "-fi") && p[1].text.to_ascii_lowercase().contains("imagename")
        })
}

fn name_based(cmd: &Simple, piped_from_lookup: bool, piped_from_get_process: bool) -> bool {
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
        "pkill" => !only_own_children(rest),
        "killall" | "killall5" => true,
        "taskkill" => taskkill_by_name(rest), // conformance: pid-kill — a program name the rule matches; nothing here ends a process
        "kill" => {
            words[at..].iter().any(|w| w.subs.iter().any(|s| sub_looks_up_names(s))) || (xargs && piped_from_lookup) || kills_a_group(rest)
        }
        "powershell" | "pwsh" => rest.iter().any(|w| ps_stop_by_name(&w.text)),
        "stop-process" | "spps" => piped_from_get_process || rest.iter().any(|w| ps_name_param(&w.text)),
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
    static RE: OnceLock<Option<(Regex, Regex)>> = OnceLock::new();
    let Some((named, fed)) = RE
        .get_or_init(|| {
            Some((
                Regex::new(r"(?i)\b(?:stop-process|spps|kill)\b[^;|\n]*\s-n(?:a(?:me?)?)?\b").ok()?,
                Regex::new(r"(?i)\b(?:get-process|gps)\b([^;|\n]*)\|\s*(?:stop-process|spps|kill)\b").ok()?,
            ))
        })
        .as_ref()
    else {
        return false;
    };
    // `Get-Process -Id 12 | Stop-Process` names a pid; with no `-Id` it is by name (or every process).
    let by_id = |args: &str| {
        args.split_whitespace()
            .any(|w| w.len() > 1 && "-id".starts_with(&w.to_ascii_lowercase()))
    };
    named.is_match(t) || fed.captures_iter(t).any(|c| !by_id(c.get(1).map_or("", |m| m.as_str())))
}

impl Rule for NoKillByName {
    fn id(&self) -> &'static str {
        "no-kill-by-name"
    }

    fn check(&self, _call: &ToolCall, script: Option<&Script>) -> Option<Denial> {
        let script = script?;
        let mut prev_lookup = false;
        let mut prev_get_process = false;
        let mut piped = false;
        for item in &script.items {
            match item {
                Item::Cmd(cmd) => {
                    if name_based(cmd, piped && prev_lookup, piped && prev_get_process) {
                        return Some(self.denial());
                    }
                    prev_lookup = cmd.command().first().is_some_and(|w| is_lookup(basename(&w.text)));
                    prev_get_process = get_process_by_name(cmd);
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
                r"(?i)(?:^|[\s;|&(`'\x22/])(?:pkill|killall5?)\b|taskkill[^;&|\n]*\s[/-](?:im\b|fi\s+\S*imagename)|stop-process[^;&|\n]*\s-n(?:a(?:me?)?)?\b",
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
            "pkill -P 1234",
            "pkill -P $PPID",
            "pkill -P ${X}",
            "pkill -P $$ -P 1",
            "pkill --parent $PPID",
            "pkill -f x -P $(cat p)",
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
            "killall5",
            "killall5 -9",
            "sudo killall5",
            "kill -9 -1",
            "kill -- -1",
            "kill -TERM -- -4242",
            "kill -s KILL -1",
            "kill -9 1234 -1",
            "kill -9 -$pgid",
            "kill -- -$$ -1",
            "kill -TERM -$$ -$other",
            "sudo kill -9 -1",
            "bash -c 'kill -- -1'",
            "taskkill /F /FI \"IMAGENAME eq node.exe\"",
            "taskkill /fi 'imagename eq node*' /f",
            "Get-Process node | Stop-Process", // conformance: pid-kill — a command text the rule must refuse or allow; nothing here ends a process
            "Get-Process -Name node | Stop-Process -Force", // conformance: pid-kill — a command text the rule must refuse or allow; nothing here ends a process
            "gps node | spps",
            "Get-Process | Stop-Process", // conformance: pid-kill — a command text the rule must refuse or allow; nothing here ends a process
            "pwsh -c 'Get-Process node | Stop-Process'", // conformance: pid-kill — a command text the rule must refuse or allow; nothing here ends a process
            "powershell -Command \"gps node | kill\"",
            "powershell -c \"kill -Name node\"",
            "pwsh -c 'kill -na node'",
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
            "pkill -P $$",
            "kill -- -$$",
            "kill -TERM -$$",
            "kill -9 -- -$$",
            "pkill -P$$",
            "pkill -P $BASHPID",
            "pkill -9 -P $BASHPID",
            "pkill --parent $$",
            "pkill -P $$ sleep",
            "kill -9 1234 -h",
            "kill -l",
            "kill -s TERM 12",
            "kill -- 1234",
            "taskkill /F /FI \"PID eq 12\"",
            "Get-Process -Id 12 | Stop-Process", // conformance: pid-kill — a command text the rule must refuse or allow; nothing here ends a process
            "gps -Id 12 | spps",
            "pwsh -c 'Get-Process -Id 12 | Stop-Process'", // conformance: pid-kill — a command text the rule must refuse or allow; nothing here ends a process
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
