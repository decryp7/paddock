//! The guard of pushes to `refs/heads/main` (user, 2026-11-28; ER-49, ER-33): a push to main lands only a commit that
//! `ci landing-held` says may land (the complete green CI run plus the green slow rows on that exact commit), and only as a
//! fast-forward. It reads git's pre-push lines (`<local ref> <local sha> <remote ref> <remote sha>`, githooks(5), "pre-push")
//! and asks [`super::known::Lookups::landing`], which is [`super::landing_held::landing_of`]: the one definition (ER-22).
//! Deletes and every other ref are left alone. Anything that cannot be read refuses (never a pass, ER-33).

use super::known::Lookups;
use super::train_split::Landing;
use paddock_cli::Exit;
use std::path::Path;

/// The remote ref the guard protects.
pub const MAIN_REF: &str = "refs/heads/main";

/// Whether `sha` is git's all-zero id (a new remote ref, or a delete when it is the local side).
fn is_zero(sha: &str) -> bool {
    !sha.is_empty() && sha.bytes().all(|b| b == b'0')
}

/// Whether `ancestor` is an ancestor of `descendant`: `Err` when git cannot say (a commit missing here).
fn is_ancestor(root: &Path, ancestor: &str, descendant: &str) -> Result<bool, String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .output()
        .map_err(|e| format!("git could not be run: {e}"))?;
    match out.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
    }
}

/// The refusal for the pushed lines in `stdin`, or `None` when no push to main is in them or every one may land. The exit is
/// `Findings` for a refusal that is a fact (not a fast-forward, red CI, a failed slow row) and `NotMeasured` for one that is not
/// held or could not be read. One `::error::` line, with the cause and the next step.
pub fn check(root: &Path, stdin: &str, look: &dyn Lookups) -> Option<(Exit, String)> {
    for line in stdin.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let [_local_ref, local, remote_ref, remote] = f[..] else { continue };
        if remote_ref != MAIN_REF || is_zero(local) {
            continue;
        }
        if !is_zero(remote) {
            match is_ancestor(root, remote, local) {
                Ok(true) => {}
                Ok(false) => {
                    return Some((
                        Exit::Findings,
                        format!(
                            "::error::pre-push: refused: the push to main is not a fast-forward ({remote} is not an ancestor of {local}). Next step: merge origin/main into the branch, let it land through its train, and push the result."
                        ),
                    ));
                }
                Err(why) => {
                    return Some((
                        Exit::NotMeasured,
                        format!(
                            "::error::pre-push: refused: whether the push to main is a fast-forward could not be read ({why}). Next step: git fetch origin, then push again."
                        ),
                    ));
                }
            }
        }
        match look.landing(local) {
            Landing::Landable => {}
            Landing::Refused(why) => {
                return Some((
                    Exit::Findings,
                    format!(
                        "::error::pre-push: refused: {local} may not land on main: {why}. Next step: fix it and get a complete green run on that commit (./dev ci landing-held --sha {local})."
                    ),
                ));
            }
            Landing::NotHeld(why) => {
                return Some((
                    Exit::NotMeasured,
                    format!(
                        "::error::pre-push: refused: {local} is not held for landing on main: {why}. Next step: wait for or run the checks, then ./dev ci landing-held --sha {local} must say it may land."
                    ),
                ));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::git_text;
    use std::path::PathBuf;

    /// The landing answer of a fake CI and slow record; nothing else is looked up.
    struct Fake(Landing);
    impl Lookups for Fake {
        fn ls_remote(&self, _: &str) -> Option<String> {
            None
        }
        fn ci_green(&self, _: &str) -> Result<u64, String> {
            Err("not used by the guard".into())
        }
        fn landing(&self, _: &str) -> Landing {
            self.0.clone()
        }
    }

    /// main: one-two; `side` forks from one (so two is not its ancestor).
    fn repo() -> (tempfile::TempDir, PathBuf, String, String, String) {
        let d = tempfile::tempdir().unwrap();
        let r = d.path().join("r");
        std::fs::create_dir(&r).unwrap();
        let git = |a: &[&str]| git_text(&r, a).unwrap_or_else(|| panic!("git {a:?}"));
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "t"]);
        let commit = |f: &str| {
            std::fs::write(r.join(f), f).unwrap();
            git(&["add", f]);
            git(&["commit", "-q", "-m", f]);
            git(&["rev-parse", "HEAD"]).trim().to_string()
        };
        let one = commit("one");
        let two = commit("two");
        git(&["checkout", "-q", "-b", "side", &one]);
        let side = commit("side");
        (d, r, one, two, side)
    }

    fn line(local_ref: &str, local: &str, remote_ref: &str, remote: &str) -> String {
        format!("{local_ref} {local} {remote_ref} {remote}\n")
    }
    fn zero() -> String {
        "0".repeat(40)
    }

    #[test]
    fn a_fast_forward_of_a_landing_held_commit_passes() {
        let (_d, r, one, two, _) = repo();
        let got = check(&r, &line("refs/heads/main", &two, MAIN_REF, &one), &Fake(Landing::Landable));
        assert_eq!(got, None);
    }

    #[test]
    fn a_commit_without_a_green_run_is_refused_with_its_cause_and_next_step() {
        let (_d, r, one, two, _) = repo();
        let why = "CI: no complete run of ci.yml on it".to_string();
        let (exit, text) = check(
            &r,
            &line("refs/heads/x", &two, MAIN_REF, &one),
            &Fake(Landing::NotHeld(why.clone())),
        )
        .unwrap();
        assert_eq!(exit, Exit::NotMeasured);
        assert_eq!(
            text,
            format!(
                "::error::pre-push: refused: {two} is not held for landing on main: {why}. Next step: wait for or run the checks, then ./dev ci landing-held --sha {two} must say it may land."
            )
        );
        let (exit, text) = check(
            &r,
            &line("refs/heads/x", &two, MAIN_REF, &one),
            &Fake(Landing::Refused("CI is not green: red".into())),
        )
        .unwrap();
        assert_eq!(exit, Exit::Findings);
        assert!(
            text.contains(&format!("{two} may not land on main: CI is not green: red")),
            "{text}"
        );
    }

    #[test]
    fn a_non_fast_forward_is_refused_even_when_the_commit_may_land() {
        let (_d, r, _one, two, side) = repo();
        let (exit, text) = check(&r, &line("refs/heads/side", &side, MAIN_REF, &two), &Fake(Landing::Landable)).unwrap();
        assert_eq!(exit, Exit::Findings);
        assert_eq!(
            text,
            format!(
                "::error::pre-push: refused: the push to main is not a fast-forward ({two} is not an ancestor of {side}). Next step: merge origin/main into the branch, let it land through its train, and push the result."
            )
        );
    }

    #[test]
    fn a_remote_commit_git_cannot_find_or_an_unreadable_verdict_refuses_never_passes() {
        let (_d, r, one, two, _) = repo();
        let missing = "1".repeat(40);
        let (exit, text) = check(&r, &line("refs/heads/x", &two, MAIN_REF, &missing), &Fake(Landing::Landable)).unwrap();
        assert_eq!(exit, Exit::NotMeasured);
        assert!(text.contains("could not be read") && text.contains("git fetch origin"), "{text}");
        let unread = Landing::NotHeld("CI: GITEA_TOKEN is not set, so the runs cannot be read".into());
        let (exit, text) = check(&r, &line("refs/heads/x", &two, MAIN_REF, &one), &Fake(unread)).unwrap();
        assert_eq!(exit, Exit::NotMeasured);
        assert!(text.contains("GITEA_TOKEN is not set"), "{text}");
    }

    #[test]
    fn a_new_main_needs_a_landing_too() {
        let (_d, r, _one, two, _) = repo();
        let l = line("refs/heads/x", &two, MAIN_REF, &zero());
        assert_eq!(check(&r, &l, &Fake(Landing::Landable)), None);
        assert!(check(&r, &l, &Fake(Landing::NotHeld("x".into()))).is_some());
    }

    #[test]
    fn other_branches_tags_and_deletes_are_unaffected() {
        let (_d, r, one, two, side) = repo();
        let never = Fake(Landing::NotHeld("must not matter".into()));
        for l in [
            line("refs/heads/x", &two, "refs/heads/feat/x", &zero()),
            line("refs/heads/x", &side, "refs/heads/train/t", &two),
            line("refs/heads/x", &two, "refs/heads/main2", &one),
            line("refs/tags/v1.0.0", &two, "refs/tags/v1.0.0", &zero()),
            line("(delete)", &zero(), MAIN_REF, &two),
            String::new(),
        ] {
            assert_eq!(check(&r, &l, &never), None, "{l}");
        }
    }

    #[test]
    fn main_is_found_among_several_refs_of_one_push() {
        let (_d, r, one, two, _) = repo();
        let l = line("refs/heads/a", &two, "refs/heads/feat/a", &zero()) + &line("refs/heads/b", &two, MAIN_REF, &one);
        assert!(check(&r, &l, &Fake(Landing::NotHeld("x".into()))).is_some());
        assert_eq!(check(&r, &l, &Fake(Landing::Landable)), None);
    }
}
