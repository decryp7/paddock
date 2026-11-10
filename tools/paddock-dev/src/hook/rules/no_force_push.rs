//! no-force-push: shared history is never rewritten.
//!
//! Ported from `.claude/hooks/no-force-push.sh`, whose regexes are the oracle (`oracle_tests`). The shell hook matched the text
//! `git [options] push ... -f|--force|--force-with-lease[=x]|--force-if-includes` and `git push ... +src:dst` with no `;`, `&` or
//! `|` in between; this rule reads the commands: any simple command with a `git` word, global options (an option may take the
//! next word as its value), `push`, and a force flag or a `+refspec` among the rest of its words. The shell hook missed a `+branch`
//! refspec with no colon and combined short flags (`-fu`, `-uf`, `-vf`); both are denied here (closed on purpose, recorded in
//! `oracle_tests::DIFFERENCES`).

use crate::hook::decision::Denial;
use crate::hook::input::ToolCall;
use crate::hook::lex::{Script, Word, basename};
use crate::hook::rules::Rule;

/// Refuses a force push.
pub struct NoForcePush;

const REASON: &str = "Blocked: force pushes are never allowed (shared history is not rewritten). Push a new branch instead (for \
example <branch>-v2) and report it.";

impl Rule for NoForcePush {
    fn id(&self) -> &'static str {
        "no-force-push"
    }

    fn check(&self, _call: &ToolCall, script: Option<&Script>) -> Option<Denial> {
        let forced = script?.commands().any(|cmd| {
            cmd.words
                .iter()
                .enumerate()
                .filter(|(_, w)| basename(&w.text) == "git")
                .any(|(k, _)| force_pushes(&cmd.words[k + 1..]))
        });
        forced.then(|| Denial {
            rule: self.id(),
            reason: REASON.to_string(),
        })
    }
}

/// `words` follow a `git` word: options, `push`, then arguments.
fn force_pushes(words: &[Word]) -> bool {
    let Some(push) = find_push(words) else { return false };
    words[push + 1..]
        .iter()
        .any(|w| is_force_flag(&w.text) || is_force_refspec(&w.text))
}

/// The index of `push` after the global options (`-C dir`, `-c k=v`, `--no-pager`): each option may take the next word as its
/// value, unless that word is `push`.
fn find_push(words: &[Word]) -> Option<usize> {
    let mut k = 0;
    while let Some(word) = words.get(k) {
        let t = word.text.as_str();
        if t == "push" {
            return Some(k);
        }
        if !(t.starts_with('-') && t.len() > 1) {
            return None;
        }
        k += 1;
        if words.get(k).is_some_and(|n| n.text != "push" && !n.text.starts_with('-')) {
            k += 1;
        }
    }
    None
}

/// `-f`, a short cluster with an `f` (`-fu`, `-uf`, `-vf`; `f` is push's only short flag with that letter), or a long force
/// option. Lease-based forces are force pushes.
fn is_force_flag(t: &str) -> bool {
    matches!(t, "--force" | "--force-with-lease" | "--force-if-includes")
        || t.starts_with("--force-with-lease=")
        || t.strip_prefix('-')
            .is_some_and(|c| !c.starts_with('-') && c.chars().all(|x| x.is_ascii_alphanumeric()) && c.contains('f'))
}

/// `+src`, `+src:dst` or `+:dst`: a leading plus forces that ref.
fn is_force_refspec(t: &str) -> bool {
    t.starts_with('+') && t.len() > 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call() -> ToolCall {
        ToolCall::parse(r#"{"tool_name":"Bash","tool_input":{}}"#).unwrap()
    }

    fn denies(cmd: &str) -> bool {
        let script = Script::parse(cmd).unwrap();
        NoForcePush.check(&call(), Some(&script)).is_some()
    }

    #[test]
    fn every_force_flag_is_refused_anywhere_among_the_arguments() {
        for flag in [
            "-f",
            "--force",
            "--force-with-lease",
            "--force-with-lease=main",
            "--force-with-lease=a:b",
            "--force-if-includes",
        ] {
            for line in [
                format!("git push {flag}"),
                format!("git push origin main {flag}"),
                format!("git push {flag} origin"),
            ] {
                assert!(denies(&line), "{line}");
            }
        }
    }

    #[test]
    fn a_plus_refspec_with_a_colon_is_refused() {
        for spec in ["+main:main", "+HEAD:refs/heads/x", "+a.b/c_d-e:f", "+main", "+:x", "+HEAD"] {
            assert!(denies(&format!("git push origin {spec}")), "{spec}");
        }
        for spec in ["main:main", "+", "origin+main:x", ":old"] {
            assert!(!denies(&format!("git push origin {spec}")), "{spec}");
        }
    }

    #[test]
    fn global_options_and_their_values_are_skipped_to_reach_push() {
        for line in [
            "git -C /r push -f",
            "git -c a=b push -f",
            "git --no-pager push --force",
            "git -C /a -c x=y --no-pager push -f",
            "git --git-dir=/x --work-tree=/y push -f",
        ] {
            assert!(denies(line), "{line}");
        }
        for line in ["git -C /r status -f", "git log push -f", "git fetch -f"] {
            assert!(!denies(line), "{line}");
        }
    }

    #[test]
    fn only_the_command_that_pushes_counts() {
        assert!(!denies("git push origin main && git checkout -f main"));
        assert!(!denies("git branch -f x; git push origin x"));
        assert!(denies("git status | cat; git push -f"));
        assert!(denies("git fetch && git push --force-with-lease"));
    }

    #[test]
    fn wrappers_and_nested_shells_are_read() {
        for line in [
            "sudo git push -f",
            "FOO=1 git push -f",
            "env GIT_X=1 git push -f",
            "xargs git push -f",
            "bash -c 'git push -f'",
            "sh -c \"cd x && git push --force\"",
            "eval git push -f",
            "echo $(git push -f)",
            "echo `git push -f`",
            "bash <<EOF\ngit push -f\nEOF",
            "bash -c 'bash -c \"git push -f\"'",
        ] {
            assert!(denies(line), "{line}");
        }
    }

    #[test]
    fn text_that_only_mentions_a_force_push_is_not_one() {
        for line in [
            "git commit -m 'never git push -f'",
            "echo \"git push --force\"",
            "cat <<'EOF'\ngit push -f\nEOF",
            "# git push -f\nls",
            "grep -r 'git push -f' docs",
        ] {
            assert!(!denies(line), "{line}");
        }
    }

    #[test]
    fn the_old_gaps_are_closed_and_other_short_flags_pass() {
        for line in [
            "git push -fu origin x",
            "git push -uf origin x",
            "git push -vf origin x",
            "git push origin +main",
            "git -c a=b push origin +main",
            "git --git-dir=/x push -fu origin x",
            "git -C /r push -uf origin x",
            "git -C /r -c a=b --no-pager push --force-if-includes",
        ] {
            assert!(denies(line), "{line}");
        }
        for line in [
            "git push -u origin x",
            "git push -v",
            "git push -n",
            "git push --follow-tags",
            "git push --no-verify",
            "git push -d origin x",
        ] {
            assert!(!denies(line), "{line}");
        }
    }

    #[test]
    fn it_ignores_calls_without_a_script() {
        assert!(NoForcePush.check(&call(), None).is_none());
    }

    #[test]
    fn the_reason_names_the_cause_and_the_next_step() {
        let script = Script::parse("git push -f").unwrap();
        let d = NoForcePush.check(&call(), Some(&script)).unwrap();
        assert_eq!(d.rule, "no-force-push");
        assert!(d.reason.contains("shared history") && d.reason.contains("new branch"));
    }
}
