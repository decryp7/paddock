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
//!
//! Run it AFTER the merge too: with the train in HEAD (the sync tip), it also runs the fast static rows on that tip
//! ([`static_rows`]: conformance, the gate register, Clippy and the Windows Clippy of the workspaces the member touches, through
//! `check --pre-push --only ... --base <train>`), prints each row, and a failing or unmeasured row is exit 5 or 6 with one `::error::`
//! ("fix on the member, then re-sync"). `--no-static "<reason>"` skips them and logs the reason. Once they pass it dispatches the
//! member's own CI trial ([`trial_dispatch`]: the member's affected jobs on the train, the run id printed; `--no-trial "<reason>"`
//! skips it). The command changes no file of the checkout; the trial pushes only the scratch ref `trial/<member>`.

use super::git::{Git, short};
use super::reverted_base::live_for_member;
use super::static_rows::{self, StaticGates};
use super::trial_dispatch::{self, Dispatcher};
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

/// The member's tip before it merged the train: the first parent of the oldest merge on HEAD's first-parent line that brought the
/// train in (the train is an ancestor of its second parent and not of its first). `HEAD` itself when no merge did (a fast-forward).
fn pre_merge_tip(git: &Git, train: &str, head: &str) -> Result<String, String> {
    let log = git.text(&["log", "--first-parent", "--merges", "--format=%H %P", head])?;
    let mut found = None;
    for line in log.lines() {
        let ids: Vec<&str> = line.split_whitespace().collect();
        if let [_, first, second, ..] = ids.as_slice()
            && git.is_ancestor(train, second)?
            && !git.is_ancestor(train, first)?
        {
            found = Some((*first).to_string());
        }
    }
    Ok(found.unwrap_or_else(|| head.to_string()))
}

/// The escapes and the trial's wait, from the command line.
pub struct Options<'a> {
    /// `--no-static <reason>`: skip the static rows.
    pub no_static: Option<&'a str>,
    /// `--no-trial <reason>`: do not dispatch the member's CI trial.
    pub no_trial: Option<&'a str>,
    /// `--trial-max <duration>`: the longest the dispatch waits for a busy runner.
    pub trial_max: &'a str,
}

/// Lists the commands and warnings, then (the train merged) runs the static rows on the sync tip and, once they pass, dispatches
/// the member's own CI trial.
pub fn command(root: Option<&Path>, train: &str, member: Option<&str>, opts: &Options) -> Result<Report, CliError> {
    command_with(root, train, member, opts, &static_rows::Real, &trial_dispatch::Real)
}

/// [`command`] with the static rows' runner and the dispatcher given (the tests hand stand-ins).
fn command_with(
    root: Option<&Path>,
    train: &str,
    member: Option<&str>,
    opts: &Options,
    rows: &dyn StaticGates,
    dispatcher: &dyn Dispatcher,
) -> Result<Report, CliError> {
    let (no_static, no_trial) = (opts.no_static, opts.no_trial);
    if no_trial.is_some_and(|r| r.trim().is_empty()) {
        return Err(CliError::usage("--no-trial needs a reason.")
            .with_hint("Say why, for example --no-trial \"docs-only member, no job of its own to run\"."));
    }
    if no_static.is_some_and(|r| r.trim().is_empty()) {
        return Err(CliError::usage("--no-static needs a reason.")
            .with_hint("Say why, for example --no-static \"docs-only sync, the rows ran on the last tip\"."));
    }
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
    // After `git merge <train>` the train is in HEAD and `member..train` is empty: read from the train merge's first parent, the
    // member's pre-merge tip, and leave out what a later commit of HEAD already re-applied.
    let (basis, after) = if git.is_ancestor(&train_tip, &head).map_err(|e| step(e, "run `git status`"))? {
        (
            pre_merge_tip(&git, &train_tip, &head).map_err(|e| step(e, "run `git log --first-parent` to find the merge"))?,
            true,
        )
    } else {
        (head.clone(), false)
    };
    let mut reverts = live_for_member(&git, &train_tip, &basis).map_err(|e| step(e, "fix what is named, then run it again"))?;
    let mut done = 0;
    if after {
        let mut open = Vec::new();
        for r in reverts {
            let hits = git
                .text(&[
                    "log",
                    "-n",
                    "1",
                    "--format=%H",
                    "--fixed-strings",
                    "--grep",
                    &format!("This reverts commit {}", r.1),
                    &format!("{train_tip}..{head}"),
                ])
                .map_err(|e| step(e, "run `git log` to see why"))?;
            if hits.trim().is_empty() {
                open.push(r);
            } else {
                done += 1;
            }
        }
        reverts = open;
    }
    let (own, other): (Vec<_>, Vec<_>) = reverts.into_iter().partition(|(_, _, _, branch)| is_own(branch, &name));
    let mut text = vec![format!(
        "merge sync-member: {name} against {} ({}): {} revert(s) of this member's own merges to re-apply, {} of other branches.{}",
        train,
        short(&train_tip),
        own.len(),
        other.len(),
        if after {
            format!(
                " The train is already merged: read from its merge's first parent {}; {done} already re-applied.",
                short(&basis)
            )
        } else {
            String::new()
        }
    )];
    if !own.is_empty() {
        let lead = if after {
            "The train is merged; re-apply the member's work, oldest first".to_string()
        } else {
            format!("Merge the train first (`git merge {train}`), then re-apply the member's work, oldest first")
        };
        text.push(format!("{lead} (git's howto \"revert a faulty merge\": revert the revert):"));
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
    let step = match (no_static, after) {
        (Some(reason), _) => static_rows::skipped(reason.trim()),
        (None, false) => static_rows::not_yet(train),
        (None, true) => static_rows::interpret(rows.run(&root, &train_tip), short(&train_tip)),
    };
    text.extend(step.text.iter().cloned());
    // The trial follows the static rows: only a sync tip they passed (or that `--no-static` let through, logged) is worth a CI run.
    let trial = match (no_trial, after, step.exit) {
        (Some(reason), _, _) => trial_dispatch::skipped(reason.trim()),
        (None, false, _) => trial_dispatch::not_run("the train is not merged into HEAD yet; run this again after `git merge`"),
        (None, true, e) if e != paddock_cli::Exit::Ok => trial_dispatch::not_run("the static rows did not pass: fix them first"),
        (None, true, _) => {
            let branch = name.strip_prefix("origin/").unwrap_or(&name).to_string();
            // The trial merges what origin holds, so the sync tip goes there first, as the scratch ref `sync/<member>` (the integrator's
            // push; the member's own branch is the author's). `--force-with-lease` on that ref: a re-sync replaces the integrator's own earlier tip,
            // never one that someone else moved (the push is then refused as stale: fetch it, look, run again). The trial is then of `sync/<member>`, and `merge check` reads that name.
            let synced = trial_dispatch::sync_ref(&branch);
            match git.text(&[
                "push",
                "--quiet",
                &format!("--force-with-lease=refs/heads/{synced}"),
                "origin",
                &format!("HEAD:refs/heads/{synced}"),
            ]) {
                Ok(_) => trial_dispatch::interpret(dispatcher.dispatch(&root, &synced, train, opts.trial_max), &synced, train),
                Err(why) => trial_dispatch::not_pushed(&synced, &why),
            }
        }
    };
    text.extend(trial.text.iter().cloned());
    let exit = if step.exit != paddock_cli::Exit::Ok {
        step.exit
    } else {
        trial.exit
    };
    let item = |(m, r, s, b): &(String, String, String, String)| json!({"merge": m, "revert": r, "subject": s, "branch": b});
    Ok(Report::new(
        json!({"member": name, "train": train_tip, "own": own.iter().map(item).collect::<Vec<_>>(), "other": other.iter().map(item).collect::<Vec<_>>(), "static": step.data, "trial": trial.data}),
        text.join("\n"),
    )
    .with_exit(exit))
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
