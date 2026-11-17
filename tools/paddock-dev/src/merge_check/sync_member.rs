//! `./dev merge sync-member <train>`: lists what the author re-applies after merging the train into a member branch (read-only).
//!
//! The failure it prevents (three times, the last c156ba72a): the train merged a member and reverted it, and the author then
//! merged the train into the member branch. Git takes the train's revert as the newer change, so the member's own work is deleted
//! again. Common practice for it is git's howto "revert a faulty merge": revert the revert after the merge of the train. This
//! prints those commands and runs nothing, so no step can half-finish and no conflict can lose work.
//!
//! Run it BEFORE `git merge <train>`: it reads `member..train`, and a merged revert is no longer in that range. It lists
//!
//! - for each live train revert of a merge of this member's own branch (`branch_of(subject)` equals the branch, with or without a
//!   remote prefix such as `origin/`): the `git revert <revert>` to run after the merge, oldest revert first;
//! - for a revert of another branch's merge that this member holds: a `::warning::` (re-base on the train, or land that branch
//!   first): that work is not this member's to re-apply.

use super::git::{Git, short};
use super::reverted_base::live_for_member;
use paddock_cli::{CliError, Report};
use serde_json::json;
use std::path::Path;

/// Whether the merge of `branch` (from a train merge subject) is the member's own, with or without a remote prefix.
pub fn is_own(branch: &str, member: &str) -> bool {
    let strip = |s: &str| {
        s.split_once('/')
            .filter(|(remote, _)| *remote == "origin")
            .map_or(s.to_string(), |(_, r)| r.to_string())
    };
    strip(branch) == strip(member)
}

/// The member's branch name: `member` as given, else the branch HEAD is on.
fn member_name(git: &Git, member: Option<&str>) -> Result<String, String> {
    if let Some(m) = member {
        return Ok(m.to_string());
    }
    git.text(&["symbolic-ref", "--quiet", "--short", "HEAD"])
        .map(|b| b.trim().to_string())
        .map_err(|_| "HEAD is not on a branch, so the member's name is unknown".to_string())
}

/// Lists the commands and warnings; runs nothing.
pub fn command(root: Option<&Path>, train: &str, member: Option<&str>) -> Result<Report, CliError> {
    let root = crate::gates::command::repo_root(root)?;
    let git = Git::new(&root);
    let step = |why: String, next: &str| CliError::failure(format!("::error::merge sync-member: {why}. Next step: {next}"));
    let name = member_name(&git, member).map_err(|e| step(e, "check out the member's branch, or pass --member <branch>"))?;
    let train_tip = git
        .text(&["rev-parse", "--verify", &format!("{train}^{{commit}}")])
        .map(|t| t.trim().to_string())
        .map_err(|e| {
            step(
                e,
                "run `git fetch origin` and give the train's full ref, for example origin/train/<date>-<n>",
            )
        })?;
    let head = git
        .text(&["rev-parse", "--verify", "HEAD^{commit}"])
        .map(|t| t.trim().to_string())
        .map_err(|e| step(e, "run it inside the member's worktree"))?;
    let reverts = live_for_member(&git, &train_tip, &head).map_err(|e| step(e, "fix what is named, then run it again"))?;
    let (own, other): (Vec<_>, Vec<_>) = reverts.into_iter().partition(|(_, _, _, branch)| is_own(branch, &name));
    let mut text = vec![format!(
        "merge sync-member: {name} against {} ({}): {} revert(s) of this member's own merges to re-apply, {} of other branches.",
        train,
        short(&train_tip),
        own.len(),
        other.len()
    )];
    if !own.is_empty() {
        text.push(format!(
            "Merge the train first (`git merge {train}`), then re-apply the member's work, oldest first (git's howto \"revert a faulty merge\": revert the revert):"
        ));
        text.extend(
            own.iter()
                .map(|(m, r, s, _)| format!("  git revert {r}   # {} reverted the merge {} \"{s}\"", short(r), short(m))),
        );
    }
    for (m, r, s, branch) in &other {
        text.push(format!(
            "::warning::{} reverted the merge {} of {branch} (\"{s}\"), work this member holds but did not make: re-base on the train, or land {branch} first.",
            short(r),
            short(m)
        ));
    }
    let item = |(m, r, s, b): &(String, String, String, String)| json!({"merge": m, "revert": r, "subject": s, "branch": b});
    Ok(Report::new(
        json!({"member": name, "train": train_tip, "own": own.iter().map(item).collect::<Vec<_>>(), "other": other.iter().map(item).collect::<Vec<_>>()}),
        text.join("\n"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_branch_is_the_members_own_with_or_without_the_remote_prefix_and_no_other() {
        assert!(is_own("feat/x", "feat/x"));
        assert!(is_own("feat/x", "origin/feat/x"));
        assert!(is_own("origin/feat/x", "feat/x"));
        assert!(!is_own("feat/y", "feat/x"));
        assert!(!is_own("feat/x2", "feat/x"));
        assert!(!is_own("fix/feat/x", "feat/x"));
    }
}
