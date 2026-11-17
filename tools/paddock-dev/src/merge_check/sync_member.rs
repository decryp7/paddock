//! `./dev merge sync-member <train>`: the author's step that keeps a member's own work when the train carries a revert of it.
//!
//! The failure it ends (three times, the last c156ba72a): the train merged a member, reverted it (a member pulled out), and the
//! author then merged the train into the member branch to catch up. Git takes the train's revert as the newer change, so the
//! member's own work is silently deleted from the member branch. This merges the train, then finds each live train revert whose
//! `This reverts commit X` names a merge of this member ([`super::reverted_base`]'s reading of `member..train`) and adds the
//! revert of that revert, `revert: reapply "<merge subject>"`, the commit [`super::reland`] ties to when the member is merged back.
//!
//! It writes only in the author's worktree and runs git with its hooks on (ER-39). A conflict, a dirty tree or a detached HEAD
//! stops it with the cause and the next step; it never forces. Run twice, the second run finds nothing to do.

use super::git::{Git, short};
use super::reverted_base::live_for_member;
use paddock_cli::{CliError, Report};
use serde_json::json;
use std::path::Path;

fn run(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = crate::git_env::git();
    cmd.arg("-C").arg(root).args(args);
    let out = paddock_proc::run_bounded(
        cmd,
        paddock_proc::Limits::new(std::time::Duration::from_secs(600)).max_output(1 << 24),
    )
    .map_err(|e| format!("git {} could not run: {e}", args.join(" ")))?;
    if !out.success() {
        let err = out.stderr_text();
        let msg = if err.trim().is_empty() { out.stdout_text() } else { err };
        return Err(format!("git {} failed: {}", args.join(" "), msg.trim()));
    }
    Ok(out.stdout_text())
}

/// The commit message of the revert of a revert.
pub fn reapply_message(merge_subject: &str, revert: &str) -> String {
    format!("revert: reapply \"{merge_subject}\"\n\nThis reverts commit {revert}.\n")
}

/// Merges `train` into the branch checked out in `root` and re-applies the member's reverted work.
pub fn command(root: Option<&Path>, train: &str) -> Result<Report, CliError> {
    let root = crate::gates::command::repo_root(root)?;
    let step = |why: String, next: &str| CliError::failure(format!("::error::merge sync-member: {why}. Next step: {next}"));
    let git = Git::new(&root);
    let branch = run(&root, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .map(|b| b.trim().to_string())
        .map_err(|_| {
            step(
                "HEAD is not on a branch".into(),
                "check out the member's branch in your worktree, then run it again",
            )
        })?;
    let dirty = run(&root, &["status", "--porcelain"]).map_err(|e| step(e, "run `git status` to see why"))?;
    if !dirty.trim().is_empty() {
        return Err(step(
            "the worktree has uncommitted changes".into(),
            "commit them (a merge needs a clean tree), then run it again",
        ));
    }
    let train_tip = run(&root, &["rev-parse", "--verify", &format!("{train}^{{commit}}")])
        .map(|t| t.trim().to_string())
        .map_err(|e| {
            step(
                e,
                "run `git fetch origin` and give the train's full ref, for example origin/train/<date>-<n>",
            )
        })?;
    let before = run(&root, &["rev-parse", "HEAD"])
        .map_err(|e| step(e, "run `git status`"))?
        .trim()
        .to_string();
    // What the member holds that the train reverted, read before the merge: `member..train` is the train's side only then.
    let reverts = live_for_member(&git, &train_tip, &before).map_err(|e| step(e, "fix what is named, then run it again"))?;
    let merged = if run(&root, &["merge-base", "--is-ancestor", &train_tip, "HEAD"]).is_ok() {
        false
    } else {
        run(&root, &["merge", "--no-edit", &train_tip]).map_err(|e| {
            step(
                e,
                "resolve the conflicts and commit the merge (git's usual way), then run `./dev merge sync-member` again to re-apply what the train reverted",
            )
        })?;
        true
    };
    let mut applied = Vec::new();
    for (merge, revert, subject) in &reverts {
        let message = reapply_message(subject, revert);
        let tried = run(&root, &["revert", "--no-commit", revert]).and_then(|_| run(&root, &["commit", "-q", "-m", &message]));
        if let Err(e) = tried {
            let _ = run(&root, &["revert", "--abort"]);
            return Err(step(
                format!("re-applying {} (the revert of {}) failed: {e}", short(revert), short(merge)),
                &format!("run `git revert {revert}` yourself, resolve it, and commit it with the subject `revert: reapply \"{subject}\"`"),
            ));
        }
        applied.push(json!({"merge": merge, "revert": revert, "subject": subject}));
    }
    let mut text = vec![format!(
        "merge sync-member: {} {} (train {}); {} revert(s) of this member's work re-applied.",
        branch,
        if merged { "merged the train" } else { "already contains the train" },
        short(&train_tip),
        applied.len()
    )];
    text.extend(reverts.iter().map(|(m, r, s)| {
        format!(
            "  re-applied {} (it reverted the merge {}): revert: reapply \"{s}\"",
            short(r),
            short(m)
        )
    }));
    Ok(Report::new(
        json!({"branch": branch, "train": train_tip, "merged": merged, "reapplied": applied}),
        text.join("\n"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_message_has_the_subject_the_reland_check_reads_and_git_s_own_revert_line() {
        let m = reapply_message("Merge branch 'feat/x'", "abc1234");
        assert!(super::super::reland::is_reland_subject(m.lines().next().unwrap()), "{m}");
        assert_eq!(super::super::reverted_base::trailer_targets(&m), ["abc1234"]);
    }
}
