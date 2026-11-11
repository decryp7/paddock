//! The oracle of the two ported rules: a corpus of commands per rule whose decisions the old shell hooks
//! (`fixtures/no-pgrep-wait.sh`, `fixtures/no-force-push.sh`, verbatim from `.claude/hooks/`) gave. The new rules must decide
//! every command identically. The corpus files hold `allow` or `deny`, a tab and the command (`\n` and `\t` stand for a line end
//! and a tab). The decisions in them are checked against the live shell hooks on every Unix run (the `live_*` tests count what they
//! compared, and fail, never pass silently, when `bash` or `jq` is missing: rule 71 and ER-33, "not measured" is not a pass; the CI
//! image installs `jq`), and against the new rules always. `DIFFERENCES` and `RESIDUAL` are checked against the old hooks the same way.

use super::input::ToolCall;
use super::lex::Script;
use super::rules;
#[cfg(unix)]
use crate::input_hash::{Exec, Real};
use serde_json::json;
#[cfg(unix)]
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
    // Holes the old regexes had and the coordinator closed (2026-11-10): a `+refspec` with no colon, combined short flags,
    // and `pgrep` options after a valued option or in long form.
    ("no-force-push", "allow", "git push origin +main"),
    ("no-force-push", "allow", "git push -fu origin feat"),
    ("no-force-push", "allow", "git push -uf origin feat"),
    ("no-force-push", "allow", "git push -vf origin main"),
    ("no-force-push", "allow", "git push origin main -fx"),
    ("no-force-push", "allow", "git push origin HEAD:refs/heads/x +y"),
    ("no-force-push", "allow", "git -c a=b push origin +main"),
    ("no-force-push", "allow", "git --git-dir=/x push -fu origin x"),
    ("no-force-push", "allow", "git -C /r push -uf origin x"),
    ("no-pgrep-wait", "allow", "while pgrep -u me -f foo; do sleep 1; done"),
    ("no-pgrep-wait", "allow", "while pgrep --full foo; do sleep 1; done"),
    ("no-pgrep-wait", "allow", "while pgrep foo -f; do :; done"),
    ("no-pgrep-wait", "allow", "while pgrep -u me -fl foo; do :; done"),
    // Closed with the review of 2026-11-10 (the old hooks allowed all of these).
    ("no-force-push", "allow", "f() { git push -f; }; f"),
    ("no-force-push", "allow", "git pu\\\nsh -f"),
    ("no-force-push", "allow", "'git' \"push\" '-f'"),
    ("no-force-push", "allow", "g''it pu''sh -''f"),
    (
        "no-force-push",
        "allow",
        "git -c remote.origin.push=+refs/heads/*:refs/heads/* push origin",
    ),
    ("no-force-push", "allow", "git -c remote.origin.mirror=true push origin"),
    ("no-force-push", "allow", "xargs sh -c 'git push -f'"),
    ("no-force-push", "allow", "nice -n 5 bash -c 'git push -f'"),
    ("no-force-push", "allow", "timeout 5 sh -c 'git push -f'"),
    ("no-force-push", "allow", "env -S 'git push -f'"),
    ("no-force-push", "allow", "sudo -u me bash -lc 'git push -f'"),
    ("no-force-push", "allow", "cat <<EOF\n$(git push -f)\nEOF"),
    ("no-force-push", "allow", "cat <<< \"$(git push -f)\""),
    ("no-force-push", "allow", "bash <<<'git push -f'"),
    ("no-force-push", "allow", "git push --force-w origin x"),
    ("no-force-push", "allow", "git push --force-wi origin x"),
    ("no-force-push", "allow", "git push --mirror origin"),
    ("no-force-push", "allow", "git push --mir origin"),
    ("no-force-push", "allow", "git send-pack --force origin main"),
    ("no-force-push", "allow", "git send-pack --mirror origin"),
    ("no-pgrep-wait", "allow", "while true; do pgrep -f x || break; sleep 5; done"),
    (
        "no-pgrep-wait",
        "allow",
        "for i in $(seq 100); do pgrep -f x >/dev/null || break; sleep 1; done",
    ),
    (
        "no-pgrep-wait",
        "allow",
        "until false; do if ! pgrep -f x; then break; fi; sleep 1; done",
    ),
    (
        "no-pgrep-wait",
        "allow",
        "while true; do [ -z \"$(pgrep -f x)\" ] || break; sleep 1; done",
    ),
    ("no-pgrep-wait", "allow", "while pgrep -$'f' x; do sleep 1; done"),
    ("no-pgrep-wait", "allow", "while pgrep -\\f x; do :; done"),
    ("no-pgrep-wait", "allow", "while pkill -0 -f x; do sleep 1; done"),
    ("no-pgrep-wait", "allow", "while pgr\\\nep -f x; do :; done"),
    ("no-pgrep-wait", "allow", "while pgrep --fu x; do :; done"),
    ("no-pgrep-wait", "allow", "while (( 1<<2 )); pgrep -f x; do :; done"),
    ("no-pgrep-wait", "allow", "while echo ${x:-\"}\"}; pgrep -f x; do :; done"),
    (
        "no-pgrep-wait",
        "allow",
        "while $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:) $(:); pgrep -f x; do :; done",
    ),
    // The old regex stops at `;` and reads a loop condition of two commands as one.
    ("no-pgrep-wait", "allow", "while sleep 1; pgrep -f foo; do :; done"),
];

/// Documented residual risk (the module doc of `hook` and `tools/githooks/README.md`): commands the hook reads as harmless because
/// what they run is built from variables, `$IFS`, aliases, functions, an interpreter or look-alike characters. The old hooks
/// allowed them too. Each is checked so a change that starts denying one is a decision, not an accident.
const RESIDUAL: &[(&str, &str)] = &[
    ("no-force-push", "X=-f; git push $X"),
    ("no-force-push", "X=-f && git push \"$X\""),
    ("no-force-push", "G=git; $G push -f"),
    ("no-force-push", "git push ${X:--f}"),
    ("no-force-push", "git -c alias.p='push -f' p"),
    ("no-force-push", "git -c alias.p='!git push -f' p"),
    ("no-force-push", "git${IFS}push${IFS}-f"),
    ("no-force-push", "IFS=,; c=git,push,-f; $c"),
    ("no-force-push", "gіt push -f"),
    ("no-force-push", "git push －f"),
    ("no-force-push", "python3 -c 'import os; os.system(\"git push -f\")'"),
    ("no-force-push", "alias gp='git push -f'; gp"),
    ("no-pgrep-wait", "P='pgrep -f'; while $P x; do sleep 1; done"),
    ("no-pgrep-wait", "w() { pgrep -f x; }; while w; do sleep 1; done"),
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

/// What one ported rule decides for a command: the id of the rule if it refuses. The rule is looked up by id and asked on its own
/// (`check` on the parsed command, `legacy` on text the lexer cannot read, as `decide` does), so another rule's decision, or the order
/// the rules are registered in, never changes a corpus row.
fn new_rule(id: &str, cmd: &str) -> Option<&'static str> {
    let call = ToolCall::parse(&input(cmd)).unwrap();
    let rule = rules::all().into_iter().find(|r| r.id() == id).expect("a registered rule");
    let denial = match Script::parse(cmd) {
        Ok(script) => rule.check(&call, Some(&script)),
        Err(_) => rule.legacy(cmd),
    };
    denial.map(|d| d.rule)
}

fn check_new(corpus: &str, rule: &str) -> usize {
    let rows = rows(corpus);
    for (deny, cmd) in &rows {
        let got = new_rule(rule, cmd);
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
    assert_eq!(ids, ["no-pgrep-wait", "no-force-push", "no-unbounded-wait"]);
}

#[test]
fn the_deliberate_differences_hold() {
    for (rule, old, cmd) in DIFFERENCES {
        let got = new_rule(rule, cmd);
        match *old {
            "deny" => assert_eq!(got, None, "{cmd:?} must now pass"),
            _ => assert_eq!(got, Some(*rule), "{cmd:?} must now be refused"),
        }
    }
}

#[cfg(unix)]
/// Runs an old shell hook on a command and says whether it refused. A missing `bash` or `jq` fails the test (rule 71, ER-33: a
/// check that could not run is "not measured", never a pass; the CI image installs `jq`).
fn old_denies(script: &str, cmd: &str) -> bool {
    let path = format!("{}/src/hook/fixtures/{script}", env!("CARGO_MANIFEST_DIR"));
    if let Err(e) = Real.run("jq", &["--version"], Path::new("."), None) {
        panic!("the live oracle is not measured: jq is missing ({e}); install jq (apt-get install jq, brew install jq)");
    }
    match Real.run("bash", &[&path], Path::new("."), Some(input(cmd).as_bytes())) {
        Ok(out) => !out.trim().is_empty(),
        Err(e) => panic!("the live oracle is not measured: {script} did not run ({e})"),
    }
}

#[cfg(unix)]
fn live(corpus: &str, script: &str) {
    let mut compared = 0;
    for (deny, cmd) in rows(corpus) {
        assert_eq!(
            old_denies(script, &cmd),
            deny,
            "{script} decided {cmd:?} differently from the corpus"
        );
        compared += 1;
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
        assert_eq!(old_denies(script, cmd), *old == "deny", "{cmd:?}");
    }
}

#[test]
fn the_documented_residual_risks_still_pass() {
    for (rule, cmd) in RESIDUAL {
        assert_eq!(
            new_rule(rule, cmd),
            None,
            "{rule}: {cmd:?} is documented as a residual risk; if it is denied now, move it to the corpus"
        );
    }
}

#[cfg(unix)]
#[test]
fn live_the_old_hooks_allowed_the_residual_risks_too() {
    for (rule, cmd) in RESIDUAL {
        let script = if *rule == "no-pgrep-wait" {
            "no-pgrep-wait.sh"
        } else {
            "no-force-push.sh"
        };
        assert!(!old_denies(script, cmd), "{cmd:?}");
    }
}
