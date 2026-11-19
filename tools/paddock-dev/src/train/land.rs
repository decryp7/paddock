//! `train land`: fast-forwards `main` to the train head and verifies it (the port of the integrator's `land-*.sh` tails).
//!
//! Only a fast-forward: the push names the train head as the new value of `main` with no `+`, no force and no rebase, and is
//! refused before it is made when `origin/main` is not an ancestor of the head. The result is verified with the remote's own
//! answer (`ls-remote`), never with the push's exit code alone. Git is behind [`Git`], so the tests never touch a repository.

use super::sequence::Stop;
use std::io::Write;

/// The git calls `land` needs, on the integrator's own worktree.
pub trait Git {
    /// The commit `HEAD` of the worktree is on.
    fn head(&mut self) -> Result<String, String>;
    /// What the remote says `refs/heads/<name>` is (`ls-remote`); `None` when the remote has no such branch.
    fn remote_branch(&mut self, name: &str) -> Result<Option<String>, String>;
    /// Fetches the remote's branches (so `main`'s head is a local object).
    fn fetch(&mut self) -> Result<(), String>;
    /// Whether `ancestor` is reachable from `of`.
    fn is_ancestor(&mut self, ancestor: &str, of: &str) -> Result<bool, String>;
    /// Pushes `sha` as the new value of the remote branch `name`, never forced. The repository's own pre-push gate runs.
    fn push_ff(&mut self, sha: &str, name: &str) -> Result<(), String>;
}

/// How the landing ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Landed {
    /// `main` was moved to the head.
    FastForwarded {
        /// What `main` was.
        from: String,
        /// The head it is now.
        to: String,
    },
    /// `main` already was the head (a landing that was made before; nothing was pushed).
    AlreadyOnMain(String),
}

/// A train branch: `train/` and then letters, digits, `.`, `_`, `-`.
pub fn check_branch(branch: &str) -> Result<(), String> {
    let ok = branch
        .strip_prefix("train/")
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')));
    if ok {
        Ok(())
    } else {
        Err(format!("`{branch}` is not a train branch (train/<name>)"))
    }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(9)]
}

/// Fast-forwards `main` to the head of `branch` and verifies it.
pub fn land(branch: &str, git: &mut dyn Git, log: &mut dyn Write) -> Result<Landed, Stop> {
    check_branch(branch).map_err(|e| Stop::new(e, "name the train branch, for example train/2026-11-19-16"))?;
    let git_err = |what: &str| {
        let what = what.to_string();
        move |e: String| Stop::new(format!("{what}: {e}"), "fix what git printed, then run `./dev train land` again")
    };
    let head = git.head().map_err(git_err("the worktree's HEAD could not be read"))?;
    let remote = git
        .remote_branch(branch)
        .map_err(git_err("the remote could not be asked"))?
        .ok_or_else(|| {
            Stop::new(
                format!("the remote has no {branch}"),
                "push the train branch through the gate, run its CI, then land it",
            )
        })?;
    if remote != head {
        return Err(Stop::new(
            format!("this worktree is on {} but origin/{branch} is {}", short(&head), short(&remote)),
            "run this in the worktree that holds the train, with the pushed head checked out",
        ));
    }
    git.fetch().map_err(git_err("the fetch failed"))?;
    let main = git
        .remote_branch("main")
        .map_err(git_err("the remote could not be asked for main"))?
        .ok_or_else(|| Stop::new("the remote has no main", "check the remote"))?;
    if main == head {
        let _ = writeln!(log, "train land: main is already {}; nothing to push", short(&head));
        return Ok(Landed::AlreadyOnMain(head));
    }
    let ff = git
        .is_ancestor(&main, &head)
        .map_err(git_err("the ancestry of main could not be read"))?;
    if !ff {
        return Err(Stop::new(
            format!(
                "origin/main ({}) is not an ancestor of {}: no fast-forward",
                short(&main),
                short(&head)
            ),
            "merge origin/main into the train, push it through the gate and run the CI sequence again; never force or rebase",
        ));
    }
    let _ = writeln!(log, "train land: fast-forwarding main {} -> {}", short(&main), short(&head));
    git.push_ff(&head, "main").map_err(git_err("the push of main was refused"))?;
    let now = git
        .remote_branch("main")
        .map_err(git_err("main could not be verified"))?
        .unwrap_or_default();
    if now != head {
        return Err(Stop::new(
            format!(
                "after the push the remote's main is {} and not {}",
                if now.is_empty() { "missing" } else { short(&now) },
                short(&head)
            ),
            "read the push output above and `git ls-remote origin refs/heads/main`; do not push again until you know why",
        ));
    }
    let _ = writeln!(log, "train land: LANDED main={} (verified with ls-remote)", short(&now));
    Ok(Landed::FastForwarded { from: main, to: head })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OLD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    /// A remote and a worktree in a table. `push_moves` is whether a push really moves the remote's main.
    struct Fake {
        head: String,
        remote: BTreeMap<String, String>,
        ancestor: bool,
        push_moves: bool,
        pushed: Vec<(String, String)>,
        fetched: usize,
        push_error: Option<String>,
    }

    fn fake() -> Fake {
        Fake {
            head: HEAD.into(),
            remote: BTreeMap::from([("train/x".to_string(), HEAD.to_string()), ("main".to_string(), OLD.to_string())]),
            ancestor: true,
            push_moves: true,
            pushed: Vec::new(),
            fetched: 0,
            push_error: None,
        }
    }

    impl Git for Fake {
        fn head(&mut self) -> Result<String, String> {
            Ok(self.head.clone())
        }
        fn remote_branch(&mut self, name: &str) -> Result<Option<String>, String> {
            Ok(self.remote.get(name).cloned())
        }
        fn fetch(&mut self) -> Result<(), String> {
            self.fetched += 1;
            Ok(())
        }
        fn is_ancestor(&mut self, _: &str, _: &str) -> Result<bool, String> {
            Ok(self.ancestor)
        }
        fn push_ff(&mut self, sha: &str, name: &str) -> Result<(), String> {
            self.pushed.push((sha.into(), name.into()));
            if let Some(e) = &self.push_error {
                return Err(e.clone());
            }
            if self.push_moves {
                self.remote.insert(name.into(), sha.into());
            }
            Ok(())
        }
    }

    fn run(f: &mut Fake) -> Result<Landed, Stop> {
        land("train/x", f, &mut Vec::new())
    }

    /// The positive control: a train that is ahead of main, pushed and checked out, lands, and the remote says so.
    #[test]
    fn a_fast_forward_lands_and_is_verified() {
        let mut f = fake();
        let got = run(&mut f).unwrap();
        assert_eq!(
            got,
            Landed::FastForwarded {
                from: OLD.into(),
                to: HEAD.into()
            }
        );
        assert_eq!(f.pushed, vec![(HEAD.to_string(), "main".to_string())]);
        assert_eq!(f.fetched, 1);
    }

    #[test]
    fn a_push_the_remote_did_not_take_is_not_a_landing() {
        let mut f = fake();
        f.push_moves = false;
        let stop = run(&mut f).unwrap_err();
        assert!(
            stop.cause
                .contains("after the push the remote's main is bbbbbbbbb and not aaaaaaaaa"),
            "{stop:?}"
        );
    }

    #[test]
    fn a_main_that_moved_on_is_no_fast_forward_and_nothing_is_pushed() {
        let mut f = fake();
        f.ancestor = false;
        let stop = run(&mut f).unwrap_err();
        assert!(
            stop.cause.contains("not an ancestor") && stop.next.contains("never force or rebase"),
            "{stop:?}"
        );
        assert!(f.pushed.is_empty());
    }

    #[test]
    fn a_worktree_that_is_not_on_the_pushed_head_pushes_nothing() {
        let mut f = fake();
        f.head = OLD.into();
        assert!(
            run(&mut f)
                .unwrap_err()
                .cause
                .contains("this worktree is on bbbbbbbbb but origin/train/x is aaaaaaaaa")
        );
        assert!(f.pushed.is_empty());
        let mut f = fake();
        f.remote.remove("train/x");
        assert!(run(&mut f).unwrap_err().cause.contains("the remote has no train/x"));
        assert!(f.pushed.is_empty());
    }

    #[test]
    fn a_main_that_already_is_the_head_pushes_nothing() {
        let mut f = fake();
        f.remote.insert("main".into(), HEAD.into());
        assert_eq!(run(&mut f).unwrap(), Landed::AlreadyOnMain(HEAD.into()));
        assert!(f.pushed.is_empty());
    }

    #[test]
    fn a_refused_push_stops_with_the_cause() {
        let mut f = fake();
        f.push_error = Some("pre-push: conformance failed".into());
        let stop = run(&mut f).unwrap_err();
        assert!(
            stop.cause.contains("the push of main was refused: pre-push: conformance failed"),
            "{stop:?}"
        );
    }

    #[test]
    fn only_train_branches_land() {
        for bad in ["main", "feat/x", "train/", "train/a b", "train/x;rm", "", "refs/heads/train/x"] {
            let mut f = fake();
            assert!(land(bad, &mut f, &mut Vec::new()).is_err(), "{bad}");
            assert!(f.pushed.is_empty());
        }
    }
}
