//! no-unscoped-cleanup (ER-2, ER-14): a cleanup touches only its own items.
//!
//! Refused: `git worktree prune` (it drops the record of every worktree whose folder is missing at that moment, also a live agent's on
//! a drive that is not mounted; a dry run, `-n` or `--dry-run`, is allowed), `docker` or `podman` `system|container|volume|image|
//! network|builder|buildx prune`, and a mass removal of every listed container (`docker rm -f $(docker ps -aq)`, `docker ps -q |
//! xargs docker stop`, `docker volume rm $(docker volume ls -q)`, `docker rmi $(docker images -q)`) unless the listing is narrowed by
//! `-f` or `--filter` (also `--filter=`), and `git worktree remove --force`/`-f` inside a loop or under `xargs` (the 2026-11-10 shape). The next step is named in the refusal: `./dev worktree clean`
//! for worktrees, a container's own name or label for containers. The same patterns in committed code are conformance check C115
//! (this folder is outside its scan, since the rule names the patterns to refuse them). Like the other rules it answers every caller
//! and reads commands, not text; a command the lexer cannot read gets the text check ([`Rule::legacy`]).

use crate::hook::decision::Denial;
use crate::hook::input::ToolCall;
use crate::hook::lex::{Item, Script, Sep, Simple, Word, basename};
use crate::hook::loops::loops;
use crate::hook::rules::Rule;
use regex::Regex;
use std::sync::OnceLock;

/// Refuses `git worktree prune` and the container engines' mass removals.
pub struct NoUnscopedCleanup;

const WORKTREE_REASON: &str = "Blocked: `git worktree prune` drops the record of every worktree whose folder is missing right now, also a \
live agent's (ER-2). Remove your own worktree with `./dev worktree drop <path>` or release it with `./dev worktree release`; the \
coordinator cleans merged ones with `./dev worktree clean` (unlocked, merged, clean, not in use; never force).";
const DOCKER_REASON: &str = "Blocked: this removes containers, images or volumes that are not this task's (ER-14): a `prune`, or every \
listed container. Stop and remove only what you started, by name or label: `docker rm -f <your-container>`, \
`docker ps -q --filter label=<your-label> | xargs docker rm -f`. Containers are created with `--rm` and limits; never restart or \
reconfigure shared Docker or OrbStack.";

const KINDS: &[&str] = &["system", "container", "volume", "image", "network", "builder", "buildx"];
const REMOVERS: &[&str] = &["rm", "rmi", "stop", "kill", "restart"];
const OBJECTS: &[&str] = &["container", "volume", "image", "network"];
/// Engine options that take the next word as their value.
const ENGINE_VALUE_OPTS: &[&str] = &["-H", "--host", "--context", "-c", "--config", "-l", "--log-level"];
/// Git options that take the next word as their value.
const GIT_VALUE_OPTS: &[&str] = &[
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--exec-path",
    "--super-prefix",
    "--config-env",
];

impl NoUnscopedCleanup {
    fn denial(&self, reason: &str) -> Denial {
        Denial {
            rule: self.id(),
            reason: reason.to_string(),
        }
    }
}

/// The words after the `at`-th word, minus leading options (those in `value_opts` take a value; `--opt=value` is one word).
fn after_options<'a>(words: &'a [Word], at: usize, value_opts: &[&str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut k = at + 1;
    let mut options = true;
    while let Some(w) = words.get(k) {
        let t = w.text.as_str();
        if options && t.starts_with('-') && t.len() > 1 {
            k += if value_opts.contains(&t) { 2 } else { 1 };
            continue;
        }
        options = false;
        out.push(t);
        k += 1;
    }
    out
}

fn is_engine(word: &Word) -> bool {
    matches!(basename(&word.text), "docker" | "podman")
}

/// `rm`, `rmi`, `stop`, `kill` or `restart`, alone or after an object word (`container`, `volume`, `image`, `network`).
fn removes_objects(rest: &[&str]) -> bool {
    match rest {
        [kind, sub, ..] if OBJECTS.contains(kind) => REMOVERS.contains(sub),
        [sub, ..] => REMOVERS.contains(sub),
        [] => false,
    }
}

/// `ps`, `ls`, `images` or `list`, alone or after an object word.
fn lists_objects(rest: &[&str]) -> bool {
    match rest {
        [kind, sub, ..] if OBJECTS.contains(kind) => matches!(*sub, "ps" | "ls" | "list"),
        [sub, ..] => matches!(*sub, "ps" | "ls" | "images" | "list"),
        [] => false,
    }
}

/// A listing of every container: `docker ps`/`ls` with no `--filter`.
fn lists_all(cmd: &Simple) -> bool {
    let words = cmd.command();
    words
        .iter()
        .position(is_engine)
        .is_some_and(|at| lists_objects(&after_options(words, at, ENGINE_VALUE_OPTS)) && !words.iter().any(|w| is_filter(&w.text)))
}

/// `-f`, `--filter` and `--filter=...` narrow a listing.
fn is_filter(t: &str) -> bool {
    t == "-f" || t == "--filter" || t.starts_with("--filter=")
}

fn sub_lists_all(src: &str) -> bool {
    static LIST: OnceLock<Option<Regex>> = OnceLock::new();
    LIST.get_or_init(|| Regex::new(r"\b(?:docker|podman)\s+(?:(?:container|volume|image|network)\s+)?(?:ps|ls|images|list)\b").ok())
        .as_ref()
        .is_some_and(|re| re.is_match(src) && !src.split_whitespace().any(is_filter))
}

fn engine_refusal(cmd: &Simple, prev_lists_all: bool, piped: bool) -> bool {
    let words = cmd.command();
    // A pipeline into xargs: the command is `xargs [opts] docker rm ...`.
    let base = words.first().map(|w| basename(&w.text));
    let from = if base == Some("xargs") {
        words.iter().skip(1).position(is_engine).map(|p| p + 1)
    } else {
        base.is_some_and(|b| b == "docker" || b == "podman").then_some(0)
    };
    let Some(at) = from else { return false };
    let rest = after_options(words, at, ENGINE_VALUE_OPTS);
    match rest.as_slice() {
        [kind, "prune", ..] if KINDS.contains(kind) => true,
        _ if removes_objects(&rest) => {
            (base == Some("xargs") && piped && prev_lists_all) || words.iter().any(|w| w.subs.iter().any(|s| sub_lists_all(s)))
        }
        _ => false,
    }
}

fn worktree_prune(cmd: &Simple) -> bool {
    let words = cmd.command();
    if words.first().map(|w| basename(&w.text)) != Some("git") {
        return false;
    }
    let rest = after_options(words, 0, GIT_VALUE_OPTS);
    matches!(rest.as_slice(), ["worktree", "prune", tail @ ..] if !tail.iter().any(|t| *t == "-n" || *t == "--dry-run"))
}

/// `git worktree remove` with `-f` or `--force` (a second force also counts).
fn forced_worktree_remove(cmd: &Simple) -> bool {
    let words = cmd.command();
    let Some(at) = words.iter().position(|w| basename(&w.text) == "git") else {
        return false;
    };
    let rest = after_options(words, at, GIT_VALUE_OPTS);
    matches!(rest.as_slice(), ["worktree", "remove", tail @ ..] if tail.iter().any(|t| *t == "-f" || *t == "--force"))
}

impl Rule for NoUnscopedCleanup {
    fn id(&self) -> &'static str {
        "no-unscoped-cleanup"
    }

    fn check(&self, _call: &ToolCall, script: Option<&Script>) -> Option<Denial> {
        let script = script?;
        // A forced remove inside a loop, or on the other end of a pipe into xargs, removes whatever the list names.
        let in_loop = |at: usize| loops(script).iter().any(|lp| lp.commands().any(|c| c == at));
        for (at, item) in script.items.iter().enumerate() {
            if let Item::Cmd(cmd) = item {
                let xargs = cmd.command().first().is_some_and(|w| basename(&w.text) == "xargs") && forced_worktree_remove(cmd);
                if forced_worktree_remove(cmd) && (xargs || in_loop(at)) {
                    return Some(self.denial(WORKTREE_REASON));
                }
            }
        }
        let mut prev: Option<&Simple> = None;
        let mut piped = false;
        for item in &script.items {
            match item {
                Item::Cmd(cmd) => {
                    if worktree_prune(cmd) {
                        return Some(self.denial(WORKTREE_REASON));
                    }
                    if engine_refusal(cmd, prev.is_some_and(lists_all), piped) {
                        return Some(self.denial(DOCKER_REASON));
                    }
                    prev = Some(cmd);
                    piped = false;
                }
                Item::Sep(s) => piped = *s == Sep::Pipe,
                _ => {}
            }
        }
        None
    }

    fn legacy(&self, text: &str) -> Option<Denial> {
        static PATTERNS: OnceLock<Option<(Regex, Regex, Regex)>> = OnceLock::new();
        let (worktree, docker, dry) = PATTERNS
            .get_or_init(|| {
                Some((
                    // The whole command that holds the prune, so a dry-run flag counts only when it is this command's.
                    Regex::new(r"\bgit\b[^;&|\n]*\bworktree\s+prune\b[^;&|\n]*").ok()?,
                    Regex::new(r"\b(?:docker|podman)\s+(?:system|container|volume|image|network|builder|buildx)\s+prune\b").ok()?,
                    Regex::new(r"(?:^|\s)(?:-n|--dry-run)\b").ok()?,
                ))
            })
            .as_ref()?;
        if worktree.find_iter(text).any(|m| !dry.is_match(m.as_str())) {
            return Some(self.denial(WORKTREE_REASON));
        }
        docker.is_match(text).then(|| self.denial(DOCKER_REASON))
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
        NoUnscopedCleanup.check(&call(), Some(&script))
    }

    #[test]
    fn git_worktree_prune_is_refused_in_every_shape() {
        for line in [
            "git worktree prune",
            "git -C /repo worktree prune",
            "git -c core.x=y worktree prune --expire now",
            "cd /x && git worktree prune",
            "/usr/bin/git worktree prune",
            "bash -c 'git worktree prune'",
            "echo a; git worktree prune; echo b",
            "git --git-dir=/x/.git worktree prune",
        ] {
            let d = verdict(line).unwrap_or_else(|| panic!("allowed: {line}"));
            assert_eq!(d.rule, "no-unscoped-cleanup");
            assert!(d.reason.contains("./dev worktree clean"), "{line}");
        }
    }

    #[test]
    fn other_worktree_commands_and_a_dry_run_are_allowed() {
        for line in [
            "git worktree list",
            "git worktree remove /x/wt",
            "git worktree add -b b /x/wt origin/main",
            "git worktree prune -n",
            "git worktree prune --dry-run",
            "./dev worktree clean",
            "echo git worktree prune",
            "git log --oneline",
        ] {
            assert!(verdict(line).is_none(), "refused: {line}");
        }
    }

    #[test]
    fn container_prunes_and_mass_removals_are_refused() {
        for line in [
            "docker system prune -af",
            "docker container prune -f",
            "docker volume prune",
            "docker image prune -a",
            "docker builder prune",
            "docker -H ssh://h system prune",
            "podman system prune",
            "docker rm -f $(docker ps -aq)",
            "docker stop `docker ps -q`",
            "docker container rm -f $(docker container ls -aq)",
            "docker kill $(docker ps -q)",
            "docker ps -aq | xargs docker rm -f",
            "docker ps -q | xargs -r docker stop",
            "bash -c 'docker system prune -f'",
        ] {
            let d = verdict(line).unwrap_or_else(|| panic!("allowed: {line}"));
            assert!(d.reason.contains("by name or label"), "{line}");
        }
    }

    #[test]
    fn scoped_container_commands_are_allowed() {
        for line in [
            "docker rm -f paddock-test-1",
            "docker stop paddock-a paddock-b",
            "docker rm -f $(docker ps -q --filter label=paddock-own)",
            "docker ps -q --filter name=paddock | xargs docker rm -f",
            "docker run --rm --cpus 4 --memory 12g img",
            "docker ps -a",
            "docker image ls",
            "docker system df",
            "echo docker system prune",
            "xargs rm < list",
        ] {
            assert!(verdict(line).is_none(), "refused: {line}");
        }
    }

    #[test]
    fn a_command_the_lexer_cannot_read_gets_the_text_check() {
        let legacy = |t: &str| NoUnscopedCleanup.legacy(t).is_some();
        assert!(legacy("echo 'x; git worktree prune"));
        assert!(legacy("docker system prune -f # '"));
        assert!(!legacy("git worktree prune --dry-run # '"));
        assert!(!legacy("git worktree list # '"));
    }

    #[test]
    fn decide_refuses_it_end_to_end_with_the_rule_id() {
        let call = ToolCall::parse(r#"{"tool_name":"Bash","tool_input":{"command":"git worktree prune"}}"#).unwrap();
        assert_eq!(crate::hook::decide(&call).denial.map(|d| d.rule), Some("no-unscoped-cleanup"));
        let ok = ToolCall::parse(r#"{"tool_name":"Bash","tool_input":{"command":"git worktree list"}}"#).unwrap();
        assert!(crate::hook::decide(&ok).denial.is_none());
        let unread = ToolCall::parse(r#"{"tool_name":"Bash","tool_input":{"command":"git worktree prune; echo 'x"}}"#).unwrap();
        assert_eq!(crate::hook::decide(&unread).denial.map(|d| d.rule), Some("no-unscoped-cleanup"));
    }

    #[test]
    fn it_agrees_with_the_shared_corpus_that_conformance_check_c115_reads_too() {
        let corpus = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/cleanup-corpus.tsv")).unwrap();
        let mut rows = 0;
        for row in corpus.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let cols: Vec<&str> = row.splitn(3, '\t').collect();
            let [expect, scope, cmd] = cols[..] else { panic!("bad row: {row}") };
            if scope != "both" {
                continue;
            }
            rows += 1;
            assert_eq!(verdict(cmd).is_some(), expect == "deny", "{expect}: {cmd}");
        }
        assert!(rows >= 30, "only {rows} rows read");
    }

    #[test]
    fn a_dry_run_is_allowed_in_the_text_check_too() {
        assert!(NoUnscopedCleanup.legacy("echo 'x; git worktree prune -n").is_none());
        // an `-n` or `--dry-run` of another command is no dry run of the prune
        assert!(NoUnscopedCleanup.legacy("echo 'x; git worktree prune; ls -n").is_some());
        assert!(NoUnscopedCleanup.legacy("git worktree prune; git clean --dry-run # '").is_some());
        assert!(NoUnscopedCleanup.legacy("git worktree prune --dry-run; echo '").is_none());
    }

    #[test]
    fn it_ignores_other_tools() {
        let edit = ToolCall::parse(r#"{"tool_name":"Edit","tool_input":{"file_path":"/x"}}"#).unwrap();
        assert!(NoUnscopedCleanup.check(&edit, None).is_none());
    }
}
