//! no-force-push: shared history is never rewritten.
//!
//! Ported from `.claude/hooks/no-force-push.sh`, whose regexes are the oracle (`oracle_tests`). The shell hook matched the text
//! `git [options] push ... -f|--force|--force-with-lease[=x]|--force-if-includes` and `git push ... +src:dst` with no `;`, `&` or
//! `|` in between; this rule reads the commands: any simple command with a `git` word, global options (an option may take the
//! next word as its value, `-c remote.*.push=+...` counts), `push` or `send-pack`, and a force flag, `--mirror` or a `+refspec`
//! among the rest of its words. The shell hook missed a `+branch`
//! refspec with no colon and combined short flags (`-fu`, `-uf`, `-vf`); both are denied here (closed on purpose, recorded in
//! `oracle_tests::DIFFERENCES`).

use crate::hook::decision::Denial;
use crate::hook::input::ToolCall;
use crate::hook::lex::{Script, Word, basename};
use crate::hook::rules::Rule;
use regex::Regex;
use std::sync::OnceLock;

/// Refuses a force push.
pub struct NoForcePush;

impl NoForcePush {
    fn denial(&self) -> Denial {
        Denial {
            rule: self.id(),
            reason: REASON.to_string(),
        }
    }
}

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
        forced.then(|| self.denial())
    }

    fn legacy(&self, text: &str) -> Option<Denial> {
        legacy_patterns().iter().any(|p| p.is_match(text)).then(|| self.denial())
    }
}

/// `words` follow a `git` word: options, `push` (or `send-pack`), then arguments.
fn force_pushes(words: &[Word]) -> bool {
    let mut config_forces = false;
    let mut k = 0;
    while let Some(word) = words.get(k) {
        let t = word.text.as_str();
        if t == "push" || t == "send-pack" {
            return config_forces || words[k + 1..].iter().any(|w| is_force_flag(&w.text) || is_force_refspec(&w.text));
        }
        if !(t.starts_with('-') && t.len() > 1) {
            return false;
        }
        k += 1;
        // An option may take the next word as its value, unless that word is the subcommand.
        if let Some(value) = words
            .get(k)
            .filter(|n| n.text != "push" && n.text != "send-pack" && !n.text.starts_with('-'))
        {
            if t == "-c" && config_forces_push(&value.text) {
                config_forces = true;
            }
            k += 1;
        }
    }
    false
}

/// `-c remote.<name>.push=+...` and `-c remote.<name>.mirror=true` make a plain `git push` a force. (Git config names are
/// case-insensitive in the section and the variable.)
fn config_forces_push(setting: &str) -> bool {
    let s = setting.to_ascii_lowercase();
    s.starts_with("remote.") && (s.contains(".push=+") || s.contains(".mirror=true"))
}

/// `-f`, a short cluster with an `f` (`-fu`, `-uf`, `-vf`; `f` is push's only short flag with that letter), any `--force...`
/// option (git accepts `--force-w` for `--force-with-lease`), and `--mirror` or an abbreviation of it. Lease-based forces are
/// force pushes.
fn is_force_flag(t: &str) -> bool {
    let Some(long) = t.strip_prefix("--") else {
        return t
            .strip_prefix('-')
            .is_some_and(|c| c.chars().all(|x| x.is_ascii_alphanumeric()) && c.contains('f'));
    };
    let name = long.split('=').next().unwrap_or(long);
    name.starts_with("force") || (!name.is_empty() && "mirror".starts_with(name))
}

/// `+src`, `+src:dst` or `+:dst`: a leading plus forces that ref.
fn is_force_refspec(t: &str) -> bool {
    t.starts_with('+') && t.len() > 1
}

/// The shell hook's patterns, on text.
fn legacy_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            // The flag may end at any shell punctuation, not only a space (the shell hook needed a space: `-f;` and `-f)` passed).
            r#"git([[:space:]]+-[[:alnum:]-]+([[:space:]]+[^[:space:]-][^[:space:]]*)?)*[[:space:]]+push([[:space:]]+[^;&|]*)?[[:space:]](-f|--force|--force-with-lease(=[^[:space:]]*)?|--force-if-includes)([[:space:];&|)'"`]|$)"#,
            r"git[[:space:]]+push[^;&|]*[[:space:]]\+[[:alnum:]_./-]+:",
        ]
        .iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect()
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

    #[test]
    fn long_force_options_may_be_abbreviated_and_mirror_is_refused() {
        for flag in [
            "--force-w",
            "--force-wi",
            "--force-with",
            "--force-i",
            "--force-if-includes",
            "--mirror",
            "--mirro",
            "--mir",
            "--m",
        ] {
            assert!(denies(&format!("git push {flag} origin x")), "{flag}");
        }
        for flag in [
            "--mi",
            "--follow-tags",
            "--fo",
            "--no-verify",
            "--set-upstream",
            "--delete",
            "--dry-run",
            "--atomic",
            "--",
        ] {
            // `--mi` is a prefix of `--mirror` too, so it is refused; the others are other options
            let expected = flag == "--mi";
            assert_eq!(denies(&format!("git push {flag} origin x")), expected, "{flag}");
        }
    }

    #[test]
    fn config_that_turns_a_plain_push_into_a_force_is_refused() {
        for line in [
            "git -c remote.origin.push=+refs/heads/*:refs/heads/* push origin",
            "git -c remote.upstream.push=+HEAD push",
            "git -c Remote.Origin.Push=+x push",
            "git -c remote.origin.mirror=true push origin",
            "git -C /r -c remote.o.push=+a:b --no-pager push",
        ] {
            assert!(denies(line), "{line}");
        }
        for line in [
            "git -c user.name=x push origin main",
            "git -c remote.origin.push=main push",
            "git -c remote.origin.mirror=false push",
            "git -c push.default=simple push",
        ] {
            assert!(!denies(line), "{line}");
        }
    }

    #[test]
    fn send_pack_is_a_push_too() {
        for line in [
            "git send-pack --force origin main",
            "git send-pack -f origin main",
            "git send-pack --mirror origin",
            "git -C /r send-pack origin +main",
        ] {
            assert!(denies(line), "{line}");
        }
        for line in [
            "git send-pack origin main",
            "git send-pack --dry-run origin main",
            "git send-email --force",
        ] {
            assert!(!denies(line), "{line}");
        }
    }

    #[test]
    fn lexer_gaps_that_the_old_text_check_covered_are_covered() {
        for line in [
            "(( y = 1<<2 ))\ngit push -f",
            "echo ${x:-\"}\"}; git push -f",
            "echo $(echo $'\\''); git push -f",
            "echo x | xargs -I{} sh -c 'git push -f {}'",
            "env -S 'git push -f'",
            "cat <<EOF\n$(git push -f)\nEOF",
            "cat <<< \"$(git push -f)\"",
            "git pu\\\nsh -f",
            "'git' \"push\" '-f'",
        ] {
            assert!(denies(line), "{line}");
        }
    }

    #[test]
    fn the_text_fallback_decides_what_the_lexer_cannot_read() {
        let legacy = |t: &str| NoForcePush.legacy(t).is_some();
        for yes in [
            "git push -f",
            "git -C x push --force origin",
            "git push origin +a:b",
            "x'; git push -f",
            "git push --force-with-lease=a:b ",
            "git\npush\n-f",
        ] {
            assert!(legacy(yes), "{yes}");
        }
        for no in [
            "git push",
            "git push origin main",
            "echo force",
            "git push origin main; git branch -f x",
            "git status -f",
        ] {
            assert!(!legacy(no), "{no}");
        }
    }

    #[test]
    fn the_text_fallback_sees_a_flag_followed_by_any_shell_punctuation() {
        let legacy = |t: &str| NoForcePush.legacy(t).is_some();
        for yes in [
            "git push -f;",
            "git push -f)",
            "git push -f'",
            "git push -f\"",
            "git push -f`",
            "git push -f&",
            "git push -f|cat",
            "(git push --force)",
            "x'; git push --force-with-lease;",
        ] {
            assert!(legacy(yes), "{yes}");
        }
        for no in ["git push -fx", "git push -x -fl", "git push origin; git branch -f"] {
            // `-fx` is not a flag the old pattern knew; the lexer path denies it, the text path stays as it was
            assert!(!legacy(no), "{no}");
        }
    }
}
