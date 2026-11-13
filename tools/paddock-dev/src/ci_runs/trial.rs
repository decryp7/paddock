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
    /// Git's merge conflicts: the files, the target and the branch's head. Nothing was pushed.
    Conflict {
        /// What the branch was to be merged onto.
        target: String,
        /// The conflicting files.
        files: Vec<String>,
    },
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
            "--format=%(refname:short)",
            "refs/remotes/origin/train/",
        ],
    )?;
    for name in listed.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (ok, _) = g.run_git(Child::Local, &["merge-base", "--is-ancestor", name, "origin/main"])?;
        if !ok {
            return Ok(Some(name.to_string()));
        }
    }
    Ok(None)
}

/// Fetches, merges and pushes. `check` is the merge check on the trial commit (the clone is its root); it returns the findings'
/// text as an error. `onto` names the target (`origin/train/x`) instead of the open train.
pub fn prepare(
    g: &GitTrain,
    branch: &str,
    onto: Option<&str>,
    check: &dyn Fn(&Path, &str) -> Result<(), String>,
) -> Result<Outcome, String> {
    if !valid_branch(branch) {
        return Err(format!(
            "`{branch}` is not a branch name this command accepts (letters, digits, . _ - and /)"
        ));
    }
    git(
        g,
        Child::Authed,
        &[
            "fetch",
            "--quiet",
            "--no-tags",
            "origin",
            "+refs/heads/main:refs/remotes/origin/main",
            "+refs/heads/train/*:refs/remotes/origin/train/*",
            &format!("+refs/heads/{branch}:refs/remotes/origin/{branch}"),
        ],
    )?;
    let target = match onto {
        Some(t) => t.to_string(),
        None => open_train(g)?.unwrap_or_else(|| "origin/main".to_string()),
    };
    let target_sha = git(g, Child::Local, &["rev-parse", "--verify", &format!("{target}^{{commit}}")])?
        .trim()
        .to_string();
    let branch_sha = git(
        g,
        Child::Local,
        &["rev-parse", "--verify", &format!("refs/remotes/origin/{branch}^{{commit}}")],
    )?
    .trim()
    .to_string();
    // Git's own merge, in memory: exit 1 is a conflict and lists the files.
    let (clean, out) = g.run_git(
        Child::Local,
        &[
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--no-messages",
            &target_sha,
            &branch_sha,
        ],
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
            return Err(format!("git merge-tree failed: {}", out.trim()));
        }
        return Ok(Outcome::Conflict { target, files });
    }
    let tree = out.lines().next().unwrap_or("").trim().to_string();
    let message = format!(
        "Merge {branch} ({}) into trial of {target}\n",
        branch_sha.get(..9).unwrap_or(&branch_sha)
    );
    let head = git(
        g,
        Child::Local,
        &["commit-tree", &tree, "-p", &target_sha, "-p", &branch_sha, "-m", &message],
    )?
    .trim()
    .to_string();
    if let Err(findings) = check(&g.root, &head) {
        return Ok(Outcome::Findings(findings));
    }
    let trial_ref = format!("trial/{branch}");
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[allow(clippy::disallowed_methods, reason = "a test starts git in its own scratch repositories")]
    fn git_in(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
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
        work: PathBuf,
        clone: PathBuf,
    }

    fn commit(w: &World, branch: &str, file: &str, text: &str, from: &str) {
        git_in(&w.work, &["checkout", "-q", "-B", branch, from]);
        std::fs::write(w.work.join(file), text).unwrap();
        git_in(&w.work, &["add", "-A"]);
        git_in(&w.work, &["commit", "-q", "-m", &format!("feat: {file}")]);
        git_in(&w.work, &["push", "-q", "-f", "origin", branch]);
    }

    /// An origin with main (shared.txt), a landed train, an open train with member `m1`, and a working copy that makes branches.
    fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let origin = dir.path().join("origin.git");
        let work = dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        git_in(dir.path(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
        git_in(&work, &["init", "-q", "-b", "main"]);
        git_in(&work, &["remote", "add", "origin", origin.to_str().unwrap()]);
        std::fs::write(work.join("shared.txt"), "one\n").unwrap();
        git_in(&work, &["add", "-A"]);
        git_in(&work, &["commit", "-q", "-m", "base"]);
        git_in(&work, &["push", "-q", "origin", "main"]);
        let w = World {
            origin,
            clone: dir.path().join("clone"),
            work,
            _dir: dir,
        };
        commit(&w, "train/old", "old.txt", "old", "main");
        git_in(&w.work, &["checkout", "-q", "main"]);
        git_in(&w.work, &["merge", "-q", "--ff-only", "train/old"]);
        git_in(&w.work, &["push", "-q", "origin", "main"]);
        commit(&w, "train/open", "m1.txt", "m1", "main");
        w
    }

    fn open(w: &World) -> GitTrain {
        let url = w.origin.to_str().unwrap();
        GitTrain::open(url, &w.clone, None, false, true).unwrap()
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

    #[test]
    fn with_no_open_train_the_target_is_main() {
        let w = world();
        git_in(&w.work, &["push", "-q", "origin", "--delete", "train/open"]);
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
        let Outcome::Conflict { target, files } = prepare(&g, "feat/c", None, &ok).unwrap() else {
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
}
