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
    fetch.push(format!("+refs/heads/{branch}:refs/remotes/origin/{branch}"));
    // A target that is neither main nor a train (an experiment branch) is fetched too: the kept clone only has the refs named here.
    if let Some(name) = onto.and_then(|t| t.strip_prefix("origin/"))
        && name != branch
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
        // A branch that is sent back keeps no trial ref from an earlier, cleaner trial.
        delete(g, &format!("trial/{branch}"))?;
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
        delete(g, &format!("trial/{branch}"))?;
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

/// The trial refs of branches that are gone or whose trialed head is on main now (landed), and the `sync/<member>/<short sha>` refs
/// `./dev merge sync-member` pushes (one per synced commit) with the `trial/sync/...` refs made from them: a sync ref goes once its tip is
/// on main or its member branch is gone from origin, and its trial ref goes with it. `dry` lists them without deleting.
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
            "+refs/heads/sync/*:refs/remotes/origin/sync/*",
        ],
    )?;
    let heads = git(g, Child::Authed, &["ls-remote", "--heads", "origin"])?;
    let branches: std::collections::BTreeSet<&str> = heads
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1))
        .filter_map(|r| r.strip_prefix("refs/heads/"))
        .collect();
    let listed = git(
        g,
        Child::Local,
        &["for-each-ref", "--format=%(refname:short)", "refs/remotes/origin/trial/"],
    )?;
    let mut stale = Vec::new();
    // The sync refs: one bulk read of those whose tip main contains, and the member branch of each (`sync/<member>/<sha>`).
    let (_, landed_out) = g.run_git(
        Child::Local,
        &[
            "for-each-ref",
            "--merged",
            "origin/main",
            "--format=%(refname:short)",
            "refs/remotes/origin/sync/",
        ],
    )?;
    let landed_sync: std::collections::BTreeSet<&str> = landed_out
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("origin/"))
        .collect();
    let all_sync = git(
        g,
        Child::Local,
        &["for-each-ref", "--format=%(refname:short)", "refs/remotes/origin/sync/"],
    )?;
    let mut stale_sync = std::collections::BTreeSet::new();
    for sync_ref in all_sync.lines().map(str::trim).filter_map(|l| l.strip_prefix("origin/")) {
        let member = sync_ref.strip_prefix("sync/").and_then(|r| r.rsplit_once('/')).map(|(m, _)| m);
        let why = if landed_sync.contains(sync_ref) {
            format!("{sync_ref} is on main")
        } else if member.is_some_and(|m| !branches.contains(m)) {
            format!("member branch {} is gone", member.unwrap_or_default())
        } else {
            continue;
        };
        stale_sync.insert(sync_ref.to_string());
        stale.push(Stale {
            trial_ref: sync_ref.to_string(),
            why,
        });
    }
    for short in listed.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let Some(trial_ref) = short.strip_prefix("origin/") else { continue };
        let branch = trial_ref.trim_start_matches("trial/");
        if stale_sync.contains(branch) {
            stale.push(Stale {
                trial_ref: trial_ref.to_string(),
                why: format!("its sync ref {branch} can go"),
            });
            continue;
        }
        if !branches.contains(branch) {
            stale.push(Stale {
                trial_ref: trial_ref.to_string(),
                why: format!("branch {branch} is gone"),
            });
            continue;
        }
        // per-item: one `merge-base --is-ancestor` of a trial's second parent, which no bulk form answers (`for-each-ref --merged` tests
        // the ref's own tip); the loop is bounded by the live trial refs, which prune keeps to a handful.
        // The head the trial merged is its second parent: on main, the branch landed.
        let (landed, _) = g.run_git(Child::Local, &["merge-base", "--is-ancestor", &format!("{short}^2"), "origin/main"])?;
        if landed {
            stale.push(Stale {
                trial_ref: trial_ref.to_string(),
                why: format!("branch {branch} landed on main"),
            });
        }
    }
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

    thread_local! {
        /// The git starts of this test's thread (each test runs on its own thread), what the budget test reads.
        static GIT_STARTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    #[allow(clippy::disallowed_methods, reason = "a test starts git in its own scratch repositories")]
    fn git_in(dir: &Path, args: &[&str]) -> String {
        GIT_STARTS.with(|c| c.set(c.get() + 1));
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

    /// The world's fixture, built once per machine and recipe (`paddock_testkit::git_fixture::template`): an origin with main (shared.txt), a landed train,
    /// an open train with member `m1`, and a working copy that makes branches. Each test gets a private copy ([`world`]), so no test
    /// shares mutable state and the dozens of git starts of the build are paid once, not by every test.
    fn template() -> PathBuf {
        paddock_testkit::git_fixture::template("trial", include_str!("trial.rs"), |build, ready| {
            let origin = build.join("origin.git");
            let work = build.join("work");
            std::fs::create_dir_all(&work).unwrap();
            git_in(build, &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
            git_in(&work, &["init", "-q", "-b", "main"]);
            // LF checkouts on every OS before the first checkout (ER-61).
            paddock_testkit::git_fixture::lf_only(&origin);
            paddock_testkit::git_fixture::lf_only(&work);
            git_in(&work, &["remote", "add", "origin", origin.to_str().unwrap()]);
            std::fs::write(work.join("shared.txt"), "one\n").unwrap();
            git_in(&work, &["add", "-A"]);
            git_in(&work, &["commit", "-q", "-m", "base"]);
            git_in(&work, &["push", "-q", "origin", "main"]);
            let w = World {
                origin,
                clone: build.join("clone"),
                work,
                _dir: tempfile::tempdir().unwrap(),
            };
            commit(&w, "train/old", "old.txt", "old", "main");
            git_in(&w.work, &["checkout", "-q", "main"]);
            git_in(&w.work, &["merge", "-q", "--ff-only", "train/old"]);
            git_in(&w.work, &["push", "-q", "origin", "main"]);
            commit(&w, "train/open", "m1.txt", "m1", "main");
            // The working copy's url names the scratch folder; it must name the template's final place.
            let cfg = w.work.join(".git").join("config");
            let text = std::fs::read_to_string(&cfg).unwrap();
            std::fs::write(&cfg, text.replace(&escaped(&w.origin), &escaped(&ready.join("origin.git")))).unwrap();
        })
    }

    /// A path as git's config file writes it in a value (a backslash, on Windows, is doubled).
    fn escaped(p: &Path) -> String {
        p.to_str().unwrap().replace('\\', "\\\\")
    }

    /// A private copy of the template, made with no git start: both folders are copied and the working copy's `origin` url, the one
    /// absolute path in it, is pointed at the copy of the origin.
    fn world() -> World {
        let t = template();
        let (torigin, twork) = (t.join("origin.git"), t.join("work"));
        let dir = tempfile::tempdir().unwrap();
        let origin = dir.path().join("origin.git");
        let work = dir.path().join("work");
        paddock_testkit::git_fixture::copy_tree(&torigin, &origin);
        paddock_testkit::git_fixture::copy_tree(&twork, &work);
        let cfg = work.join(".git").join("config");
        let text = std::fs::read_to_string(&cfg).unwrap();
        let (old, new) = (escaped(&torigin), escaped(&origin));
        assert!(text.contains(&old), "the template's origin url is in the working copy's config");
        std::fs::write(&cfg, text.replace(&old, &new)).unwrap();
        World {
            origin,
            clone: dir.path().join("clone"),
            work,
            _dir: dir,
        }
    }

    /// Budget (rule 85): a test's world costs no git start once the template exists, and is private: a push in one copy is not in
    /// another or in the template.
    #[test]
    fn a_world_is_a_private_copy_of_the_template_and_costs_no_git_start() {
        let t = template();
        let before = GIT_STARTS.with(|c| c.get());
        let a = world();
        let b = world();
        assert_eq!(GIT_STARTS.with(|c| c.get()), before, "a world is a file copy, not git starts");
        commit(&a, "feat/only-a", "a.txt", "a", "main");
        assert!(git_in(&a.origin, &["branch", "--list", "feat/only-a"]).contains("only-a"));
        assert!(git_in(&b.origin, &["branch", "--list", "feat/only-a"]).is_empty());
        assert!(git_in(&t.join("origin.git"), &["branch", "--list", "feat/only-a"]).is_empty());
        assert!(
            git_in(&b.origin, &["branch", "--list", "train/open"]).contains("train/open"),
            "the template's content is there"
        );
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
        git_in(&w.work, &["checkout", "-q", "main"]);
        git_in(&w.work, &["merge", "-q", "--ff-only", "feat/landed"]);
        git_in(&w.work, &["push", "-q", "origin", "main"]);
        git_in(&w.work, &["push", "-q", "origin", "--delete", "feat/gone"]);
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

    /// The refs `merge sync-member` pushes (`sync/<member>/<short sha>`) and the trials made from them go once their tip is on main or
    /// their member branch is gone; a live member's refs stay.
    #[test]
    fn prune_deletes_the_sync_refs_and_their_trial_refs_of_landed_and_gone_members_and_keeps_live_ones() {
        let w = world();
        for m in ["feat/s", "feat/t", "feat/u"] {
            commit(&w, m, &format!("{}.txt", m.replace('/', "_")), m, "main");
        }
        let syncs = ["sync/feat/s/aaaaaaaaa", "sync/feat/t/bbbbbbbbb", "sync/feat/u/ccccccccc"];
        for (sync, m) in syncs.iter().zip(["feat/s", "feat/t", "feat/u"]) {
            commit(&w, sync, "synced.txt", sync, m);
        }
        let g = open(&w);
        for sync in syncs {
            assert!(matches!(prepare(&g, sync, None, &ok).unwrap(), Outcome::Ready { .. }), "{sync}");
        }
        // feat/s landed (its sync tip is on main); feat/u's branch is deleted; feat/t is live.
        git_in(&w.work, &["checkout", "-q", "main"]);
        git_in(&w.work, &["merge", "-q", "--ff-only", syncs[0]]);
        git_in(&w.work, &["push", "-q", "origin", "main"]);
        git_in(&w.work, &["push", "-q", "origin", "--delete", "feat/u"]);
        let dry = prune(&g, true).unwrap();
        let mut named: Vec<&str> = dry.iter().map(|s| s.trial_ref.as_str()).collect();
        named.sort_unstable();
        assert_eq!(
            named,
            [
                "sync/feat/s/aaaaaaaaa",
                "sync/feat/u/ccccccccc",
                "trial/sync/feat/s/aaaaaaaaa",
                "trial/sync/feat/u/ccccccccc"
            ]
        );
        assert!(
            git_in(&w.origin, &["branch", "--list", "sync/*"]).contains("aaaaaaaaa"),
            "a dry run deletes nothing"
        );
        assert_eq!(prune(&g, false).unwrap().len(), 4);
        let left = git_in(&w.origin, &["branch", "--list", "sync/*", "trial/sync/*"]);
        assert!(
            left.contains("sync/feat/t/bbbbbbbbb")
                && left.contains("trial/sync/feat/t/bbbbbbbbb")
                && !left.contains("aaaa")
                && !left.contains("cccc"),
            "{left}"
        );
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
}
