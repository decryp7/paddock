//! A member must be built on the train's base: the newest commit of the train's base line that the train has merged so far.
//!
//! The base line is declared by the train's own history, not by a second list (ER-42): a train stacked on another carries
//! re-merge commits `chore(train): merge train/<previous> (<sha>) into train N`, and the newest one on the train's first-parent
//! line names the line (`train/<previous>`) and, as its second parent, the commit of it merged so far. A train with none is cut
//! from main (the queue's `base` is main's commit, docs/plans/merge-queue.md): the base is then the merge base of the train and
//! `origin/main`. Other members merged ahead of this one are not required: members are merged one after another and never
//! contain each other.
//!
//! A member that lacks that commit is refused (`::error::`, exit 5) with the missing commit and `./dev merge sync-member`, which
//! merges the train and keeps the member's own reverted work. Nothing declares a base (no re-merge commit and no `origin/main`):
//! the check says so and does not guess ([`Base::Unknown`], reported, never silent: ER-33). A target that is not ahead of main
//! (a merge into main itself) has no train and no base to demand.

use super::git::{Git, short};
use super::reverted_base::branch_of;

/// The newest commits of the first-parent line that are read for a re-merge commit.
const DEPTH: &str = "2000";

/// What the train says its base is.
#[derive(Debug, PartialEq, Eq)]
pub enum Base {
    /// The commit every member must contain, and the line it belongs to (`train/<name>` or `main`).
    Commit { sha: String, line: String },
    /// Nothing declares a base: why.
    Unknown(String),
    /// The target is not ahead of main: a merge into main itself, not a train.
    NotATrain,
}

/// `Some(full sha)` of `name`, `None` when this clone has no such commit.
fn resolve(git: &Git, name: &str) -> Result<Option<String>, String> {
    let out = git.bytes_or_none(&["rev-parse", "--verify", "--quiet", &format!("{name}^{{commit}}")])?;
    let id = String::from_utf8_lossy(&out).trim().to_string();
    Ok((!id.is_empty()).then_some(id))
}

/// Whether `ancestor` is reachable from `of` (the merge base of the two is `ancestor`).
fn is_ancestor(git: &Git, ancestor: &str, of: &str) -> Result<bool, String> {
    Ok(git.text(&["merge-base", ancestor, of])?.trim() == ancestor)
}

/// The base of the train whose head is `head`.
pub fn base_of(git: &Git, head: &str) -> Result<Base, String> {
    let log = git.text(&["log", "--first-parent", "-n", DEPTH, "--format=%H%x00%P%x00%s", head])?;
    for rec in log.lines() {
        let mut f = rec.splitn(3, '\0');
        let (Some(_), Some(parents), Some(subject)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let parents: Vec<&str> = parents.split_whitespace().collect();
        let line = branch_of(subject);
        if parents.len() == 2 && line.starts_with("train/") && subject.contains("merge ") {
            return Ok(Base::Commit {
                sha: parents[1].to_string(),
                line,
            });
        }
    }
    let Some(main) = resolve(git, "origin/main")? else {
        return Ok(Base::Unknown(
            "the train has no re-merge commit of an earlier train and this clone has no origin/main to measure it from".into(),
        ));
    };
    if is_ancestor(git, head, &main)? {
        return Ok(Base::NotATrain);
    }
    let sha = git.text(&["merge-base", head, &main])?.trim().to_string();
    if sha.is_empty() {
        return Ok(Base::Unknown("the train and origin/main share no commit".into()));
    }
    Ok(Base::Commit { sha, line: "main".into() })
}

/// The refusal for a member that lacks the train's base; `None` when it has it. `train` and `member` are names for the message.
pub fn refusal(git: &Git, head: &str, member_commit: &str, train: &str, member: &str) -> Result<Option<String>, String> {
    match base_of(git, head)? {
        Base::Commit { sha, line } if !is_ancestor(git, &sha, member_commit)? => Ok(Some(format!(
            "::error::merge check: {member} is built on an old base: it does not contain {}, the newest commit of {line} that {train} has merged. Next step: run `./dev merge sync-member {train}` in the member's worktree (it merges the train and keeps the member's own reverted work), push, then check it again.",
            short(&sha)
        ))),
        _ => Ok(None),
    }
}

/// The notice for a train whose base nothing declares (the check did not judge), `None` otherwise.
pub fn unknown(git: &Git, head: &str) -> Result<Option<String>, String> {
    Ok(match base_of(git, head)? {
        Base::Unknown(why) => Some(format!(
            "::notice::stale-base check did not run (the base is not declared): {why}. Next step: `git fetch origin` so origin/main exists; members are not refused for an old base until it does."
        )),
        _ => None,
    })
}
