//! The commands that change a worktree's ownership or existence: `add`, `claim`, `reassign`, `release` and `drop`
//! (ADR 0083 §1). Each runs its read-check-write under the owner map's lock, so a refusal never leaves half a change.

use super::git::{Git, GitError, Repo, WorkState, WorktreeInfo, is_under, key, worktrees};
use super::note::{Note, check_owner, format_utc, now_secs};
use super::owners::{Claim, OwnerError, OwnerMap, OwnerStore, PENDING_TTL, same_path};
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
    /// The folder name, one path component under `root`.
    pub name: &'a str,
    /// The folder every agent worktree goes under.
    pub root: &'a Path,
    /// The new branch, or `None` for a detached copy.
    pub branch: Option<&'a str>,
    /// The commit-ish to start from.
    pub base: &'a str,
    /// `<role>:<task>`.
    pub owner: &'a str,
    /// Folders for a sparse cone checkout.
    pub sparse: &'a [String],
}

/// What a requested branch name refers to: nothing yet, a local branch, or a branch only a remote has.
enum Attach {
    New,
    /// A local branch, with its tip.
    Local(String),
    /// A remote-tracking ref such as `origin/fix/x`, with its tip; a local branch tracking it is made.
    Remote(String, String),
}

impl Attach {
    fn find(git: &Git, branch: &str) -> Result<Attach, CliError> {
        if let Ok(sha) = git.text(&["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}^{{commit}}")]) {
            return Ok(Attach::Local(sha.trim().to_string()));
        }
        let refs = git
            .text(&["for-each-ref", "--format=%(refname:strip=2) %(objectname)", "refs/remotes"])
            .map_err(git_failure)?;
        let mut found: Vec<(String, String)> = refs
            .lines()
            .filter_map(|l| l.split_once(' '))
            .filter(|(name, _)| name.split_once('/').is_some_and(|(_, b)| b == branch))
            .map(|(n, s)| (n.to_string(), s.to_string()))
            .collect();
        if found.len() > 1 {
            found.retain(|(n, _)| n.starts_with("origin/"));
        }
        match found.len() {
            0 => Ok(Attach::New),
            1 => {
                let (n, s) = found.remove(0);
                Ok(Attach::Remote(n, s))
            }
            _ => Err(CliError::usage(format!("The branch `{branch}` exists on several remotes."))
                .with_hint("Make a local branch of the one you mean first (`git branch <name> <remote>/<name>`), then run add again.")),
        }
    }

    /// For an existing branch: how far it is from `--base` and from its remote, refusing a branch that is already merged
    /// into the base or that is behind its remote (both are almost always a stale name, and work would start on old code).
    fn drift(&self, git: &Git, base: &str, base_sha: &str, branch: &str) -> Result<Option<Drift>, CliError> {
        let tip = match self {
            Attach::New => return Ok(None),
            Attach::Local(t) | Attach::Remote(_, t) => t.clone(),
        };
        let count = |a: &str, b: &str| -> Result<(u64, u64), CliError> {
            let out = git
                .text(&["rev-list", "--left-right", "--count", &format!("{a}...{b}")])
                .map_err(git_failure)?;
            let mut it = out.split_whitespace().map(|n| n.parse::<u64>().unwrap_or(0));
            Ok((it.next().unwrap_or(0), it.next().unwrap_or(0)))
        };
        let (behind_base, ahead_base) = count(base_sha, &tip)?;
        if ahead_base == 0 && behind_base > 0 {
            return Err(CliError::refused(format!(
                "The branch `{branch}` is already merged into {base} ({behind_base} commit(s) behind it, none of its own)."
            ))
            .with_hint("Use a new branch name for new work, or work on the branch that still has commits."));
        }
        let remote = match self {
            Attach::Remote(r, _) => Some(r.clone()),
            _ => {
                let r = format!("origin/{branch}");
                git.succeeds(&["rev-parse", "--verify", "--quiet", &format!("refs/remotes/{r}")])
                    .unwrap_or(false)
                    .then_some(r)
            }
        };
        let vs_remote = match remote {
            Some(r) => {
                let (behind, ahead) = count(&format!("refs/remotes/{r}"), &tip)?;
                if behind > 0 && ahead == 0 {
                    return Err(CliError::refused(format!(
                        "The local branch `{branch}` is {behind} commit(s) behind {r}."
                    ))
                    .with_hint(format!(
                        "Fast-forward it first: `git fetch origin && git branch -f {branch} {r}` (safe: it has no commit of its own), then run add again."
                    )));
                }
                Some((r, ahead, behind))
            }
            None => None,
        };
        Ok(Some(Drift {
            ahead_base,
            behind_base,
            vs_remote,
        }))
    }

    fn describe(&self) -> Option<String> {
        match self {
            Attach::New => None,
            Attach::Local(_) => Some("local branch".to_string()),
            Attach::Remote(r, _) => Some(format!("remote branch {r}")),
        }
    }
}

/// How an attached branch stands against `--base` and its remote.
struct Drift {
    ahead_base: u64,
    behind_base: u64,
    /// Remote ref, commits ahead, commits behind.
    vs_remote: Option<(String, u64, u64)>,
}

impl Drift {
    fn json(&self) -> serde_json::Value {
        json!({"ahead_of_base": self.ahead_base, "behind_base": self.behind_base,
               "remote": self.vs_remote.as_ref().map(|(r, a, b)| json!({"ref": r, "ahead": a, "behind": b}))})
    }

    fn text(&self, base: &str) -> String {
        let mut t = format!("The branch is {} ahead of and {} behind {base}", self.ahead_base, self.behind_base);
        if let Some((r, a, b)) = &self.vs_remote {
            t.push_str(&format!("; {a} ahead of and {b} behind {r}"));
        }
        t + "."
    }
}

/// The folder name is one plain path component.
pub fn check_name(name: &str) -> Result<(), CliError> {
    let ok = !name.is_empty()
        && name.len() <= 100
        && !name.starts_with(['.', '-'])
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if ok {
        Ok(())
    } else {
        Err(CliError::usage(format!("`{name}` is not a worktree name.")).with_hint(
            "Use one folder name of letters, digits and `._-` (no `/`, no leading `.` or `-`); the tool places it under the worktree root.",
        ))
    }
}

/// Where the worktree root comes from: `--root`, then git config `paddock.worktreeRoot`, then `PADDOCK_WORKTREE_ROOT`.
pub fn resolve_root(repo: &Repo, arg: Option<&Path>, env: &dyn Fn(&str) -> Option<String>) -> Result<std::path::PathBuf, CliError> {
    if let Some(r) = arg {
        return Ok(r.to_path_buf());
    }
    if let Ok(v) = repo.git().text(&["config", "--get", "paddock.worktreeRoot"])
        && !v.trim().is_empty()
    {
        return Ok(v.trim().into());
    }
    if let Some(v) = env("PADDOCK_WORKTREE_ROOT").filter(|v| !v.is_empty()) {
        return Ok(v.into());
    }
    Err(CliError::usage("No worktree root is set, so there is no safe place to make the worktree.")
        .with_hint("Pass --root <folder>, or run `git config paddock.worktreeRoot <folder>` once, or set PADDOCK_WORKTREE_ROOT (for example /Volumes/d7sst7/dev-targets/wt)."))
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
        None if Path::new(&k).join(".git").is_file() => Err(CliError::refused(format!(
            "{k} has a `.git` file but git no longer lists it as a worktree of this repository."
        ))
        .with_hint(
            "An `add` whose checkout timed out or a prune does this. Move or delete the folder (check it holds nothing unpushed), then run `./dev worktree add` again with the same branch: it attaches to the existing branch. `git worktree list` shows what git knows.",
        )),
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

/// Git calls one `add` makes after its reservation: `worktree add`, `sparse-checkout set` and `checkout`.
const ADD_GIT_STEPS: u64 = 3;
// A live `add` must never outlast its reservation, or a second `add` could take the path while the first still runs.
const _: () = assert!(PENDING_TTL > super::git::GIT_TIMEOUT_SECS * ADD_GIT_STEPS + 60);

/// `add`: `git worktree add --lock --reason <owner note>` under the worktree root.
///
/// The owner map's lock is held only to check and reserve the path, and again to confirm it: the slow git work (a checkout of
/// a large tree) runs between the two with the lock free, so other claims never wait for it. **The reservation is the only
/// guard against two `add`s of one path**: git does not refuse an existing path atomically (the reviewer measured two raw
/// `git worktree add` of one path both succeeding in 5 of 30 runs). A reservation older than `PENDING_TTL` is a crashed
/// `add`; the compile-time assert below keeps that longer than any live `add` can run.
pub fn add(repo: &Repo, caller: &Caller, req: &AddRequest<'_>) -> Result<Report, CliError> {
    let (role, task) =
        check_owner(req.owner).map_err(|m| CliError::usage(m).with_hint("Write --owner implementer:RT-3 (role, a colon, task)."))?;
    check_name(req.name)?;
    if req.branch.is_some() && !req.sparse.is_empty() {
        return Err(CliError::usage("--sparse is for review copies (--detach); a branch worktree is a full checkout.")
            .with_hint(
                "The gates (conformance, affected.mjs, `gates check`) read most of the repository; a full checkout costs a little more disk and time (docs/benchmarks.md, \"Full against sparse worktree checkout\"). Drop --sparse, or add --detach for a review copy.",
            ));
    }
    let git = repo.git();
    let base_sha = git
        .text(&["rev-parse", "--verify", "--quiet", &format!("{}^{{commit}}", req.base)])
        .map_err(|_| {
            CliError::usage(format!("The base `{}` is not a commit here.", req.base))
                .with_hint("Fetch it (`git fetch origin`) or name another base.")
        })?
        .trim()
        .to_string();
    let attach = match req.branch {
        Some(b) => Attach::find(&git, b)?,
        None => Attach::New,
    };
    let drift = attach.drift(&git, req.base, &base_sha, req.branch.unwrap_or_default())?;
    // An existing branch keeps its own commits: the base it was made from is the branch's tip, not `--base`.
    let base_sha = match &attach {
        Attach::New => base_sha,
        Attach::Local(sha) | Attach::Remote(_, sha) => sha.clone(),
    };
    let k = key(&req.root.join(req.name));
    let branch_name = req.branch.unwrap_or("detached");
    let note = Note {
        role,
        task,
        branch: branch_name.to_string(),
        base: base_sha.clone(),
        created: Some(now_secs()),
        by: caller.by(),
    }
    .render();
    let store = OwnerStore::open(&repo.common_dir);
    // 1. Under the lock: check, then reserve.
    store
        .update_as("worktree add (reserve)", |map| -> Result<(), CliError> {
            check_nesting(repo, &k, map)?;
            if let Some(e) = map.get(&k) {
                return Err(
                    CliError::refused(format!("{k} is already in the owner map (agent {}).", e.agent_id))
                        .with_hint("Pick a new name; the coordinator frees an old entry with `./dev worktree reassign`."),
                );
            }
            if map.is_pending(&k, now_secs()) {
                return Err(CliError::refused(format!("{k} is being made by another `worktree add` right now."))
                    .with_hint("Pick another name, or run it again once that command has finished."));
            }
            if Path::new(&k).symlink_metadata().is_ok() {
                return Err(
                    CliError::refused(format!("{k} already exists.")).with_hint("Pick a new name; never reuse another agent's folder.")
                );
            }
            map.reserve(&k, now_secs());
            Ok(())
        })
        .map_err(owner_failure)??;
    // 2. Without the lock: make it. A failure ends the reservation.
    let made = make_worktree(&git, req, &attach, &k, &base_sha, &note);
    // 3. Under the lock again: confirm (claim for an agent caller) or remove the reservation.
    let confirmed = store.update_as("worktree add (confirm)", |map| {
        map.unreserve(&k);
        if made.is_ok()
            && let Some(a) = &caller.agent
        {
            map.claim(&k, &a.id, &a.kind, &format_utc(now_secs()));
        }
    });
    made?;
    confirmed.map_err(|e| {
        CliError::failure(format!(
            "The worktree {k} was made and locked, but its owner entry was not written: {e}"
        ))
        .with_hint(format!("Run `./dev worktree claim {k}` as the agent that works in it."))
    })?;
    let head = Git::at(Path::new(&k))
        .text(&["rev-parse", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or(base_sha);
    Ok(Report::new(
        json!({"path": k, "branch": req.branch, "head": head, "owner": req.owner, "locked": true, "note": note,
               "claimed_by": caller.agent.as_ref().map(|a| a.id.clone()), "attached": attach.describe(), "drift": drift.as_ref().map(Drift::json)}),
        format!(
            "Made {k} ({}{}), locked: {note}",
            req.branch.map_or("detached".to_string(), |b| format!("branch {b}")),
            attach
                .describe()
                .map_or(String::new(), |d| format!(", attached to the existing {d}"))
        ) + &drift.as_ref().map_or(String::new(), |d| format!("\n{}", d.text(req.base))),
    ))
}

/// Never inside, or around, the primary, another worktree, an owner-map path or a path another `add` has reserved. Run under
/// the lock against the worktrees as they are now, so two `add`s with different roots cannot nest.
fn check_nesting(repo: &Repo, k: &str, map: &OwnerMap) -> Result<(), CliError> {
    let list = worktrees(repo).map_err(git_failure)?;
    let mut others: Vec<(String, String)> = list
        .iter()
        .enumerate()
        .map(|(i, w)| {
            (
                w.path.replace('\\', "/"),
                if i == 0 { "the primary checkout" } else { "another worktree" }.to_string(),
            )
        })
        .collect();
    others.extend(map.iter().map(|(p, _)| (p.to_string(), "another worktree".to_string())));
    others.extend(
        map.pending_paths(now_secs())
            .into_iter()
            .map(|p| (p, "a worktree another `add` is making".to_string())),
    );
    for (other, what) in others {
        let (inside, around) = (is_under(k, &other), is_under(&other, k));
        // The very same path is judged by the checks after this one (existing folder, owner map, reservation).
        if (inside || around) && !(inside && around) {
            return Err(
                CliError::refused(format!("{k} is {} {other}, {what}.", if inside { "inside" } else { "around" }))
                    .with_hint("Choose a worktree root outside every checkout (`--root`, `paddock.worktreeRoot`)."),
            );
        }
    }
    Ok(())
}

/// `git worktree add --lock`, then the sparse cone (made without a checkout first, so the full tree is never written).
fn make_worktree(git: &Git, req: &AddRequest<'_>, attach: &Attach, k: &str, base_sha: &str, note: &str) -> Result<(), CliError> {
    let mut args = vec!["worktree", "add", "--lock", "--reason", note];
    if !req.sparse.is_empty() {
        args.push("--no-checkout");
    }
    let remote_ref;
    match (req.branch, attach) {
        (Some(b), Attach::New) => args.extend(["-b", b, k, base_sha]),
        (Some(b), Attach::Local(_)) => args.extend([k, b]),
        (Some(b), Attach::Remote(r, _)) => {
            remote_ref = r.clone();
            args.extend(["--track", "-b", b, k, &remote_ref]);
        }
        (None, _) => args.extend(["--detach", k, base_sha]),
    }
    git.text(&args).map_err(git_failure)?;
    if req.sparse.is_empty() {
        return Ok(());
    }
    let at = Git::at(Path::new(k));
    let mut s = vec!["sparse-checkout", "set", "--cone"];
    s.extend(req.sparse.iter().map(String::as_str));
    let steps = at.text(&s).and_then(|_| at.text(&["checkout", "--force", "HEAD"]));
    steps.map(|_| ()).map_err(|e| {
        CliError::failure(format!("The worktree {k} was made (locked) but the sparse checkout failed: {e}"))
            .with_hint("Fix the folder names and run `git sparse-checkout set --cone ...` in it.")
    })
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
            // Never removes work no remote has, and never with --force: staged, modified and untracked files and commits on
            // no remote branch each refuse (a failed commit chained before a removal once lost an agent's work).
            let state = WorkState::of(Path::new(&k)).map_err(|e| {
                git_failure(e)
                    .with_hint("The state of the worktree is unknown, so nothing was removed. Fix the git error, then drop again.")
            })?;
            let uncommitted = state.uncommitted();
            if !uncommitted.is_empty() {
                return Err(CliError::refused(format!(
                    "{k} has uncommitted changes or untracked files ({uncommitted}); nothing was removed."
                ))
                .with_hint("Commit and push them, or delete the files you do not need, then drop again."));
            }
            if state.unpushed > 0 {
                return Err(CliError::refused(format!(
                    "{k} has {} commit(s) that are on no remote branch; nothing was removed.",
                    state.unpushed
                ))
                .with_hint("Push them (or cherry-pick what you need elsewhere), then drop again."));
            }
            if let Some(reason) = &w.locked {
                repo.git().text(&["worktree", "unlock", &k]).map_err(git_failure)?;
                if let Err(e) = repo.git().text(&["worktree", "remove", &k]) {
                    let relock = repo.git().text(&["worktree", "lock", "--reason", reason, &k]);
                    let hint = match relock {
                        Ok(_) => "The lock was put back; nothing was removed.".to_string(),
                        Err(le) => {
                            format!("The lock could NOT be put back ({le}); run `git worktree lock --reason \"{reason}\" {k}` yourself.")
                        }
                    };
                    return Err(git_failure(e).with_hint(hint));
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
