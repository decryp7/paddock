//! The commands that change a worktree's ownership or existence: `add`, `claim`, `reassign`, `release` and `drop`
//! (ADR 0083 §1). Each runs its read-check-write under the owner map's lock, so a refusal never leaves half a change.

use super::git::{Git, GitError, Repo, WorktreeInfo, key, worktrees};
use super::note::{Note, check_owner, format_utc, now_secs};
use super::owners::{Claim, OwnerError, OwnerMap, OwnerStore, same_path};
use paddock_cli::{CliError, Report};
use serde_json::json;
use std::path::Path;

/// The caller of a command: a named agent, or the coordinator (no agent id).
#[derive(Debug, Clone, Default)]
pub struct Caller {
    /// The agent id and type, when the caller is an agent.
    pub agent: Option<Agent>,
}

/// An agent's identity as the Claude Code hook reports it.
#[derive(Debug, Clone)]
pub struct Agent {
    /// `agent_id`.
    pub id: String,
    /// `agent_type`, possibly empty.
    pub kind: String,
}

impl Caller {
    /// From the arguments, else from `PADDOCK_AGENT_ID` and `PADDOCK_AGENT_TYPE` (read through `env`).
    pub fn resolve(id: Option<&str>, kind: Option<&str>, env: &dyn Fn(&str) -> Option<String>) -> Result<Caller, CliError> {
        let id = id.map(str::to_string).or_else(|| env("PADDOCK_AGENT_ID")).filter(|s| !s.is_empty());
        let Some(id) = id else { return Ok(Caller::default()) };
        let ok = id.len() <= 128
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'@' | b'-'));
        if !ok {
            return Err(CliError::usage(format!("The agent id `{id}` is not valid."))
                .with_hint("Use the agent_id the hook reports: letters, digits and `._:@-`, at most 128 characters."));
        }
        let kind = kind.map(str::to_string).or_else(|| env("PADDOCK_AGENT_TYPE")).unwrap_or_default();
        let kind = kind
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            .take(40)
            .collect();
        Ok(Caller {
            agent: Some(Agent { id, kind }),
        })
    }

    fn by(&self) -> String {
        match &self.agent {
            Some(a) => format!("agent:{}", a.id),
            None => "coordinator".to_string(),
        }
    }
}

pub(super) fn git_failure(e: GitError) -> CliError {
    CliError::failure(e.to_string())
}

pub(super) fn owner_failure(e: OwnerError) -> CliError {
    CliError::failure(e.to_string())
}

/// `add`'s request.
pub struct AddRequest<'a> {
    /// Where the worktree goes.
    pub path: &'a Path,
    /// The new branch, or `None` for a detached copy.
    pub branch: Option<&'a str>,
    /// The commit-ish to start from.
    pub base: &'a str,
    /// `<role>:<task>`.
    pub owner: &'a str,
    /// Folders for a sparse cone checkout.
    pub sparse: &'a [String],
}

fn known<'a>(list: &'a [WorktreeInfo], k: &str) -> Option<&'a WorktreeInfo> {
    list.iter().find(|w| same_path(&w.path.replace('\\', "/"), k))
}

/// The worktree at `path` as git lists it, never the primary checkout.
fn find_secondary(repo: &Repo, path: &Path) -> Result<(WorktreeInfo, String), CliError> {
    let k = key(path);
    let list = worktrees(repo).map_err(git_failure)?;
    match known(&list, &k) {
        Some(w) if w.path != list[0].path => Ok((w.clone(), k)),
        Some(_) => Err(CliError::refused(format!("{k} is the primary checkout, not an agent worktree."))
            .with_hint("Work in a worktree made with `./dev worktree add`.")),
        None => Err(CliError::refused(format!("{k} is not a worktree of this repository."))
            .with_hint("`./dev worktree list` shows the worktrees; make one with `./dev worktree add`.")),
    }
}

fn require_locked(w: &WorktreeInfo, k: &str) -> Result<(), CliError> {
    if w.locked.is_some() {
        Ok(())
    } else {
        Err(CliError::refused(format!("{k} is not locked, so it has no owner."))
            .with_hint("Agent worktrees are locked when `./dev worktree add` makes them; ask the coordinator."))
    }
}

/// `add`: `git worktree add --lock --reason <owner note>`; refuses an existing path or one the owner map has.
pub fn add(repo: &Repo, caller: &Caller, req: &AddRequest<'_>) -> Result<Report, CliError> {
    let (role, task) =
        check_owner(req.owner).map_err(|m| CliError::usage(m).with_hint("Write --owner implementer:RT-3 (role, a colon, task)."))?;
    let k = key(req.path);
    let git = repo.git();
    let base_sha = git
        .text(&["rev-parse", "--verify", "--quiet", &format!("{}^{{commit}}", req.base)])
        .map_err(|_| {
            CliError::usage(format!("The base `{}` is not a commit here.", req.base))
                .with_hint("Fetch it (`git fetch origin`) or name another base.")
        })?
        .trim()
        .to_string();
    let branch_name = req.branch.unwrap_or("detached");
    let note = Note {
        role,
        task,
        branch: branch_name.to_string(),
        base: base_sha.clone(),
        created: now_secs(),
        by: caller.by(),
    }
    .render();
    let store = OwnerStore::open(&repo.common_dir);
    store
        .update(|map| -> Result<(), CliError> {
            if let Some(e) = map.get(&k) {
                return Err(
                    CliError::refused(format!("{k} is already in the owner map (agent {}).", e.agent_id))
                        .with_hint("Pick a new path; the coordinator frees an old entry with `./dev worktree reassign`."),
                );
            }
            if req.path.symlink_metadata().is_ok() {
                return Err(
                    CliError::refused(format!("{k} already exists.")).with_hint("Pick a new path; never reuse another agent's folder.")
                );
            }
            let mut args = vec!["worktree", "add", "--lock", "--reason", note.as_str()];
            match req.branch {
                Some(b) => args.extend(["-b", b, k.as_str(), base_sha.as_str()]),
                None => args.extend(["--detach", k.as_str(), base_sha.as_str()]),
            }
            git.text(&args).map_err(git_failure)?;
            if !req.sparse.is_empty() {
                let mut s = vec!["sparse-checkout", "set", "--cone"];
                s.extend(req.sparse.iter().map(String::as_str));
                Git::at(Path::new(&k)).text(&s).map_err(|e| {
                    CliError::failure(format!("The worktree {k} was made (locked) but the sparse checkout failed: {e}"))
                        .with_hint("Fix the folder names and run `git sparse-checkout set --cone ...` in it.")
                })?;
            }
            if let Some(a) = &caller.agent {
                map.claim(&k, &a.id, &a.kind, &format_utc(now_secs()));
            }
            Ok(())
        })
        .map_err(owner_failure)??;
    let head = Git::at(Path::new(&k))
        .text(&["rev-parse", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or(base_sha);
    Ok(Report::new(
        json!({"path": k, "branch": req.branch, "head": head, "owner": req.owner, "locked": true, "note": note,
               "claimed_by": caller.agent.as_ref().map(|a| a.id.clone())}),
        format!(
            "Made {k} ({}), locked: {note}",
            req.branch.map_or("detached".to_string(), |b| format!("branch {b}"))
        ),
    ))
}

fn need_agent(caller: &Caller, what: &str) -> Result<Agent, CliError> {
    caller.agent.clone().ok_or_else(|| {
        CliError::usage(format!("`{what}` needs the agent's id."))
            .with_hint("Pass --agent <agent_id> (the guard hook passes it), or set PADDOCK_AGENT_ID.")
    })
}

/// `claim`: binds the locked worktree to the calling agent.
pub fn claim(repo: &Repo, caller: &Caller, path: &Path) -> Result<Report, CliError> {
    let agent = need_agent(caller, "worktree claim")?;
    let store = OwnerStore::open(&repo.common_dir);
    let (k, outcome) = store
        .update(|map| -> Result<(String, Claim), CliError> {
            let (w, k) = find_secondary(repo, path)?;
            require_locked(&w, &k)?;
            let c = map.claim(&k, &agent.id, &agent.kind, &format_utc(now_secs()));
            Ok((k, c))
        })
        .map_err(owner_failure)??;
    match outcome {
        Claim::Granted | Claim::Already => Ok(Report::new(
            json!({"path": k, "agent_id": agent.id, "already": outcome == Claim::Already}),
            format!("{k} is claimed by {}.", agent.id),
        )),
        Claim::Held(e) => Err(CliError::refused(format!("{k} is claimed by {}.", e.agent_id))
            .with_hint(format!("The coordinator reassigns it with `./dev worktree reassign {k}`."))),
    }
}

/// `reassign`: the coordinator clears the owner entry and keeps the lock.
pub fn reassign(repo: &Repo, caller: &Caller, path: &Path) -> Result<Report, CliError> {
    if caller.agent.is_some() {
        return Err(CliError::refused("Only the coordinator reassigns a worktree.")
            .with_hint("Ask the coordinator; an agent never frees another agent's worktree."));
    }
    let k = key(path);
    let store = OwnerStore::open(&repo.common_dir);
    let (previous, locked) = store
        .update(|map| -> Result<(Option<String>, bool), CliError> {
            let list = worktrees(repo).map_err(git_failure)?;
            let w = known(&list, &k);
            let prev = map.get(&k).map(|e| e.agent_id.clone());
            if w.is_none() && prev.is_none() {
                return Err(CliError::refused(format!("{k} is not a worktree and has no owner entry."))
                    .with_hint("`./dev worktree list` shows the worktrees."));
            }
            map.remove(&k);
            Ok((prev, w.is_some_and(|w| w.locked.is_some())))
        })
        .map_err(owner_failure)??;
    let said = previous
        .as_deref()
        .map_or("had no owner".to_string(), |p| format!("was claimed by {p}"));
    Ok(Report::new(
        json!({"path": k, "previous": previous, "locked": locked}),
        format!("{k} {said}; it is free to claim{}.", if locked { " and stays locked" } else { "" }),
    ))
}

/// `release`: unlocks the worktree and forgets its owner. The owner, or the coordinator after the agent ended.
pub fn release(repo: &Repo, caller: &Caller, path: &Path) -> Result<Report, CliError> {
    let store = OwnerStore::open(&repo.common_dir);
    let k = store
        .update(|map| -> Result<String, CliError> {
            let (w, k) = find_secondary(repo, path)?;
            require_locked(&w, &k)?;
            authorize_owner(map, caller, &k, "release")?;
            repo.git().text(&["worktree", "unlock", &k]).map_err(git_failure)?;
            map.remove(&k);
            Ok(k)
        })
        .map_err(owner_failure)??;
    Ok(Report::new(
        json!({"path": k, "locked": false}),
        format!("Released {k}: unlocked and no owner."),
    ))
}

/// The owner, or the coordinator, may act on `k`.
fn authorize_owner(map: &OwnerMap, caller: &Caller, k: &str, what: &str) -> Result<(), CliError> {
    match (&caller.agent, map.get(k)) {
        (None, _) => Ok(()),
        (Some(a), Some(e)) if e.agent_id == a.id => Ok(()),
        (Some(_), Some(e)) => Err(
            CliError::refused(format!("{k} belongs to agent {}; only its owner can {what} it.", e.agent_id))
                .with_hint("Work in your own worktree; the coordinator frees one after its agent ended."),
        ),
        (Some(_), None) => Err(
            CliError::refused(format!("{k} is not claimed by any agent; only the coordinator can {what} it."))
                .with_hint("Claim your own worktree with `./dev worktree claim <path>`."),
        ),
    }
}

/// `drop`: removes a worktree its owner claimed (a review copy). Refuses unpushed commits and uncommitted changes.
pub fn drop_own(repo: &Repo, caller: &Caller, path: &Path) -> Result<Report, CliError> {
    let agent = need_agent(caller, "worktree drop")?;
    let store = OwnerStore::open(&repo.common_dir);
    let k = store
        .update(|map| -> Result<String, CliError> {
            let (w, k) = find_secondary(repo, path)?;
            match map.get(&k) {
                Some(e) if e.agent_id == agent.id => {}
                Some(e) => {
                    return Err(CliError::refused(format!(
                        "{k} belongs to agent {}; you can only drop your own worktree.",
                        e.agent_id
                    )));
                }
                None => {
                    return Err(CliError::refused(format!("{k} is not claimed by you."))
                        .with_hint("`drop` removes only a worktree you claimed yourself."));
                }
            }
            let at = Git::at(Path::new(&k));
            let dirty = at.text(&["status", "--porcelain"]).map_err(git_failure)?;
            if !dirty.trim().is_empty() {
                return Err(
                    CliError::refused(format!("{k} has uncommitted changes or untracked files; nothing was removed."))
                        .with_hint("Commit and push them, or delete the files you do not need, then drop again."),
                );
            }
            let unpushed = at
                .text(&["rev-list", "--count", "HEAD", "--not", "--remotes"])
                .map_err(git_failure)?;
            if unpushed.trim() != "0" {
                return Err(CliError::refused(format!(
                    "{k} has {} commit(s) that are on no remote; nothing was removed.",
                    unpushed.trim()
                ))
                .with_hint("Push them (or cherry-pick what you need elsewhere), then drop again."));
            }
            if let Some(reason) = &w.locked {
                repo.git().text(&["worktree", "unlock", &k]).map_err(git_failure)?;
                if let Err(e) = repo.git().text(&["worktree", "remove", &k]) {
                    let _ = repo.git().text(&["worktree", "lock", "--reason", reason, &k]);
                    return Err(git_failure(e).with_hint("The lock was put back; nothing was removed."));
                }
            } else {
                repo.git().text(&["worktree", "remove", &k]).map_err(git_failure)?;
            }
            map.remove(&k);
            Ok(k)
        })
        .map_err(owner_failure)??;
    Ok(Report::new(json!({"path": k, "removed": true}), format!("Dropped {k}.")))
}
