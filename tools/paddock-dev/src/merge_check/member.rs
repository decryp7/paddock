//! `./dev merge member <branch-or-sync-ref>`: the integrator's one way to merge a member into the train (user decision 100,
//! 2026-11-27). Run it in the train's worktree. Integrators had written the train merges by hand, with subjects of every shape
//! (`Merge X (sha, synced as ref) into train`), and the readers (sync-member's `is_own`, the trial reader, stale-base,
//! `--write-reverted`) parsed the member out of those subjects: three fix rounds. Now this makes the merge itself:
//!
//! - `git merge --no-ff` of the member's commit, with the fixed subject `Merge <member> into <train>` and trailers
//!   ([`super::trailers`]): `Paddock-Member`, `Paddock-Sync-Ref` (a sync ref `sync/<member>/<sha>` was given), `Paddock-Sync-Sha`
//!   (the full sha), the `Reverted:` lines `merge check --write-reverted` ties to a re-land, and for each `--reland <revert>` a
//!   `Paddock-Reland: <revert>` and a `Reverted:` line for each file the revert changed (a re-land of the member's own reverted
//!   merge is found from the history; `--reland` names any other);
//! - a conflict stops it with the merge in progress and the message prepared (git keeps it in `MERGE_MSG`); it prints the next step and
//!   never resolves anything;
//! - after the merge it runs `merge check` on the new commit and returns its answer.
//!
//! It refuses a dirty tree, a merge already in progress and a detached HEAD before it changes anything.

use super::reverted_base::resolve;
use super::{Request, trailers};
use crate::git::{Git, short};
use paddock_cli::{CliError, Report};
use paddock_proc::{Limits, run_bounded};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The longest the merge may take (hooks included): a hang detector, not a speed limit (ER-12).
const TIMEOUT: Duration = Duration::from_secs(600);

/// The command line's options.
pub struct Options<'a> {
    /// `--reland <revert>` (repeatable): a revert of the train's merge of this member that the member re-applies, besides the ones
    /// found in its history ([`super::reland::own_relands`]).
    pub relands: &'a [String],
    /// `--old-base-anyway <reason>`: handed to the merge check.
    pub old_base_anyway: Option<&'a str>,
}

/// Merges the member into the train in HEAD, then checks the merge.
pub fn command(root: Option<&Path>, member: &str, opts: &Options) -> Result<Report, CliError> {
    if opts.old_base_anyway.is_some_and(|r| r.trim().is_empty()) {
        return Err(CliError::usage("--old-base-anyway needs a reason.")
            .with_hint("Say why, for example --old-base-anyway \"hotfix, the newer main work is unrelated\"."));
    }
    let root = crate::repo_root::repo_root(root)?;
    let git = Git::new(&root);
    let step = |why: String, next: &str| CliError::failure(format!("::error::merge member: {why}. Next step: {next}"));
    let train = git
        .text(&["symbolic-ref", "--quiet", "--short", "HEAD"])
        .map(|b| b.trim().to_string())
        .map_err(|_| {
            step(
                "HEAD is not on a branch, so the train is unknown".into(),
                "check out the train's branch in its worktree",
            )
        })?;
    let at = |what: &str| -> Result<Option<String>, CliError> {
        resolve(&git, what).map_err(|e| step(e, "run `git status` to see what is wrong with the clone"))
    };
    if at("MERGE_HEAD")?.is_some() {
        return Err(step(
            "a merge is already in progress".into(),
            "finish it (`git commit`) or undo it (`git merge --abort`), then run this again",
        ));
    }
    let dirty = git
        .text(&["status", "--porcelain", "--untracked-files=no"])
        .map_err(|e| step(e.to_string(), "run `git status`"))?;
    if !dirty.trim().is_empty() {
        return Err(step(
            format!("the worktree has uncommitted changes ({} path(s))", dirty.lines().count()),
            "commit them in a branch of their own, then run this again; a train merge starts from a clean tree",
        ));
    }
    // The member: the name as given, else the remote's.
    let sha = match at(member)? {
        Some(s) => s,
        None => at(&format!("origin/{member}"))?.ok_or_else(|| {
            step(
                format!("no branch or ref named {member}"),
                "run `git fetch origin` and give the member's branch or its `sync/<member>/<sha>` ref",
            )
        })?,
    };
    let who = trailers::named(member);
    let mut relands = Vec::new();
    let mut reland_lines = Vec::new();
    // A revert of the member's own earlier merge that the member re-adds is found from the history; `--reland` names any other.
    let found = super::reland::own_relands(&git, "HEAD", &sha, &who.branch)
        .map_err(|e| step(e, "run `git log HEAD..<member>` to see why, or name the revert with --reland"))?;
    let given = opts
        .relands
        .iter()
        .map(|given| {
            let full = at(given)?.ok_or_else(|| {
                CliError::usage(format!("--reland {given} is not a commit."))
                    .with_hint("Give the revert commit the train made of the member's merge (`./dev merge sync-member <train>` names it).")
            })?;
            if !git.is_ancestor(&full, "HEAD").map_err(|e| step(e, "run `git status`"))? {
                return Err(CliError::usage(format!("--reland {given} is not part of {train}."))
                    .with_hint("Give a revert commit that is on the train in HEAD."));
            }
            Ok(full)
        })
        .collect::<Result<Vec<_>, CliError>>()?;
    for full in found.into_iter().chain(given) {
        if relands.contains(&full) {
            continue;
        }
        let files = git
            .paths(&[
                "diff-tree",
                "--no-commit-id",
                "--name-only",
                "-r",
                "--no-renames",
                "-z",
                "--root",
                &full,
            ])
            .map_err(|e| step(e.to_string(), "run `git show` on the revert"))?;
        reland_lines.extend(trailers::reland_lines(&full, &files));
        relands.push(full);
    }
    // The `Reverted:` lines the check would ask for, found the way `--write-reverted` finds them, on the merge as it would be made.
    // A merge that conflicts has no result to read: the lines are then left to the check after the person's commit.
    let mut reverted = Vec::new();
    let preview = Request {
        root: Some(root.clone()),
        commit: None,
        branch: Some(sha.clone()),
        target: Some("HEAD".into()),
        generators: false,
        heavy_generators: false,
        gates: false,
        write_reverted: None,
        old_base_anyway: None,
        no_trial: None,
        gates_base: None,
    };
    if let Ok(merge) = super::resolve(&git, &preview) {
        let analysis = super::analyze(&git, &merge);
        if let Ok(lines) = super::reland::lines(&git, &merge, &analysis.found) {
            reverted = lines;
        }
    }
    for l in reland_lines {
        if !reverted.contains(&l) {
            reverted.push(l);
        }
    }
    let before = at("HEAD")?.unwrap_or_default();
    let mut lines = trailers::member_lines(&who, &sha, &relands);
    lines.extend(reverted.iter().cloned());
    let text = trailers::with_block(&format!("Merge {} into {train}", who.branch), &lines);
    let file = message_file(&git, &root).map_err(|e| step(e, "check that the repository's git folder is writable"))?;
    std::fs::write(&file, &text).map_err(|e| {
        step(
            format!("cannot write {}: {e}", file.display()),
            "check that the git folder is writable",
        )
    })?;
    let mut cmd = crate::git_env::git();
    cmd.arg("-C").arg(&root).args(["merge", "--no-ff", "-F"]).arg(&file).arg(&sha);
    let ran = run_bounded(cmd, Limits::new(TIMEOUT));
    let _ = std::fs::remove_file(&file);
    let out = ran.map_err(|e| step(format!("git merge could not run: {e}"), "run `git status`"))?;
    if !out.success() {
        if at("MERGE_HEAD")?.is_some() {
            return Err(CliError::new(
                paddock_cli::Exit::Findings,
                format!(
                    "::error::merge member: {} conflicts with {train}; the merge is in progress and its message is prepared (subject and trailers, in .git/MERGE_MSG). \
Next step: resolve the conflicts yourself, `git add` the files, `git commit` (the prepared message is used; add `Reverted:` lines to the trailer block if the check asks for them), \
then run `./dev merge check HEAD`; or `git merge --abort`.\n{}",
                    who.branch,
                    out.stdout_text().trim_end()
                ),
            ));
        }
        return Err(step(
            format!("git merge failed: {}", out.stderr_text().trim()),
            "fix what git names (a hook's message is above), then run this again; nothing was merged",
        ));
    }
    let head = at("HEAD")?.unwrap_or_default();
    let said = format!(
        "merge member: merged {} ({}) into {train} as {} ({} Reverted line(s), {} Paddock-Reland); undo with `git reset --hard {}` if the check refuses it.",
        who.branch,
        short(&sha),
        short(&head),
        reverted.len(),
        relands.len(),
        short(&before)
    );
    let checked = super::command(&Request {
        root: Some(root),
        commit: Some(head.clone()),
        branch: None,
        target: None,
        generators: true,
        heavy_generators: false,
        gates: false,
        write_reverted: None,
        old_base_anyway: opts.old_base_anyway.map(str::to_string),
        no_trial: None,
        gates_base: None,
    });
    let data =
        json!({"member": who.branch, "sync_ref": who.sync_ref, "sync_sha": sha, "merge": head, "before": before, "relands": relands});
    match checked {
        Ok(mut report) => {
            report.text = format!("{said}\n{}", report.text);
            report.data["member"] = data;
            Ok(report)
        }
        Err(mut e) => {
            e.message = format!("{said}\n{}", e.message);
            Err(e)
        }
    }
}

/// The file the message is written to for `git merge -F`: under the checkout's own git folder, which this command owns while it runs.
fn message_file(git: &Git, root: &Path) -> Result<PathBuf, String> {
    let p = git.text(&["rev-parse", "--git-path", "paddock-merge-member-msg"])?;
    let p = PathBuf::from(p.trim());
    Ok(if p.is_absolute() { p } else { root.join(p) })
}
