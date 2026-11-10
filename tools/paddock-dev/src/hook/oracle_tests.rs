//! The oracle of the two ported rules: a corpus of commands per rule whose decisions the old shell hooks
//! (`fixtures/no-pgrep-wait.sh`, `fixtures/no-force-push.sh`, verbatim from `.claude/hooks/`) gave. The new rules must decide
//! every command identically. The corpus files hold `allow` or `deny`, a tab and the command (`\n` and `\t` stand for a line end
//! and a tab). The decisions in them are checked against the live shell hooks whenever `bash` and `jq` exist (the `live_*` tests
//! count what they compared and fail when it is zero, rule 71), and against the new rules always.

use super::input::ToolCall;
use super::{decide, rules};
use crate::input_hash::{Exec, Real};
use serde_json::json;
use std::path::Path;

const PGREP: &str = include_str!("fixtures/corpus-no-pgrep-wait.tsv");
const PUSH: &str = include_str!("fixtures/corpus-no-force-push.tsv");

/// Commands the old regexes decided wrongly or not at all and the new rules decide on what the command means. Every one is a
/// deliberate difference: (rule, what the old hook decided, command). The new decision is the opposite.
const DIFFERENCES: &[(&str, &str, &str)] = &[
    // The old regex reads quoted text, comments and heredoc bodies as commands.
    ("no-force-push", "deny", "git commit -m 'revert: git push -f is refused' && ls"),
    ("no-force-push", "deny", "cat <<'EOF'\ngit push --force origin main \nEOF"),
    ("no-pgrep-wait", "deny", "echo 'while pgrep -f foo; do sleep 1; done'"),
    ("no-pgrep-wait", "deny", "# while pgrep -f foo; do sleep 1; done\nls"),
    // The old regex needs a space after the flag.
    ("no-force-push", "allow", "git push origin main -f;"),
    ("no-force-push", "allow", "{ git push -f; }"),
    ("no-force-push", "allow", "(git push --force-with-lease)"),
    ("no-force-push", "allow", "bash -c 'git push -f'"),
    ("no-force-push", "allow", "eval \"git push --force\""),
    ("no-force-push", "allow", "echo $(git push -f)"),
    // A line end ends a command (the old regex read it as a space).
    ("no-force-push", "deny", "git push\n-f"),
    // A long option glued to its value and a quoted flag are still the flag.
    ("no-force-push", "allow", "git --work-tree=/w push -f"),
    ("no-force-push", "allow", "git push origin main '-f'"),
    // The old regex stops at `;` and reads a loop condition of two commands as one.
    ("no-pgrep-wait", "allow", "while sleep 1; pgrep -f foo; do :; done"),
];

fn rows(corpus: &str) -> Vec<(bool, String)> {
    corpus
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (decision, cmd) = l.split_once('\t').expect("a tab between decision and command");
            assert!(decision == "allow" || decision == "deny", "{l}");
            (decision == "deny", cmd.replace("\\n", "\n").replace("\\t", "\t"))
        })
        .collect()
}

fn input(cmd: &str) -> String {
    json!({"tool_name": "Bash", "tool_input": {"command": cmd}}).to_string()
}

/// The new rules' decision for a command: the id of the rule that refuses, if any.
fn new_rule(cmd: &str) -> Option<&'static str> {
    let call = ToolCall::parse(&input(cmd)).unwrap();
    decide(&call).unwrap().map(|d| d.rule)
}

fn check_new(corpus: &str, rule: &str) -> usize {
    let rows = rows(corpus);
    for (deny, cmd) in &rows {
        let got = new_rule(cmd);
        let want = deny.then_some(rule);
        assert_eq!(got, want, "{rule}: {cmd:?}");
    }
    rows.len()
}

#[test]
fn the_corpora_are_big_enough_and_have_both_decisions() {
    for (name, corpus) in [("no-pgrep-wait", PGREP), ("no-force-push", PUSH)] {
        let r = rows(corpus);
        assert!(r.len() >= 40, "{name}: {} commands", r.len());
        assert!(r.iter().filter(|(d, _)| *d).count() >= 15, "{name}: too few denials");
        assert!(r.iter().filter(|(d, _)| !*d).count() >= 15, "{name}: too few allowed commands");
    }
}

#[test]
fn no_pgrep_wait_decides_the_corpus_as_the_shell_hook_did() {
    assert!(check_new(PGREP, "no-pgrep-wait") >= 40);
}

#[test]
fn no_force_push_decides_the_corpus_as_the_shell_hook_did() {
    assert!(check_new(PUSH, "no-force-push") >= 40);
}

#[test]
fn the_rules_are_registered_in_this_order() {
    let ids: Vec<&str> = rules::all().iter().map(|r| r.id()).collect();
    assert_eq!(ids, ["no-pgrep-wait", "no-force-push"]);
}

#[test]
fn the_deliberate_differences_hold() {
    for (rule, old, cmd) in DIFFERENCES {
        let got = new_rule(cmd);
        match *old {
            "deny" => assert_eq!(got, None, "{cmd:?} must now pass"),
            _ => assert_eq!(got, Some(*rule), "{cmd:?} must now be refused"),
        }
    }
}

/// Runs an old shell hook on a command and says whether it refused. `None` when `bash` or `jq` is missing.
fn old_denies(script: &str, cmd: &str) -> Option<bool> {
    let path = format!("{}/src/hook/fixtures/{script}", env!("CARGO_MANIFEST_DIR"));
    Real.run("jq", &["--version"], Path::new("."), None).ok()?;
    let out = Real.run("bash", &[&path], Path::new("."), Some(input(cmd).as_bytes())).ok()?;
    Some(!out.trim().is_empty())
}

fn live(corpus: &str, script: &str) {
    let mut compared = 0;
    for (deny, cmd) in rows(corpus) {
        match old_denies(script, &cmd) {
            Some(old) => {
                assert_eq!(old, deny, "{script} decided {cmd:?} differently from the corpus");
                compared += 1;
            }
            None => return eprintln!("{script}: bash or jq is missing, the live oracle did not run"),
        }
    }
    assert!(compared >= 40, "{script}: only {compared} commands compared");
    eprintln!("{script}: {compared} commands compared with the live shell hook");
}

#[cfg(unix)]
#[test]
fn live_no_pgrep_wait_agrees_with_the_corpus() {
    live(PGREP, "no-pgrep-wait.sh");
}

#[cfg(unix)]
#[test]
fn live_no_force_push_agrees_with_the_corpus() {
    live(PUSH, "no-force-push.sh");
}

#[cfg(unix)]
#[test]
fn live_the_old_hooks_decide_the_differences_as_recorded() {
    for (rule, old, cmd) in DIFFERENCES {
        let script = if *rule == "no-pgrep-wait" {
            "no-pgrep-wait.sh"
        } else {
            "no-force-push.sh"
        };
        let Some(denies) = old_denies(script, cmd) else { return };
        assert_eq!(denies, *old == "deny", "{cmd:?}");
    }
}
