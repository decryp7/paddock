//! `ci dispatch <workflow> --trial <branch>` (ADR 0098 WP-4, D2 (b)): the branch's one CI run on the combined tree. The branch head
//! is merged `--no-ff` onto the open train's head (origin/main when no train is open) with git's own three-way merge, in the
//! tool's own clone with hooks off and no checkout (`merge-tree` and `commit-tree`: the files of the branch are never written,
//! and nothing in them runs). A conflict is reported with its files and nothing is dispatched; the merge check runs in process;
//! the result is pushed to `trial/<branch>` and the caller dispatches that ref with `base` = the target's sha, so the affected
//! plan is the branch's own change. It replaces the branch's one run (rule 50 (a)), so it adds none. The queue's git setup
//! ([`GitTrain`]) is reused; `Train::build` is not (it needs a ledger pull request and writes a marker commit).

use crate::queue::real::{Child, GitTrain};
use std::path::Path;

/// What the trial merge gave.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The merge is pushed to `trial_ref`; dispatch it with `base` = `target_sha`.
    Ready {
        /// The ref the trial merge is pushed to (`trial/<branch>`).
        trial_ref: String,
        /// What the branch was merged onto (`origin/train/x` or `origin/main`).
        target: String,
        /// The commit of the target.
        target_sha: String,
        /// The trial merge commit.
        head: String,
    },
    /// Git's merge conflicts: the member that did not merge, the files, the target. Nothing was pushed.
    Conflict {
        /// The member whose merge conflicted (the branch, with `--trial`).
        member: String,
        /// What the members were to be merged onto.
        target: String,
        /// The conflicting files.
        files: Vec<String>,
    },
    /// Members that are not on origin (a local branch is not a member of a trial): none was merged and nothing was pushed.
    NotOnOrigin(Vec<String>),
    /// The merge check found something in the combined tree: its text. Nothing was pushed.
    Findings(String),
}

/// A branch name the tool will put in a ref and a command line: letters, digits, `.`, `_`, `-` and `/`, no `..`, no leading `-` or `/`.
pub fn valid_branch(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['-', '/'])
        && !name.contains("..")
        && !name.ends_with(['/', '.'])
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-/".contains(c))
}

/// The folder a trial's scratch folders live in (ER-14: a data drive, never inside the home folder; one place for `ci dispatch
/// --trial`, `ci trial-prune` and the member gate of `merge check`, rule 55). In this order: `PADDOCK_TRIAL_DIR`, git config
/// `paddock.trialRoot`, then `trials` beside the worktree root (git config `paddock.worktreeRoot`, else `PADDOCK_WORKTREE_ROOT`).
/// Whichever gives the folder, it must be absolute, without `..`, and not inside `home` (ER-14). `Err` names the cause and the next
/// step.
pub fn trial_root(
    env: &dyn Fn(&str) -> Option<String>,
    config: &dyn Fn(&str) -> Option<String>,
    home: Option<&Path>,
) -> Result<std::path::PathBuf, String> {
    use std::path::PathBuf;
    let nonempty = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let (path, from) = if let Some(v) = nonempty(env("PADDOCK_TRIAL_DIR")) {
        (PathBuf::from(v), "PADDOCK_TRIAL_DIR".to_string())
    } else if let Some(v) = nonempty(config("paddock.trialRoot")) {
        (PathBuf::from(v), "git config paddock.trialRoot".to_string())
    } else if let Some(w) = nonempty(config("paddock.worktreeRoot")).or_else(|| nonempty(env("PADDOCK_WORKTREE_ROOT"))) {
        let parent = PathBuf::from(&w).parent().map(Path::to_path_buf).filter(|p| p.parent().is_some());
        match parent {
            Some(p) => (p.join("trials"), format!("`trials` beside the worktree root {w}")),
            None => {
                return Err(format!(
                    "the worktree root {w} has no parent folder to put `trials` in. Next step: run `git config paddock.trialRoot <folder on a data drive>` once, or set PADDOCK_TRIAL_DIR"
                ));
            }
        }
    } else {
        return Err("no trial folder is set: PADDOCK_TRIAL_DIR is empty, git config `paddock.trialRoot` and `paddock.worktreeRoot` are not set. Next step: run `git config paddock.trialRoot <folder on a data drive, not under your home>` once in the main checkout".into());
    };
    if !path.is_absolute() || path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(format!(
            "the trial folder {} ({from}) is not an absolute folder without `..`. Next step: set `git config paddock.trialRoot` (or PADDOCK_TRIAL_DIR) to one",
            path.display()
        ));
    }
    if home.is_some_and(|h| path.starts_with(h)) {
        return Err(format!(
            "the trial folder {} ({from}) is inside your home folder (ER-14). Next step: set `git config paddock.trialRoot` (or PADDOCK_TRIAL_DIR) to a folder on a data drive",
            path.display()
        ));
    }
    Ok(path)
}

/// [`trial_root`] with the process environment, the repository's git config (read from `repo`) and the home folder.
pub fn trial_root_real(repo: &Path) -> Result<std::path::PathBuf, String> {
    let git_config = |key: &str| {
        crate::worktree::git::Repo::discover(repo)
            .ok()
            .and_then(|r| r.git().text(&["config", "--get", key]).ok())
    };
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from);
    trial_root(&|k| std::env::var(k).ok(), &git_config, home.as_deref())
}

fn git(g: &GitTrain, kind: Child, args: &[&str]) -> Result<String, String> {
    let (ok, out) = g.run_git(kind, args)?;
    if ok {
        Ok(out)
    } else {
        Err(format!("git {} failed: {}", args.first().unwrap_or(&""), out.trim()))
    }
}

/// The open train: the newest `origin/train/*` whose tip is not on main yet (a landed train is an ancestor of main); `None` when
/// there is none.
pub fn open_train(g: &GitTrain) -> Result<Option<String>, String> {
    let listed = git(
        g,
        Child::Local,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(objectname) %(refname:short)",
            "refs/remotes/origin/train/",
        ],
    )?;
    let trains: Vec<(&str, &str)> = listed.lines().filter_map(|l| l.trim().split_once(' ')).collect();
    // One question for all trains (ER-20): the tips that are not reachable from main, in the order given (newest first).
    let tips: Vec<&str> = trains.iter().map(|(sha, _)| *sha).collect();
    let open = not_on_main(g, &tips)?;
    Ok(open
        .first()
        .and_then(|sha| trains.iter().find(|(s, _)| s == sha))
        .map(|(_, name)| name.to_string()))
}

/// The commits of `shas` that are not reachable from `origin/main`, in the order given: one `git rev-list`, not a `merge-base` per
/// commit (a git start costs about 45 ms on the Windows runner, ER-20).
fn not_on_main(g: &GitTrain, shas: &[&str]) -> Result<Vec<String>, String> {
    if shas.is_empty() {
        return Ok(Vec::new());
    }
    let mut args = vec!["rev-list", "--no-walk=unsorted"];
    args.extend(shas);
    args.extend(["--not", "origin/main"]);
    Ok(git(g, Child::Local, &args)?.lines().map(|l| l.trim().to_string()).collect())
}

/// The `::error::` text of an outcome that is not [`Outcome::Ready`] (`None` for a ready one): which member, why, and the next step.
/// A conflict names the member that did not merge and the members merged before it.
pub fn refusal(outcome: &Outcome, members: &[String]) -> Option<String> {
    match outcome {
        Outcome::Ready { .. } => None,
        Outcome::Conflict { member, target, files } => {
            let before: Vec<&str> = members.iter().map(String::as_str).take_while(|m| m != member).collect();
            let after = if before.is_empty() {
                String::new()
            } else {
                format!(" (after merging {})", before.join(", "))
            };
            Some(format!(
                "{member} conflicts with {target}{after} in {} file(s): {}. Next step: send {member} back to its author to merge {target} and resolve them; nothing was pushed or dispatched",
                files.len(),
                files.join(", ")
            ))
        }
        Outcome::NotOnOrigin(missing) => Some(format!(
            "not on origin: {}. Next step: push the branch(es) so they are proven heads (a local-only branch is not a member of a trial); nothing was merged or dispatched",
            missing.join(", ")
        )),
        Outcome::Findings(text) => Some(format!(
            "the merge check found something in the merged members; nothing was dispatched: {text}"
        )),
    }
}

/// The most members one trial composes (a train has 3 to 4; ER-9).
pub const MAX_MEMBERS: usize = 10;

/// Fetches, merges and pushes one branch: [`prepare_members`] with that branch as the only member (its trial ref is `trial/<branch>`).
pub fn prepare(
    g: &GitTrain,
    branch: &str,
    onto: Option<&str>,
    check: &dyn Fn(&Path, &str) -> Result<(), String>,
) -> Result<Outcome, String> {
    prepare_members(g, &[branch.to_string()], onto, check)
}

/// Fetches, merges and pushes. `check` is the merge check on the trial commit (the clone is its root); it returns the findings'
/// text as an error. `onto` names the target (`origin/train/x`) instead of the open train.
///
/// The members are merged in the order given, each with git's own three-way merge (merge commits, never a rebase, so a member's own
/// commits are the commits of the trial), in the tool's clone with no checkout. Every member must already be on origin: a branch
/// that is only local is refused before anything is merged, so a trial never runs an unproven head. The first merge that conflicts
/// stops the trial and names its member. One member's trial ref is `trial/<branch>`; several members' is `trial/members-<head12>`.
pub fn prepare_members(
    g: &GitTrain,
    members: &[String],
    onto: Option<&str>,
    check: &dyn Fn(&Path, &str) -> Result<(), String>,
) -> Result<Outcome, String> {
    if members.is_empty() || members.len() > MAX_MEMBERS {
        return Err(format!("a trial takes 1 to {MAX_MEMBERS} members, not {}", members.len()));
    }
    for (i, m) in members.iter().enumerate() {
        if !valid_branch(m) {
            return Err(format!(
                "`{m}` is not a branch name this command accepts (letters, digits, . _ - and /)"
            ));
        }
        if members[..i].contains(m) {
            return Err(format!("`{m}` is named twice: a member is merged once"));
        }
    }
    // Every member must be on origin (one question for all of them), before anything is fetched or merged.
    let mut ask: Vec<String> = ["ls-remote", "--heads", "origin"].map(String::from).to_vec();
    ask.extend(members.iter().map(|m| format!("refs/heads/{m}")));
    let listed = git(g, Child::Authed, &ask.iter().map(String::as_str).collect::<Vec<_>>())?;
    let on_origin: Vec<&str> = listed
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1))
        .filter_map(|r| r.strip_prefix("refs/heads/"))
        .collect();
    let missing: Vec<String> = members.iter().filter(|m| !on_origin.contains(&m.as_str())).cloned().collect();
    if !missing.is_empty() {
        return Ok(Outcome::NotOnOrigin(missing));
    }
    let mut fetch: Vec<String> = [
        "fetch",
        "--quiet",
        "--no-tags",
        "origin",
        "+refs/heads/main:refs/remotes/origin/main",
        "+refs/heads/train/*:refs/remotes/origin/train/*",
    ]
    .map(String::from)
    .to_vec();
    fetch.extend(members.iter().map(|m| format!("+refs/heads/{m}:refs/remotes/origin/{m}")));
    // A target that is neither main nor a train (an experiment branch) is fetched too: the kept clone only has the refs named here.
    if let Some(name) = onto.and_then(|t| t.strip_prefix("origin/"))
        && !members.iter().any(|m| m == name)
        && name != "main"
        && !name.starts_with("train/")
    {
        if !valid_branch(name) {
            return Err(format!(
                "--onto `{}` is not a branch name this command accepts (letters, digits, . _ - and /)",
                onto.unwrap_or("")
            ));
        }
        fetch.push(format!("+refs/heads/{name}:refs/remotes/origin/{name}"));
    }
    git(g, Child::Authed, &fetch.iter().map(String::as_str).collect::<Vec<_>>())?;
    let target = match onto {
        Some(t) => t.to_string(),
        None => open_train(g)?.unwrap_or_else(|| "origin/main".to_string()),
    };
    let target_sha = git(g, Child::Local, &["rev-parse", "--verify", &format!("{target}^{{commit}}")])?
        .trim()
        .to_string();
    let single_ref = (members.len() == 1).then(|| format!("trial/{}", members[0]));
    let mut head = target_sha.clone();
    for m in members {
        let sha = git(
            g,
            Child::Local,
            &["rev-parse", "--verify", &format!("refs/remotes/origin/{m}^{{commit}}")],
        )?
        .trim()
        .to_string();
        // Git's own merge, in memory: exit 1 is a conflict and lists the files.
        let (clean, out) = g.run_git(
            Child::Local,
            &["merge-tree", "--write-tree", "--name-only", "--no-messages", &head, &sha],
        )?;
        if !clean {
            let files: Vec<String> = out
                .lines()
                .skip(1)
                .map(str::trim)
                .take_while(|l| !l.is_empty())
                .map(str::to_string)
                .collect();
            if files.is_empty() {
                return Err(format!("git merge-tree failed for {m}: {}", out.trim()));
            }
            // A branch that is sent back keeps no trial ref from an earlier, cleaner trial.
            if let Some(r) = &single_ref {
                delete(g, r)?;
            }
            return Ok(Outcome::Conflict {
                member: m.clone(),
                target,
                files,
            });
        }
        let tree = out.lines().next().unwrap_or("").trim().to_string();
        let message = format!("Merge {m} ({}) into trial of {target}\n", sha.get(..9).unwrap_or(&sha));
        head = git(g, Child::Local, &["commit-tree", &tree, "-p", &head, "-p", &sha, "-m", &message])?
            .trim()
            .to_string();
    }
    let trial_ref = single_ref
        .clone()
        .unwrap_or_else(|| format!("trial/members-{}", head.get(..12).unwrap_or(&head)));
    if let Err(findings) = check(&g.root, &head) {
        if let Some(r) = &single_ref {
            delete(g, r)?;
        }
        return Ok(Outcome::Findings(findings));
    }
    // A scratch ref of this command alone: the next trial of the branch replaces it, so the push is forced.
    git(
        g,
        Child::Authed,
        &["push", "--quiet", "--force", "origin", &format!("{head}:refs/heads/{trial_ref}")],
    )?;
    Ok(Outcome::Ready {
        trial_ref,
        target,
        target_sha,
        head,
    })
}

/// Whether `trial/<branch>` exists on origin.
fn trial_exists(g: &GitTrain, trial_ref: &str) -> Result<bool, String> {
    let out = git(
        g,
        Child::Authed,
        &["ls-remote", "--heads", "origin", &format!("refs/heads/{trial_ref}")],
    )?;
    Ok(!out.trim().is_empty())
}

/// Deletes `trial_ref` on origin when it is there (a trial that was sent back or never dispatched leaves nothing behind).
pub fn delete(g: &GitTrain, trial_ref: &str) -> Result<bool, String> {
    if !trial_ref.starts_with("trial/") || !trial_exists(g, trial_ref)? {
        return Ok(false);
    }
    git(
        g,
        Child::Authed,
        &["push", "--quiet", "origin", "--delete", &format!("refs/heads/{trial_ref}")],
    )?;
    Ok(true)
}

/// A `trial/*` ref that is no longer needed, and why.
#[derive(Debug, PartialEq, Eq)]
pub struct Stale {
    /// The ref on origin (`trial/feat/a`).
    pub trial_ref: String,
    /// Why it can go.
    pub why: String,
}

/// The trial refs of branches that are gone or whose trialed head is on main now (landed). `dry` lists them without deleting.
pub fn prune(g: &GitTrain, dry: bool) -> Result<Vec<Stale>, String> {
    git(
        g,
        Child::Authed,
        &[
            "fetch",
            "--quiet",
            "--no-tags",
            "origin",
            "+refs/heads/main:refs/remotes/origin/main",
            "+refs/heads/trial/*:refs/remotes/origin/trial/*",
        ],
    )?;
    let heads = git(g, Child::Authed, &["ls-remote", "--heads", "origin"])?;
    let branches: std::collections::BTreeSet<&str> = heads
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1))
        .filter_map(|r| r.strip_prefix("refs/heads/"))
        .collect();
    // The head the trial merged is its second parent: on main, the branch landed. Parents come with the listing (`%(parent)`).
    let listed = git(
        g,
        Child::Local,
        &["for-each-ref", "--format=%(refname:short) %(parent)", "refs/remotes/origin/trial/"],
    )?;
    let mut stale = Vec::new();
    let mut live: Vec<(String, String, String)> = Vec::new();
    for line in listed.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let mut words = line.split_whitespace();
        let Some(short) = words.next() else { continue };
        let Some(trial_ref) = short.strip_prefix("origin/") else { continue };
        let branch = trial_ref.trim_start_matches("trial/");
        if !branches.contains(branch) {
            stale.push(Stale {
                trial_ref: trial_ref.to_string(),
                why: format!("branch {branch} is gone"),
            });
        } else if let Some(second) = words.nth(1) {
            live.push((trial_ref.to_string(), branch.to_string(), second.to_string()));
        }
    }
    let heads: Vec<&str> = live.iter().map(|(_, _, sha)| sha.as_str()).collect();
    let open = not_on_main(g, &heads)?;
    for (trial_ref, branch, sha) in &live {
        if !open.contains(sha) {
            stale.push(Stale {
                trial_ref: trial_ref.clone(),
                why: format!("branch {branch} landed on main"),
            });
        }
    }
    stale.sort_by(|a, b| a.trial_ref.cmp(&b.trial_ref));
    if !dry && !stale.is_empty() {
        let mut args = vec![
            "push".to_string(),
            "--quiet".to_string(),
            "origin".to_string(),
            "--delete".to_string(),
        ];
        args.extend(stale.iter().map(|s| format!("refs/heads/{}", s.trial_ref)));
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        git(g, Child::Authed, &refs)?;
    }
    Ok(stale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn root_of(env: &[(&str, &str)], cfg: &[(&str, &str)], home: Option<&str>) -> Result<PathBuf, String> {
        let e = |k: &str| env.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
        let c = |k: &str| cfg.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
        trial_root(&e, &c, home.map(Path::new))
    }

    /// An absolute folder on this platform: a bare `/d/t` has no drive on Windows and is not absolute there, so the tests that need
    /// an absolute path build it here (`C:/d/t` on Windows, `/d/t` elsewhere).
    fn abs(p: &str) -> String {
        if cfg!(windows) { format!("C:{p}") } else { p.to_string() }
    }

    #[test]
    fn the_trial_root_is_the_variable_then_the_config_then_trials_beside_the_worktree_root() {
        let (home_dir, t, x) = (abs("/home/me"), abs("/d/t"), abs("/d/x"));
        let (inside, dotdot) = (abs("/home/me/t"), abs("/d/../home/me"));
        let (wt, trials) = (abs("/data/dev-targets/wt"), abs("/data/dev-targets/trials"));
        let home = Some(home_dir.as_str());
        assert_eq!(
            root_of(&[("PADDOCK_TRIAL_DIR", &t)], &[("paddock.trialRoot", &x)], home).unwrap(),
            PathBuf::from(&t)
        );
        // The variable is checked like the config: inside HOME, relative or with `..` it is refused.
        for bad in [inside.as_str(), "rel/t", dotdot.as_str()] {
            let e = root_of(&[("PADDOCK_TRIAL_DIR", bad)], &[], home).unwrap_err();
            assert!(e.contains("PADDOCK_TRIAL_DIR") && e.contains("Next step"), "{bad}: {e}");
        }
        assert_eq!(root_of(&[], &[("paddock.trialRoot", &x)], home).unwrap(), PathBuf::from(&x));
        assert_eq!(
            root_of(&[], &[("paddock.worktreeRoot", &wt)], home).unwrap(),
            PathBuf::from(&trials)
        );
        assert_eq!(
            root_of(&[("PADDOCK_WORKTREE_ROOT", &wt)], &[], home).unwrap(),
            PathBuf::from(&trials)
        );
    }

    #[test]
    fn a_trial_root_that_cannot_be_used_says_why_and_the_next_step() {
        let (home_dir, in_trials, in_wt, dotdot) = (abs("/home/me"), abs("/home/me/trials"), abs("/home/me/wt"), abs("/d/../home/me"));
        let home = Some(home_dir.as_str());
        for (cfg, want) in [
            (vec![], "no trial folder is set"),
            (vec![("paddock.trialRoot", in_trials.as_str())], "inside your home folder"),
            (vec![("paddock.worktreeRoot", in_wt.as_str())], "inside your home folder"),
            (vec![("paddock.trialRoot", "relative/dir")], "not an absolute folder"),
            (vec![("paddock.trialRoot", dotdot.as_str())], "not an absolute folder"),
        ] {
            let e = root_of(&[], &cfg, home).unwrap_err();
            assert!(e.contains(want) && e.contains("Next step"), "{e}");
        }
    }

    #[allow(clippy::disallowed_methods, reason = "a test starts git in its own scratch repositories")]
    fn git_in(dir: &Path, args: &[&str]) -> String {
        let out = crate::git_env::git()
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", if cfg!(windows) { "NUL" } else { "/dev/null" })
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "protocol.file.allow=always",
                // No background `git maintenance` after each command: it is a start per command on a runner that pays for starts.
                "-c",
                "maintenance.auto=false",
                "-c",
                "gc.auto=0",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    struct World {
        _dir: tempfile::TempDir,
        origin: PathBuf,
        clone: PathBuf,
    }

    /// `git <args>` in `dir` with `input` on its standard input (one start for any number of objects or refs).
    #[allow(clippy::disallowed_methods, reason = "a test starts git in its own scratch repositories")]
    fn git_stdin(dir: &Path, args: &[&str], input: &str) {
        use std::io::Write;
        let mut child = crate::git_env::git()
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", if cfg!(windows) { "NUL" } else { "/dev/null" })
            .args(["-c", "maintenance.auto=false", "-c", "gc.auto=0"])
            .args(args)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        assert!(child.wait().unwrap().success(), "git {args:?}");
    }

    static CLOCK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    /// The `git fast-import` text of one new commit on each of `specs` (branch, file, text, parent branch; no parent for `""`).
    fn commit_script(specs: &[(&str, &str, &str, &str)]) -> String {
        let mut script = String::new();
        for (branch, file, text, from) in specs {
            let msg = format!("feat: {file}");
            // `^0` lets a commit continue its own branch: fast-import refuses `from` the same ref name, and without `from` it starts a root.
            let from_line = if from.is_empty() {
                String::new()
            } else {
                format!("from refs/heads/{from}{}\n", if branch == from { "^0" } else { "" })
            };
            script.push_str(&format!(
                "commit refs/heads/{branch}\ncommitter t <t@example.invalid> {} +0000\ndata {}\n{msg}\n{from_line}M 100644 inline {file}\ndata {}\n{text}\n\n",
                // Each commit is a second newer than the one before, so `--sort=-committerdate` has a fixed order.
                1_700_000_000 + CLOCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                msg.len(),
                text.len()
            ));
        }
        script
    }

    /// New commits on `specs`, made straight in the origin by one `git fast-import` (one start however many branches; a start costs
    /// about 45 ms on the Windows runner, ER-20). There is no working copy and no push.
    fn commits(w: &World, specs: &[(&str, &str, &str, &str)]) {
        git_stdin(&w.origin, &["fast-import", "--quiet", "--force"], &commit_script(specs));
    }

    /// Moves or deletes branches of the origin in one `git update-ref --stdin` (`update <ref> <to>`, `delete <ref>`).
    fn refs(w: &World, lines: &[&str]) {
        git_stdin(&w.origin, &["update-ref", "--stdin"], &(lines.join("\n") + "\n"));
    }

    fn commit(w: &World, branch: &str, file: &str, text: &str, from: &str) {
        commits(w, &[(branch, file, text, from)]);
    }

    /// An origin with main (shared.txt), a landed train, an open train with member `m1`, and a working copy that makes branches.
    fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let origin = dir.path().join("origin.git");
        git_in(dir.path(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
        no_auto_maintenance(&origin);
        let w = World {
            origin,
            clone: dir.path().join("clone"),
            _dir: dir,
        };
        // main (shared.txt) is the landed train/old; train/open is an open train with member m1: three commits, one start.
        let mut script = commit_script(&[("main", "shared.txt", "one\n", "")]);
        script.push_str(&commit_script(&[
            ("train/old", "old.txt", "old", "main"),
            ("train/open", "m1.txt", "m1", "main"),
        ]));
        script.push_str("reset refs/heads/main\nfrom refs/heads/train/old\n\n");
        git_stdin(&w.origin, &["fast-import", "--quiet"], &script);
        w
    }

    /// Writes the config that stops git's automatic maintenance (a `git maintenance` and `gc` start after fetches and pushes) into a
    /// repository's config file: a file write, not a git start.
    fn no_auto_maintenance(git_dir: &Path) {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(git_dir.join("config")).unwrap();
        writeln!(f, "[maintenance]\n\tauto = false\n[gc]\n\tauto = 0\n[receive]\n\tautogc = false").unwrap();
    }

    fn open(w: &World) -> GitTrain {
        let url = w.origin.to_str().unwrap();
        let g = GitTrain::open(url, &w.clone, None, false, true).unwrap();
        no_auto_maintenance(&g.root.join(".git"));
        g
    }

    fn ok(_: &Path, _: &str) -> Result<(), String> {
        Ok(())
    }

    #[test]
    fn a_clean_branch_is_merged_onto_the_open_train_and_pushed_to_its_trial_ref() {
        let w = world();
        commit(&w, "feat/a", "a.txt", "a", "main");
        let g = open(&w);
        let Outcome::Ready {
            trial_ref,
            target,
            target_sha,
            head,
        } = prepare(&g, "feat/a", None, &ok).unwrap()
        else {
            panic!("expected a ready trial");
        };
        assert_eq!(trial_ref, "trial/feat/a");
        assert_eq!(target, "origin/train/open", "the open train, not the landed one");
        let open_sha = git_in(&w.origin, &["rev-parse", "refs/heads/train/open"]);
        assert_eq!(target_sha, open_sha);
        // The pushed ref is the merge: first parent the train, second the branch, and it holds both members' files.
        assert_eq!(git_in(&w.origin, &["rev-parse", "refs/heads/trial/feat/a"]), head);
        assert_eq!(git_in(&w.origin, &["rev-parse", &format!("{head}^1")]), open_sha);
        assert_eq!(
            git_in(&w.origin, &["rev-parse", &format!("{head}^2")]),
            git_in(&w.origin, &["rev-parse", "refs/heads/feat/a"])
        );
        let files = git_in(&w.origin, &["ls-tree", "-r", "--name-only", &head]);
        assert!(files.contains("a.txt") && files.contains("m1.txt"), "{files}");
    }

    /// `--onto` names any pushed branch (a trial of an experiment on another experiment): it is fetched, not only main and
    /// `train/*` (the trial clone never had it, so the merge target did not resolve).
    #[test]
    fn onto_a_branch_that_is_neither_main_nor_a_train_fetches_it() {
        let w = world();
        // The trial clone is kept between runs: it was made before these branches existed, and only named refs are fetched into it.
        let g = open(&w);
        commit(&w, "exp/base", "base.txt", "base", "main");
        commit(&w, "feat/g", "g.txt", "g", "main");
        let Outcome::Ready { target, head, .. } = prepare(&g, "feat/g", Some("origin/exp/base"), &ok).unwrap() else {
            panic!("expected a ready trial");
        };
        assert_eq!(target, "origin/exp/base");
        let files = git_in(&w.origin, &["ls-tree", "-r", "--name-only", &head]);
        assert!(files.contains("base.txt") && files.contains("g.txt"), "{files}");
    }

    #[test]
    fn with_no_open_train_the_target_is_main() {
        let w = world();
        refs(&w, &["delete refs/heads/train/open"]);
        commit(&w, "feat/b", "b.txt", "b", "main");
        let g = open(&w);
        let Outcome::Ready { target, .. } = prepare(&g, "feat/b", None, &ok).unwrap() else {
            panic!()
        };
        assert_eq!(target, "origin/main");
    }

    #[test]
    fn a_conflict_with_the_train_names_its_files_and_pushes_nothing() {
        let w = world();
        commit(&w, "train/open", "shared.txt", "train side\n", "train/open");
        commit(&w, "feat/c", "shared.txt", "branch side\n", "main");
        let g = open(&w);
        let Outcome::Conflict { target, files, .. } = prepare(&g, "feat/c", None, &ok).unwrap() else {
            panic!("expected a conflict")
        };
        assert_eq!(target, "origin/train/open");
        assert_eq!(files, ["shared.txt"]);
        let (listed, _) = (git_in(&w.origin, &["branch", "--list", "trial/*"]), ());
        assert!(listed.is_empty(), "a conflicted trial is not pushed: {listed}");
    }

    #[test]
    fn a_finding_of_the_merge_check_stops_the_trial_before_the_push() {
        let w = world();
        commit(&w, "feat/d", "d.txt", "d", "main");
        let g = open(&w);
        let out = prepare(&g, "feat/d", None, &|_, _| Err("[lost-test] x".into())).unwrap();
        assert_eq!(out, Outcome::Findings("[lost-test] x".into()));
        assert!(git_in(&w.origin, &["branch", "--list", "trial/*"]).is_empty());
    }

    #[test]
    fn a_second_trial_of_the_same_branch_replaces_the_first() {
        let w = world();
        commit(&w, "feat/e", "e.txt", "e", "main");
        let g = open(&w);
        let Outcome::Ready { head: first, .. } = prepare(&g, "feat/e", None, &ok).unwrap() else {
            panic!()
        };
        commit(&w, "feat/e", "e2.txt", "e2", "feat/e");
        let Outcome::Ready { head: second, .. } = prepare(&g, "feat/e", None, &ok).unwrap() else {
            panic!()
        };
        assert_ne!(first, second);
        assert_eq!(git_in(&w.origin, &["rev-parse", "refs/heads/trial/feat/e"]), second);
    }

    #[test]
    fn a_branch_name_that_is_not_a_plain_ref_is_refused() {
        for bad in ["", "-x", "a..b", "a b", "a;b", "/a", "a/", "a:b", "a$(x)"] {
            assert!(!valid_branch(bad), "{bad:?}");
        }
        assert!(valid_branch("feat/adr-0098-member-gates") && valid_branch("fix/x.y_z"));
    }

    #[test]
    fn prune_deletes_the_trial_refs_of_gone_and_landed_branches_and_keeps_live_ones() {
        let w = world();
        for b in ["feat/live", "feat/gone", "feat/landed"] {
            commit(&w, b, &format!("{}.txt", b.replace('/', "_")), b, "main");
        }
        let g = open(&w);
        for b in ["feat/live", "feat/gone", "feat/landed"] {
            assert!(matches!(prepare(&g, b, None, &ok).unwrap(), Outcome::Ready { .. }));
        }
        // landed: the branch head reaches main; gone: the branch is deleted.
        refs(
            &w,
            &["update refs/heads/main refs/heads/feat/landed", "delete refs/heads/feat/gone"],
        );
        let dry = prune(&g, true).unwrap();
        assert_eq!(
            dry.iter().map(|s| s.trial_ref.as_str()).collect::<Vec<_>>(),
            ["trial/feat/gone", "trial/feat/landed"]
        );
        assert!(
            git_in(&w.origin, &["branch", "--list", "trial/*"]).contains("trial/feat/gone"),
            "a dry run deletes nothing"
        );
        let done = prune(&g, false).unwrap();
        assert_eq!(done.len(), 2);
        let left = git_in(&w.origin, &["branch", "--list", "trial/*"]);
        assert!(
            left.contains("trial/feat/live") && !left.contains("gone") && !left.contains("landed"),
            "{left}"
        );
    }

    /// The spawn budget of `prune` (ER-20, the class: one git start per ref, ADR 0108): the starts do not grow with the refs.
    /// 6 live, 1 gone and 1 landed trial refs: fetch, ls-remote, for-each-ref, one landed question for all, one push.
    #[test]
    fn prune_starts_a_fixed_number_of_gits_however_many_trial_refs_there_are() {
        let w = world();
        let names: Vec<String> = (0..8).map(|i| format!("feat/b{i}")).collect();
        let specs: Vec<(&str, String)> = names.iter().map(|n| (n.as_str(), format!("{}.txt", n.replace('/', "_")))).collect();
        let specs: Vec<(&str, &str, &str, &str)> = specs.iter().map(|(n, f)| (*n, f.as_str(), *n, "main")).collect();
        commits(&w, &specs);
        let g = open(&w);
        for n in &names {
            assert!(matches!(prepare(&g, n, None, &ok).unwrap(), Outcome::Ready { .. }));
        }
        refs(&w, &["update refs/heads/main refs/heads/feat/b0", "delete refs/heads/feat/b1"]);
        let before = g.starts();
        let done = prune(&g, false).unwrap();
        let used = g.starts() - before;
        assert_eq!(done.len(), 2, "{done:?}");
        assert!(
            used <= 5,
            "prune started {used} gits for 8 trial refs; the budget is 5 (fetch, ls-remote, for-each-ref, rev-list, push)"
        );
    }

    /// The spawn budget of `open_train`: one listing and one question for all trains, not one `merge-base` per landed train.
    #[test]
    fn open_train_starts_two_gits_however_many_landed_trains_are_newer() {
        let w = world();
        commits(&w, &[("train/l1", "l1.txt", "l1", "main")]);
        commits(&w, &[("train/l2", "l2.txt", "l2", "train/l1")]);
        commits(&w, &[("train/l3", "l3.txt", "l3", "train/l2")]);
        refs(&w, &["update refs/heads/main refs/heads/train/l3"]);
        let g = open(&w);
        git(
            &g,
            Child::Authed,
            &["fetch", "--quiet", "--no-tags", "origin", "+refs/heads/*:refs/remotes/origin/*"],
        )
        .unwrap();
        let before = g.starts();
        assert_eq!(open_train(&g).unwrap().as_deref(), Some("origin/train/open"));
        let used = g.starts() - before;
        assert!(used <= 2, "open_train started {used} gits with 4 landed trains; the budget is 2");
    }

    #[test]
    fn a_branch_sent_back_for_a_conflict_loses_its_earlier_trial_ref() {
        let w = world();
        commit(&w, "feat/f", "f.txt", "f", "main");
        let g = open(&w);
        assert!(matches!(prepare(&g, "feat/f", None, &ok).unwrap(), Outcome::Ready { .. }));
        commit(&w, "train/open", "shared.txt", "train\n", "train/open");
        commit(&w, "feat/f", "shared.txt", "branch\n", "feat/f");
        assert!(matches!(prepare(&g, "feat/f", None, &ok).unwrap(), Outcome::Conflict { .. }));
        assert!(git_in(&w.origin, &["branch", "--list", "trial/*"]).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_git_command_that_never_answers_ends_at_the_bound_and_says_so() {
        let w = world();
        let mut g = open(&w);
        g.timeout = std::time::Duration::from_secs(1);
        let e = g.run_git(Child::Local, &["-c", "alias.slow=!sleep 30", "slow"]).unwrap_err();
        assert!(e.contains("ended before it answered"), "{e}");
    }

    fn members(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    /// Several members already on origin are merged onto the target in order, as merge commits with the members' own commits (no
    /// rebase), and the one trial ref is pushed from the clone.
    #[test]
    fn members_on_origin_are_merged_in_order_as_merge_commits_and_pushed_from_the_clone() {
        let w = world();
        commit(&w, "feat/a", "a.txt", "a", "main");
        commit(&w, "feat/b", "b.txt", "b", "main");
        let g = open(&w);
        let ms = members(&["feat/a", "feat/b"]);
        let Outcome::Ready {
            trial_ref, target, head, ..
        } = prepare_members(&g, &ms, Some("origin/train/open"), &ok).unwrap()
        else {
            panic!("expected a ready trial");
        };
        assert_eq!(target, "origin/train/open");
        assert!(
            trial_ref.starts_with("trial/members-") && trial_ref.len() == "trial/members-".len() + 12,
            "{trial_ref}"
        );
        assert_eq!(git_in(&w.origin, &["rev-parse", &format!("refs/heads/{trial_ref}")]), head);
        // Two merge commits on the first-parent line, a first then b, the members' own commits as second parents.
        let first_parents = git_in(&w.origin, &["log", "--first-parent", "--format=%s", &head, "-3"]);
        let lines: Vec<&str> = first_parents.lines().collect();
        assert!(
            lines[0].starts_with("Merge feat/b") && lines[1].starts_with("Merge feat/a"),
            "{first_parents}"
        );
        let b = git_in(&w.origin, &["rev-parse", "refs/heads/feat/b"]);
        assert_eq!(
            git_in(&w.origin, &["rev-parse", &format!("{head}^2")]),
            b,
            "the member's own commit, not a rebased copy"
        );
        let tree = git_in(&w.origin, &["ls-tree", "-r", "--name-only", &head]);
        assert!(["a.txt", "b.txt", "m1.txt"].iter().all(|f| tree.contains(f)), "{tree}");
        assert_eq!(
            refusal(
                &Outcome::Ready {
                    trial_ref,
                    target,
                    target_sha: String::new(),
                    head
                },
                &ms
            ),
            None
        );
    }

    /// A member that is only local (not on origin) is refused before anything is merged or pushed, and named.
    #[test]
    fn a_member_that_is_not_on_origin_is_refused_and_nothing_is_pushed() {
        let w = world();
        commit(&w, "feat/a", "a.txt", "a", "main");
        // `feat/local` exists only locally: origin has no such branch.
        let g = open(&w);
        let ms = members(&["feat/a", "feat/local"]);
        let out = prepare_members(&g, &ms, None, &ok).unwrap();
        assert_eq!(out, Outcome::NotOnOrigin(vec!["feat/local".into()]));
        let why = refusal(&out, &ms).unwrap();
        assert!(why.contains("not on origin: feat/local") && why.contains("Next step"), "{why}");
        assert!(
            git_in(&w.origin, &["for-each-ref", "refs/heads/trial/"]).is_empty(),
            "nothing was pushed"
        );
    }

    /// The first member that conflicts stops the trial and is named, with the members merged before it; nothing is pushed.
    #[test]
    fn the_first_conflicting_member_stops_the_trial_and_is_named() {
        let w = world();
        commit(&w, "feat/a", "shared.txt", "from a\n", "main");
        commit(&w, "feat/b", "b.txt", "b", "main");
        commit(&w, "feat/c", "shared.txt", "from c\n", "main");
        let g = open(&w);
        let ms = members(&["feat/a", "feat/b", "feat/c"]);
        let out = prepare_members(&g, &ms, Some("origin/main"), &ok).unwrap();
        let Outcome::Conflict { member, files, .. } = &out else {
            panic!("expected a conflict, got {out:?}");
        };
        assert_eq!(member, "feat/c");
        assert_eq!(files, &vec!["shared.txt".to_string()]);
        let why = refusal(&out, &ms).unwrap();
        assert!(
            why.contains("feat/c conflicts") && why.contains("after merging feat/a, feat/b") && why.contains("shared.txt"),
            "{why}"
        );
        assert!(
            git_in(&w.origin, &["for-each-ref", "refs/heads/trial/"]).is_empty(),
            "nothing was pushed"
        );
    }

    #[test]
    fn a_trial_takes_one_to_ten_distinct_valid_members() {
        let w = world();
        let g = open(&w);
        assert!(prepare_members(&g, &[], None, &ok).unwrap_err().contains("1 to 10"));
        assert!(
            prepare_members(&g, &members(&["a"; 2]), None, &ok)
                .unwrap_err()
                .contains("named twice")
        );
        assert!(
            prepare_members(&g, &members(&["bad branch;x"]), None, &ok)
                .unwrap_err()
                .contains("not a branch name")
        );
        let many: Vec<String> = (0..11).map(|i| format!("m{i}")).collect();
        assert!(prepare_members(&g, &many, None, &ok).unwrap_err().contains("1 to 10"));
    }
}
