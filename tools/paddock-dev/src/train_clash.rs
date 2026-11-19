//! Train clash warning (report-only, ER-41): a shared integration point that this branch or merge changes and that another
//! open train changes too. On 2026-11-19 train 14 made the ESLint rule `paddock/e2e-ready-reads` report-only while train 15 made it
//! blocking; nobody saw it until train 15 merged main (15 conflicting files, a design decision under time pressure).
//!
//! Run at two points: `./dev worktree add` (what the base carries over `origin/main`, against the other open trains) and
//! `./dev merge check` (what the member changes over the target, against the other open trains). It lists the paths that are
//! shared integration points ([`SHARED`], plus the paths of the mechanisms in `docs/known-copies.toml`, read through that file's
//! own table, ER-42), minus lockfiles, CHANGELOGs and the files a generator of `docs/generators.toml` writes. It never blocks.
//!
//! An open train is an `origin/train/*` branch that `origin/main` does not contain yet. Its change is `git diff <against>...<train>`
//! (what the train did since it left the line of `against`), so a train stacked on the target is judged by what it added beyond
//! the target and a train that is already part of `against` has no change.
//!
//! **Blocking in `merge check`, once green (ER-41).** While [`ENFORCED`] is false the overlaps are warnings. After the change that
//! flips it (it records the first green real run's id at [`ENFORCED`]), an overlap in a merge commit is refused (exit 5) until the
//! merge message has `Train-clash: <path>: <other train>: <how the two designs fit>` for that path and train. The branch cut
//! (`worktree add`) and a branch checked before it is merged only ever warn. One `diff --name-only` per open train, and one `log`
//! per train that overlaps; nothing is read when no changed path is a shared integration point.
use crate::gates::tree::glob_regex;
use crate::generators::registry::{PATH as GENERATORS, Registry};
use regex::Regex;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// The shared integration points, as globs over slash paths (`gates::tree::glob_regex`): the ESLint config and rules, the
/// conformance checks, lists and baselines, each app's CI contract, the CI workflows and actions, the public APIs of `ui` and
/// `crates`, and the rules. The mechanisms of `docs/known-copies.toml` come from that file, not from this list.
pub const SHARED: &[&str] = &[
    "eslint/src/**",
    "conformance/src/**",
    "conformance/ci/**",
    "conformance/baselines/**",
    "**/paddock.ci.json",
    ".gitea/workflows/**",
    ".gitea/actions/**",
    "**/public-api.ts",
    "crates/*/src/lib.rs",
    "docs/rules.md",
];

/// Whether an overlap with no `Train-clash:` trailer in the merge message is refused. The user asked for this on 2026-11-20; per
/// ER-41 it stays false until its first green run on a real train, and the change that flips it records that run's id here:
/// `first green run: <not yet recorded>`.
pub const ENFORCED: bool = false;

/// The trailer that answers one overlap.
pub const TRAILER: &str = "Train-clash";

/// Never a clash: lockfiles and changelogs. (The files a generator writes are excluded through `docs/generators.toml`.)
pub const EXCLUDED: &[&str] = &["**/Cargo.lock", "**/package-lock.json", "**/*.lock", "**/CHANGELOG*"];

/// The register of mechanisms.
const KNOWN_COPIES: &str = "docs/known-copies.toml";

/// Where trains live.
const MAIN: &str = "origin/main";
const TRAINS: &str = "refs/remotes/origin/train/";

/// The most commits named per warning.
const COMMITS_SHOWN: usize = 3;

/// Reads git: the arguments after `git -C <repo>`; a failed command is an error with git's words.
pub trait GitText {
    /// Standard output of git with `args`.
    fn text(&self, args: &[&str]) -> Result<String, String>;
}

impl GitText for crate::worktree::git::Git {
    fn text(&self, args: &[&str]) -> Result<String, String> {
        crate::worktree::git::Git::text(self, args).map_err(|e| e.to_string())
    }
}

impl GitText for crate::merge_check::git::Git<'_> {
    fn text(&self, args: &[&str]) -> Result<String, String> {
        crate::merge_check::git::Git::text(self, args)
    }
}

/// What judges an overlap: the merge message that may answer it, and whether an unanswered one is refused.
#[derive(Debug, Clone, Copy)]
pub struct Judge<'m> {
    /// The merge commit's message.
    pub message: &'m str,
    /// Whether an overlap the message does not answer is an error (see [`ENFORCED`]).
    pub enforced: bool,
}

impl Judge<'_> {
    fn message_answers(&self, c: &Clash) -> bool {
        crate::merge_check::trailers::reason(self.message, TRAILER, &format!("{}: {}", c.path, c.train)).is_some()
    }
}

/// One shared path that this change and another open train both changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clash {
    /// The path.
    pub path: String,
    /// The other train, as `train/<name>`.
    pub train: String,
    /// Its commits that touched the path (short id and subject), newest first.
    pub commits: Vec<String>,
}

/// What the check found.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Every overlap, by path and then train.
    pub clashes: Vec<Clash>,
    /// Why it could not look (empty when it did): the check then says so and passes nothing off as clean (ER-33).
    pub not_measured: Option<String>,
    /// Facts that narrow what was read (a register that could not be read: its paths were not counted).
    pub notices: Vec<String>,
}

impl Outcome {
    /// Whether `merge check` refuses the merge: enforcing, and an overlap the message does not answer.
    pub fn refused(&self, judge: &Judge) -> bool {
        judge.enforced && !self.unanswered(judge.message).is_empty()
    }

    /// The overlaps that the merge message does not answer with `Train-clash: <path>: <train>: <how it fits>` (a reason is needed).
    pub fn unanswered(&self, message: &str) -> Vec<&Clash> {
        self.clashes
            .iter()
            .filter(|c| crate::merge_check::trailers::reason(message, TRAILER, &format!("{}: {}", c.path, c.train)).is_none())
            .collect()
    }

    /// The lines to print: for each overlap a `::warning::` (an answered one is a `::notice::`; one the message must answer and does
    /// not, when `judge` enforces, an `::error::`), then the not-measured and notice lines. `judge` is `None` where nothing is
    /// judged (the branch cut, a branch before its merge).
    pub fn lines(&self, mine: &str, judge: Option<&Judge>) -> Vec<String> {
        let mut out: Vec<String> = self
            .clashes
            .iter()
            .map(|c| {
                let by = if c.commits.is_empty() {
                    String::new()
                } else {
                    format!(" (its commits: {})", c.commits.join("; "))
                };
                let (path, train) = (&c.path, &c.train);
                let need = format!("`{TRAILER}: {path}: {train}: <how the two designs fit>`");
                let read = format!(
                    "read {train}'s change to {path} before merging (`git log origin/main..origin/{train} -- {path}`); record how the two designs fit in the merge message"
                );
                let what = format!("{path} is a shared integration point that {mine} changes and that {train} changes too{by}");
                match judge {
                    Some(j) if j.message_answers(c) => format!("::notice::train clash answered: {what}; the merge message records how they fit."),
                    Some(j) if j.enforced => {
                        format!("::error::train clash: {what}. Next step: {read} as {need}, then check the merge again.")
                    }
                    Some(_) => format!(
                        "::warning::train clash (report-only): {what}. Next step: {read} as {need}; once enforced, the merge is refused without it."
                    ),
                    None => format!(
                        "::warning::train clash (report-only): {what}. Next step: {read}; the trailer is {need}. This warning never blocks."
                    ),
                }
            })
            .collect();
        out.extend(self.notices.iter().map(|n| format!("::notice::train clash check: {n}")));
        if let Some(why) = &self.not_measured {
            out.push(format!(
                "::warning::train clash check NOT MEASURED: {why}. Next step: `git fetch origin` and run it again; this check is report-only."
            ));
        }
        out
    }

    /// The structured result, under `"train-clash"`.
    pub fn json(&self) -> Value {
        let outcome = match (&self.not_measured, self.clashes.is_empty()) {
            (Some(_), _) => "not measured",
            (None, true) => "clean",
            (None, false) => "warning",
        };
        json!({
            "outcome": outcome,
            "why": self.not_measured,
            "overlaps": self.clashes.iter().map(|c| json!({"path": c.path, "train": c.train, "commits": c.commits})).collect::<Vec<_>>(),
        })
    }

    /// Whether there is anything to print.
    pub fn is_quiet(&self) -> bool {
        self.clashes.is_empty() && self.not_measured.is_none() && self.notices.is_empty()
    }
}

/// The globs that make a path a shared integration point, with the registers' paths read from `rev`.
struct Shared {
    shared: Vec<Regex>,
    excluded: Vec<Regex>,
    generated: Option<Registry>,
}

impl Shared {
    fn read(git: &dyn GitText, rev: &str, notices: &mut Vec<String>) -> Shared {
        let mut shared: Vec<Regex> = SHARED.iter().map(|g| glob_regex(g)).collect();
        match git.text(&["show", &format!("{rev}:{KNOWN_COPIES}")]) {
            Ok(text) => match known_copy_paths(&text) {
                Ok(paths) => shared.extend(paths.iter().map(|g| glob_regex(g))),
                Err(why) => notices.push(format!(
                    "{KNOWN_COPIES} could not be read ({why}); the mechanisms' paths were not counted."
                )),
            },
            Err(why) => notices.push(format!(
                "{KNOWN_COPIES} could not be read ({why}); the mechanisms' paths were not counted."
            )),
        }
        let generated = match git.text(&["show", &format!("{rev}:{GENERATORS}")]) {
            Ok(text) => match Registry::parse(&text) {
                Ok(r) => Some(r),
                Err(errs) => {
                    notices.push(format!(
                        "{GENERATORS} is invalid ({}); generated files were not excluded.",
                        errs.join("; ")
                    ));
                    None
                }
            },
            Err(why) => {
                notices.push(format!(
                    "{GENERATORS} could not be read ({why}); generated files were not excluded."
                ));
                None
            }
        };
        Shared {
            shared,
            excluded: EXCLUDED.iter().map(|g| glob_regex(g)).collect(),
            generated,
        }
    }

    fn is_shared(&self, path: &str) -> bool {
        self.shared.iter().any(|g| g.is_match(path))
            && !self.excluded.iter().any(|g| g.is_match(path))
            && !self.generated.as_ref().is_some_and(|r| r.generators.iter().any(|g| g.writes(path)))
    }
}

/// The path globs of every mechanism and copy in the register's text (`[[mechanism]] paths`, `[[mechanism.copy]] paths`).
pub fn known_copy_paths(text: &str) -> Result<Vec<String>, String> {
    let doc: toml::Table = text.parse().map_err(|e: toml::de::Error| e.message().to_string())?;
    let mut out = Vec::new();
    let paths_of = |t: &toml::Table, out: &mut Vec<String>| {
        if let Some(list) = t.get("paths").and_then(toml::Value::as_array) {
            out.extend(list.iter().filter_map(|p| p.as_str().map(str::to_string)));
        }
    };
    for m in doc
        .get("mechanism")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_table)
    {
        paths_of(m, &mut out);
        for c in m
            .get("copy")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(toml::Value::as_table)
        {
            paths_of(c, &mut out);
        }
    }
    Ok(out)
}

/// The open trains: `(train/<name>, tip)` of every `origin/train/*` branch that `origin/main` does not contain.
fn open_trains(git: &dyn GitText) -> Result<Vec<(String, String)>, String> {
    let out = git.text(&[
        "for-each-ref",
        &format!("--no-merged={MAIN}"),
        "--format=%(refname:lstrip=3) %(objectname)",
        TRAINS,
    ])?;
    Ok(out
        .lines()
        .filter_map(|l| l.split_once(' '))
        .map(|(n, sha)| (n.to_string(), sha.to_string()))
        .collect())
}

/// The paths that differ between `a`'s line and `b` (`git diff --name-only a...b`: what `b` did since it left `a`).
fn changed(git: &dyn GitText, a: &str, b: &str) -> Result<Vec<String>, String> {
    let out = git.text(&["diff", "--name-only", "-z", "--no-renames", &format!("{a}...{b}")])?;
    Ok(out.split('\0').filter(|p| !p.is_empty()).map(str::to_string).collect())
}

/// The commits of `train` beyond `against` that touched any of `paths`, per path (short id and subject, newest first).
fn commits_touching(git: &dyn GitText, against: &str, train: &str, paths: &[String]) -> Result<BTreeMap<String, Vec<String>>, String> {
    let range = format!("{against}..origin/{train}");
    let mut args = vec![
        "-c",
        "core.quotepath=off",
        "log",
        "--no-merges",
        "--name-only",
        "--format=%x01%h %s",
        &range,
        "--",
    ];
    args.extend(paths.iter().map(String::as_str));
    let out = git.text(&args)?;
    let mut by_path: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for rec in out.split('\u{1}').filter(|r| !r.trim().is_empty()) {
        let mut lines = rec.lines();
        let head = lines.next().unwrap_or("").trim().to_string();
        for p in lines.map(str::trim).filter(|p| paths.iter().any(|q| q == p)) {
            by_path.entry(p.to_string()).or_default().push(head.clone());
        }
    }
    Ok(by_path)
}

/// The check for `mine` (the paths this branch or merge changes), against what `against` already has: another open train's change
/// is what it did since it left `against`'s line. `rev` is where the registers are read from (the tip being checked).
pub fn check(git: &dyn GitText, mine: &[String], against: &str, rev: &str) -> Outcome {
    let mut outcome = Outcome::default();
    if mine.is_empty() {
        return outcome;
    }
    let shared = Shared::read(git, rev, &mut outcome.notices);
    let mine: Vec<&String> = mine.iter().filter(|p| shared.is_shared(p)).collect();
    if mine.is_empty() {
        outcome.notices.clear();
        return outcome;
    }
    let trains = match open_trains(git) {
        Ok(t) => t,
        Err(why) => {
            outcome.not_measured = Some(format!("the open trains could not be listed ({why})"));
            return outcome;
        }
    };
    for (train, _tip) in trains {
        let theirs = match changed(git, against, &format!("origin/{train}")) {
            Ok(t) => t,
            Err(why) => {
                outcome.not_measured = Some(format!("{train}'s change could not be read ({why})"));
                continue;
            }
        };
        let both: Vec<String> = mine.iter().filter(|p| theirs.contains(p)).map(|p| (*p).clone()).collect();
        if both.is_empty() {
            continue;
        }
        let commits = commits_touching(git, against, &train, &both).unwrap_or_else(|why| {
            outcome.notices.push(format!(
                "{train}'s commits could not be listed ({why}); the warning names the paths only."
            ));
            BTreeMap::new()
        });
        for path in both {
            let list = commits.get(&path).cloned().unwrap_or_default();
            let more = list.len().saturating_sub(COMMITS_SHOWN);
            let mut shown: Vec<String> = list.into_iter().take(COMMITS_SHOWN).collect();
            if more > 0 {
                shown.push(format!("and {more} more"));
            }
            outcome.clashes.push(Clash {
                path,
                train: train.clone(),
                commits: shown,
            });
        }
    }
    outcome.clashes.sort_by(|a, b| (&a.path, &a.train).cmp(&(&b.path, &b.train)));
    outcome
}

/// What a member changes over its target: `git diff target...member`.
pub fn for_merge(git: &dyn GitText, target: &str, member: &str) -> Outcome {
    match changed(git, target, member) {
        Ok(mine) => check(git, &mine, target, member),
        Err(why) => Outcome {
            not_measured: Some(format!("the member's change could not be read ({why})")),
            ..Outcome::default()
        },
    }
}

/// What a branch cut from `base` carries over `origin/main`.
pub fn for_branch_cut(git: &dyn GitText, base: &str) -> Outcome {
    match changed(git, MAIN, base) {
        Ok(mine) => check(git, &mine, base, base),
        Err(why) => Outcome {
            not_measured: Some(format!("what the base carries over {MAIN} could not be read ({why})")),
            ..Outcome::default()
        },
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::worktree::git::Git;

    /// A throw-away repository (never the real one; no `GIT_DIR`, no user config). Refs `origin/main` and `origin/train/*` are made
    /// with `update-ref`: the check reads refs, not a remote.
    struct Scratch {
        dir: tempfile::TempDir,
    }

    impl Scratch {
        fn new() -> Scratch {
            let s = Scratch {
                dir: tempfile::tempdir().unwrap(),
            };
            s.git(&["init", "-q", "-b", "main"]);
            s.git(&["commit", "-q", "--allow-empty", "-m", "chore: empty start"]);
            s
        }

        fn git(&self, args: &[&str]) -> String {
            let empty = if cfg!(windows) { "NUL" } else { "/dev/null" };
            let out = crate::git_env::git()
                .arg("-C")
                .arg(self.dir.path())
                .args(["-c", "commit.gpgsign=false"])
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", empty)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }

        /// Commits `files` on a new branch from `from`; returns its tip.
        fn branch(&self, name: &str, from: &str, message: &str, files: &[(&str, &str)]) -> String {
            self.git(&["checkout", "-q", "-B", name, from]);
            for (p, text) in files {
                let full = self.dir.path().join(p);
                std::fs::create_dir_all(full.parent().unwrap()).unwrap();
                std::fs::write(full, text).unwrap();
            }
            self.git(&["add", "--all"]);
            self.git(&["commit", "-q", "-m", message]);
            self.git(&["rev-parse", "HEAD"])
        }

        fn remote(&self, name: &str, sha: &str) {
            self.git(&["update-ref", &format!("refs/remotes/origin/{name}"), sha]);
        }

        fn reader(&self) -> Git {
            Git::at(self.dir.path())
        }
    }

    /// main has the rule; train/a and the member both change it, a lockfile and a source file.
    fn history() -> (Scratch, String, String) {
        let s = Scratch::new();
        let main = s.branch("main", "HEAD", "chore: start", &[("eslint/src/rule.js", "1\n")]);
        s.remote("main", &main);
        let train = s.branch(
            "t",
            &main,
            "feat(eslint): make e2e-ready-reads blocking",
            &[
                ("eslint/src/rule.js", "blocking\n"),
                ("Cargo.lock", "a\n"),
                ("apps/x/src/a.rs", "a\n"),
            ],
        );
        s.remote("train/a", &train);
        let member = s.branch(
            "m",
            &main,
            "feat(eslint): report e2e-ready-reads",
            &[
                ("eslint/src/rule.js", "report-only\n"),
                ("Cargo.lock", "b\n"),
                ("apps/x/src/a.rs", "b\n"),
            ],
        );
        (s, main, member)
    }

    fn paths(o: &Outcome) -> Vec<(String, String)> {
        o.clashes.iter().map(|c| (c.path.clone(), c.train.clone())).collect()
    }

    #[test]
    fn a_shared_path_that_the_member_and_another_open_train_changed_is_reported_with_that_trains_commits() {
        let (s, main, member) = history();
        let o = for_merge(&s.reader(), &main, &member);
        assert_eq!(o.not_measured, None);
        // The lockfile and the source file are changed by both and are not shared integration points.
        assert_eq!(paths(&o), [("eslint/src/rule.js".to_string(), "train/a".to_string())]);
        assert_eq!(o.clashes[0].commits.len(), 1);
        assert!(
            o.clashes[0].commits[0].contains("make e2e-ready-reads blocking"),
            "{:?}",
            o.clashes[0].commits
        );
        // The scratch repository has no registers: the two notices say so (nothing is silently skipped).
        let all = o.lines("feat/m", None);
        assert_eq!(all.iter().filter(|l| l.starts_with("::notice::")).count(), 2, "{all:?}");
        let lines: Vec<String> = all.into_iter().filter(|l| l.starts_with("::warning::")).collect();
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].starts_with("::warning::")
                && lines[0].contains("train/a")
                && lines[0].contains("read train/a's change to eslint/src/rule.js before merging")
                && lines[0].contains("record how the two designs fit in the merge message"),
            "{}",
            lines[0]
        );
        assert_eq!(o.json()["outcome"], "warning");
    }

    #[test]
    fn a_path_that_is_not_a_shared_integration_point_is_not_reported() {
        let s = Scratch::new();
        let main = s.branch("main", "HEAD", "chore: start", &[("apps/x/src/a.rs", "0\n")]);
        s.remote("main", &main);
        let train = s.branch("t", &main, "feat: a", &[("apps/x/src/a.rs", "a\n"), ("CHANGELOG.md", "a\n")]);
        s.remote("train/a", &train);
        let member = s.branch("m", &main, "feat: m", &[("apps/x/src/a.rs", "b\n"), ("CHANGELOG.md", "b\n")]);
        let o = for_merge(&s.reader(), &main, &member);
        assert!(o.clashes.is_empty() && o.not_measured.is_none(), "{o:?}");
        assert_eq!(o.json()["outcome"], "clean");
    }

    #[test]
    fn a_train_that_is_merged_into_main_is_not_reported() {
        let (s, main, member) = history();
        // Positive control first: it is reported while it is open.
        assert_eq!(for_merge(&s.reader(), &main, &member).clashes.len(), 1);
        // `origin/main` now contains train/a: it is no longer open.
        s.git(&["checkout", "-q", "-B", "landed", &main]);
        s.git(&["merge", "-q", "--no-ff", "-m", "land train a", "t"]);
        let landed = s.git(&["rev-parse", "HEAD"]);
        assert_ne!(main, landed);
        s.remote("main", &landed);
        assert!(for_merge(&s.reader(), &main, &member).clashes.is_empty());
    }

    #[test]
    fn a_train_the_target_already_contains_is_not_another_design() {
        let (s, _main, member) = history();
        // The target is train/a itself: its change is already in the target.
        let target = s.git(&["rev-parse", "t"]);
        let o = for_merge(&s.reader(), &target, &member);
        assert!(o.clashes.is_empty(), "{o:?}");
    }

    #[test]
    fn a_train_stacked_on_the_target_is_judged_by_what_it_adds_beyond_the_target() {
        let (s, main, member) = history();
        let target = s.git(&["rev-parse", "t"]);
        // train/b is stacked on the target and changes another shared file only.
        let b = s.branch("b", &target, "docs: rules", &[("docs/rules.md", "x\n")]);
        s.remote("train/b", &b);
        // The member changes the rule (the target's change, not b's) and the rules.
        let m2 = s.branch("m2", &main, "feat: m2", &[("eslint/src/rule.js", "z\n"), ("docs/rules.md", "y\n")]);
        let o = for_merge(&s.reader(), &target, &m2);
        assert_eq!(paths(&o), [("docs/rules.md".to_string(), "train/b".to_string())]);
        let _ = member;
    }

    #[test]
    fn cutting_a_branch_from_a_train_names_the_other_open_train_that_changes_what_the_base_carries() {
        let s = Scratch::new();
        let main = s.branch("main", "HEAD", "chore: start", &[("docs/rules.md", "0\n")]);
        s.remote("main", &main);
        let a = s.branch("a", &main, "docs: rule 90 in train a", &[("docs/rules.md", "a\n")]);
        s.remote("train/a", &a);
        let b = s.branch("b", &main, "docs: rule 91 in train b", &[("docs/rules.md", "b\n")]);
        s.remote("train/b", &b);
        let o = for_branch_cut(&s.reader(), &b);
        assert_eq!(paths(&o), [("docs/rules.md".to_string(), "train/a".to_string())]);
        assert!(o.lines("the base", None).concat().contains("rule 90 in train a"));
        // A base that is main itself carries nothing over main: nothing to compare.
        assert!(for_branch_cut(&s.reader(), "origin/main").is_quiet());
    }

    #[test]
    fn a_path_of_a_registered_mechanism_is_shared_and_a_file_a_generator_writes_is_not() {
        let s = Scratch::new();
        let register = "version = 1\n[[mechanism]]\nid = \"m\"\ncanonical = \"c\"\npaths = [\"crates/c/src/**\"]\n  [[mechanism.copy]]\n  paths = [\"apps/x/src/copy.rs\"]\n";
        let generators = "version = 1\n[[generator]]\nid = \"g\"\nwhat = \"w\"\ncheck = [\"true\"]\nregenerate = \"none: x\"\ncwd = \".\"\npaths = [\"docs/list.md\"]\noutputs = [\"docs/list.md\"]\ncost = \"fast\"\n";
        let main = s.branch(
            "main",
            "HEAD",
            "chore: start",
            &[
                ("docs/known-copies.toml", register),
                ("docs/generators.toml", generators),
                ("crates/c/src/a.rs", "0\n"),
                ("apps/x/src/copy.rs", "0\n"),
                ("docs/list.md", "0\n"),
            ],
        );
        s.remote("main", &main);
        let files = |t: &'static str| [("crates/c/src/a.rs", t), ("apps/x/src/copy.rs", t), ("docs/list.md", t)];
        let a = s.branch("a", &main, "feat: a", &files("a\n"));
        s.remote("train/a", &a);
        let m = s.branch("m", &main, "feat: m", &files("m\n"));
        let o = for_merge(&s.reader(), &main, &m);
        assert_eq!(
            paths(&o),
            [
                ("apps/x/src/copy.rs".to_string(), "train/a".to_string()),
                ("crates/c/src/a.rs".to_string(), "train/a".to_string())
            ]
        );
    }

    #[test]
    fn without_origin_main_it_says_not_measured_and_never_clean() {
        let s = Scratch::new();
        let tip = s.branch("main", "HEAD", "chore: start", &[("docs/rules.md", "0\n")]);
        let o = for_merge(&s.reader(), "main", &tip);
        // Nothing differs here, so nothing is asked; the base cut from a ref that does not exist is the unmeasured case.
        assert!(o.is_quiet());
        let bad = for_branch_cut(&s.reader(), &tip);
        assert!(bad.not_measured.is_some(), "{bad:?}");
        assert_eq!(bad.json()["outcome"], "not measured");
        assert!(bad.lines("x", None).concat().contains("NOT MEASURED"));
    }

    #[test]
    fn the_real_register_gives_paths_and_a_broken_one_is_an_error() {
        let paths = known_copy_paths(include_str!("../../../docs/known-copies.toml")).unwrap();
        assert!(paths.len() > 10, "the register's paths were read as nothing: {}", paths.len());
        assert!(known_copy_paths("not = [toml").is_err());
    }

    #[test]
    fn every_shared_glob_matches_a_path_it_is_meant_for() {
        for (glob, sample) in [
            ("eslint/src/**", "eslint/src/rules/e2e-ready-reads.js"),
            ("conformance/ci/**", "conformance/ci/x.sh"),
            ("**/paddock.ci.json", "apps/debrief/paddock.ci.json"),
            (".gitea/workflows/**", ".gitea/workflows/ci.yml"),
            ("**/public-api.ts", "ui/projects/paddock-ui/src/public-api.ts"),
            ("crates/*/src/lib.rs", "crates/paddock-ipc/src/lib.rs"),
            ("docs/rules.md", "docs/rules.md"),
        ] {
            assert!(SHARED.contains(&glob), "{glob}");
            assert!(glob_regex(glob).is_match(sample), "{glob} vs {sample}");
        }
        assert!(!glob_regex("crates/*/src/lib.rs").is_match("crates/paddock-ipc/src/other.rs"));
    }

    fn one_overlap() -> Outcome {
        let (s, main, member) = history();
        let o = for_merge(&s.reader(), &main, &member);
        assert_eq!(o.clashes.len(), 1);
        o
    }

    fn errors(lines: &[String]) -> usize {
        lines.iter().filter(|l| l.starts_with("::error::")).count()
    }

    #[test]
    fn enforced_an_overlap_without_the_trailer_is_refused_with_the_line_to_add() {
        let o = one_overlap();
        let message = "Merge feat/m into train\n\nHunks-dropped: x: y\n";
        assert_eq!(o.unanswered(message).len(), 1);
        assert!(o.refused(&Judge { message, enforced: true }));
        let lines = o.lines("feat/m", Some(&Judge { message, enforced: true }));
        assert_eq!(errors(&lines), 1, "{lines:?}");
        assert!(
            lines
                .iter()
                .any(|l| l.contains("Train-clash: eslint/src/rule.js: train/a: <how the two designs fit>")),
            "{lines:?}"
        );
    }

    #[test]
    fn enforced_an_overlap_with_the_trailer_and_a_reason_is_accepted() {
        let o = one_overlap();
        let message = "Merge feat/m\n\nTrain-clash: eslint/src/rule.js: train/a: mine stays report-only behind the flag train/a adds\n";
        assert!(o.unanswered(message).is_empty());
        assert!(!o.refused(&Judge { message, enforced: true }));
        let lines = o.lines("feat/m", Some(&Judge { message, enforced: true }));
        assert_eq!(errors(&lines), 0, "{lines:?}");
        assert!(lines.iter().any(|l| l.starts_with("::notice::train clash answered")), "{lines:?}");
        // A trailer with no reason is no answer.
        let bare = "Train-clash: eslint/src/rule.js: train/a: \n";
        assert_eq!(o.unanswered(bare).len(), 1);
    }

    #[test]
    fn enforced_a_trailer_that_names_another_path_or_train_is_refused() {
        let o = one_overlap();
        for message in [
            "Train-clash: eslint/src/other.js: train/a: fits\n",
            "Train-clash: eslint/src/rule.js: train/b: fits\n",
            "Train-clash: eslint/src/: train/a: fits\n",
        ] {
            assert_eq!(o.unanswered(message).len(), 1, "{message}");
            assert!(o.refused(&Judge { message, enforced: true }), "{message}");
            assert_eq!(errors(&o.lines("m", Some(&Judge { message, enforced: true }))), 1, "{message}");
        }
    }

    #[test]
    fn report_only_and_the_branch_cut_never_produce_an_error() {
        let o = one_overlap();
        let judged = o.lines(
            "m",
            Some(&Judge {
                message: "no trailer",
                enforced: false,
            }),
        );
        assert_eq!(errors(&judged), 0);
        assert!(!o.refused(&Judge {
            message: "no trailer",
            enforced: false
        }));
        assert!(
            judged.iter().any(|l| l.starts_with("::warning::") && l.contains("once enforced")),
            "{judged:?}"
        );
        let cut = o.lines("m", None);
        assert_eq!(errors(&cut), 0);
        assert!(cut.iter().any(|l| l.contains("never blocks")), "{cut:?}");
    }
}
