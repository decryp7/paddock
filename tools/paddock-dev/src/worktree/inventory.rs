//! `list` and `clean`: what each worktree is, and the removal of the ones nobody holds (ADR 0083 §1).
//!
//! `clean` removes only a worktree that is **unlocked**, **merged** into the base, **clean** and **not in use**, and only
//! with `git worktree remove` (never `rm`, never `--force`): git itself refuses a locked or modified worktree, so a
//! mistake in these checks still cannot delete one. Locked ones are listed with their owner note and kept.

use super::git::{Git, Repo, WorktreeInfo, worktrees};
use super::note::{Note, format_utc, now_secs};
use super::ops::{git_failure, owner_failure};
use super::owners::{Entry, OwnerStore};
use super::procs;
use paddock_cli::{CliError, Report};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// A lock older than this is reported (ADR 0083 D3), never removed.
pub const STALE_LOCK_SECS: u64 = 7 * 24 * 3600;

/// Whether the worktree's work is in the base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Merged {
    /// Its commits are in the base.
    Yes,
    /// It has commits the base lacks.
    No,
    /// Its branch is still at the commit it was made from.
    Empty,
    /// Cannot tell (no base, or no branch history).
    Unknown,
}

impl Merged {
    /// The word `list` prints.
    pub fn word(self) -> &'static str {
        match self {
            Merged::Yes => "yes",
            Merged::No => "no",
            Merged::Empty => "empty",
            Merged::Unknown => "unknown",
        }
    }
}

/// Everything `list` and `clean` know about one worktree.
#[derive(Debug, Clone)]
pub struct Row {
    /// Git's view of it.
    pub info: WorktreeInfo,
    /// The primary checkout.
    pub primary: bool,
    /// Its parsed lock note, when the lock has one of ours.
    pub note: Option<Note>,
    /// Who claimed it.
    pub owner: Option<Entry>,
    /// Seconds since the lock note's `created`.
    pub lock_age: Option<u64>,
    /// Merged into the base.
    pub merged: Merged,
    /// Uncommitted or untracked files (`None`: the folder is missing).
    pub dirty: Option<bool>,
    /// A process has its current folder inside (`None`: not known here).
    pub in_use: Option<bool>,
}

impl Row {
    /// The path in the form the owner map uses.
    pub fn key(&self) -> String {
        self.info.path.replace('\\', "/")
    }
}

fn merged_state(repo: &Repo, w: &WorktreeInfo, base: &str) -> Merged {
    let git = repo.git();
    let base_ok = git
        .succeeds(&["rev-parse", "--verify", "--quiet", &format!("{base}^{{commit}}")])
        .unwrap_or(false);
    if !base_ok || w.head.is_empty() {
        return Merged::Unknown;
    }
    if !git.succeeds(&["merge-base", "--is-ancestor", &w.head, base]).unwrap_or(false) {
        return Merged::No;
    }
    let Some(branch) = &w.branch else { return Merged::Yes };
    // A branch made from the base and never committed to is an ancestor of it too: tell it from a merged one by its first
    // reflog entry (the commit it was created at), as worktree.sh tells it by BASE_SHA.
    let log = git
        .text(&["reflog", "show", "--format=%H", &format!("refs/heads/{branch}")])
        .unwrap_or_default();
    match log.lines().last() {
        None => Merged::Unknown,
        Some(first) if first.trim() == w.head => Merged::Empty,
        Some(_) => Merged::Yes,
    }
}

/// Reads every worktree: git's data, the lock note, the owner map, merged, dirty and in-use.
pub fn rows(repo: &Repo, base: &str, with_in_use: bool) -> Result<Vec<Row>, CliError> {
    let list = worktrees(repo).map_err(git_failure)?;
    let owners = OwnerStore::open(&repo.common_dir).read().map_err(owner_failure)?;
    let folders = if with_in_use { Some(procs::process_folders()) } else { None };
    let now = now_secs();
    let mut out = Vec::new();
    for (i, mut w) in list.iter().cloned().enumerate() {
        // git marks a missing folder prunable only when it is not locked; a missing folder is missing either way.
        if !Path::new(&w.path).exists() {
            w.prunable = true;
        }
        let k = w.path.replace('\\', "/");
        let note = w.locked.as_deref().and_then(Note::parse);
        let dirty = if w.prunable {
            None
        } else {
            Git::at(Path::new(&w.path))
                .text(&["status", "--porcelain"])
                .ok()
                .map(|s| !s.trim().is_empty())
        };
        let in_use = match &folders {
            Some(Ok(f)) if !w.prunable => Some(procs::any_inside(f, &PathBuf::from(&w.path))),
            _ => None,
        };
        out.push(Row {
            primary: i == 0,
            lock_age: note.as_ref().map(|n| now.saturating_sub(n.created)),
            merged: merged_state(repo, &w, base),
            owner: owners.get(&k).cloned(),
            note,
            dirty,
            in_use,
            info: w,
        });
    }
    Ok(out)
}

fn row_json(r: &Row) -> Value {
    json!({
        "path": r.key(),
        "primary": r.primary,
        "branch": r.info.branch,
        "detached": r.info.detached,
        "head": r.info.head,
        "locked": r.info.locked.is_some(),
        "lock_note": r.info.locked,
        "owner_note": r.note.as_ref().map(|n| format!("{}:{}", n.role, n.task)),
        "created": r.note.as_ref().map(|n| format_utc(n.created)),
        "agent_id": r.owner.as_ref().map(|e| e.agent_id.clone()),
        "agent_type": r.owner.as_ref().map(|e| e.agent_type.clone()),
        "lock_age_secs": r.lock_age,
        "stale_lock": r.lock_age.is_some_and(|a| a > STALE_LOCK_SECS),
        "merged": r.merged.word(),
        "dirty": r.dirty,
        "in_use": r.in_use,
        "prunable": r.info.prunable,
    })
}

/// `list`: every worktree with branch, lock note, claimed agent, lock age, merged, dirty and in-use.
pub fn list(repo: &Repo, base: &str) -> Result<Report, CliError> {
    let rows = rows(repo, base, true)?;
    let mut text = String::new();
    for r in &rows {
        let state = match r.dirty {
            Some(true) => "dirty",
            Some(false) => "clean",
            None => "missing",
        };
        let lock = match (&r.info.locked, &r.note) {
            (None, _) => "unlocked".to_string(),
            (Some(_), Some(n)) => format!("locked {}:{}", n.role, n.task),
            (Some(reason), None) => format!("locked ({})", if reason.is_empty() { "no note" } else { reason }),
        };
        let agent = r.owner.as_ref().map_or("-", |e| e.agent_id.as_str());
        let age = r.lock_age.map_or("-".to_string(), |a| format!("{}d", a / 86_400));
        let use_ = match r.in_use {
            Some(true) => "in-use",
            Some(false) => "idle",
            None => "use?",
        };
        text.push_str(&format!(
            "{}{}  {}  {}  agent {}  age {}  merged {}  {}  {}\n",
            r.key(),
            if r.primary { " (primary)" } else { "" },
            r.info.branch.as_deref().unwrap_or(if r.info.detached { "(detached)" } else { "-" }),
            lock,
            agent,
            age,
            r.merged.word(),
            state,
            use_,
        ));
    }
    Ok(Report::new(
        json!({"base": base, "worktrees": rows.iter().map(row_json).collect::<Vec<_>>()}),
        text,
    ))
}

/// Why `clean` keeps a worktree; `None` when it may be removed.
pub fn keep_reason(r: &Row, base: &str) -> Option<String> {
    if r.primary {
        return Some("the primary checkout".to_string());
    }
    if r.info.prunable {
        return Some("its folder is missing (git worktree prune)".to_string());
    }
    if let Some(reason) = &r.info.locked {
        return Some(match &r.note {
            Some(n) => format!(
                "locked: owner {}:{}{}",
                n.role,
                n.task,
                r.owner.as_ref().map_or(String::new(), |e| format!(", agent {}", e.agent_id))
            ),
            None => format!("locked: {}", if reason.is_empty() { "no note" } else { reason }),
        });
    }
    match r.merged {
        Merged::Yes => {}
        Merged::No => return Some(format!("not merged into {base}")),
        Merged::Empty => return Some("no commits yet".to_string()),
        Merged::Unknown => return Some(format!("cannot tell whether it is merged into {base}")),
    }
    if r.dirty != Some(false) {
        return Some("uncommitted changes".to_string());
    }
    match r.in_use {
        Some(false) => None,
        Some(true) => Some("a process has its current folder inside".to_string()),
        None => Some("whether a process uses it is not known here".to_string()),
    }
}

/// `clean`: removes the worktrees [`keep_reason`] frees, with their merged branch.
pub fn clean(repo: &Repo, base: &str, dry_run: bool) -> Result<Report, CliError> {
    let rows = rows(repo, base, true)?;
    let store = OwnerStore::open(&repo.common_dir);
    let (mut removed, mut kept) = (Vec::new(), Vec::new());
    let mut text = String::new();
    for r in rows.iter().filter(|r| !r.primary) {
        let k = r.key();
        if let Some(why) = keep_reason(r, base) {
            text.push_str(&format!("keep {k} ({why})\n"));
            kept.push(json!({"path": k, "reason": why}));
            continue;
        }
        if dry_run {
            text.push_str(&format!(
                "would remove {k}{}\n",
                r.info.branch.as_ref().map_or(String::new(), |b| format!(", branch {b}"))
            ));
            removed.push(json!({"path": k, "branch": r.info.branch, "dry_run": true}));
            continue;
        }
        // No --force: git refuses what is modified, and a locked worktree was already kept above.
        if let Err(e) = repo.git().text(&["worktree", "remove", &k]) {
            let why = format!("git would not remove it: {}", e.stderr.trim());
            text.push_str(&format!("keep {k} ({why})\n"));
            kept.push(json!({"path": k, "reason": why}));
            continue;
        }
        let _ = store.update(|m| m.remove(&k));
        let mut branch_note = String::new();
        if let Some(b) = &r.info.branch {
            // Safe: the tip is an ancestor of the base, so no commit is lost; `-d` would compare with the primary's HEAD.
            match repo.git().text(&["branch", "-D", b]) {
                Ok(_) => branch_note = format!(", branch {b}"),
                Err(e) => branch_note = format!(", branch {b} kept ({})", e.stderr.trim()),
            }
        }
        text.push_str(&format!("removed {k}{branch_note}\n"));
        removed.push(json!({"path": k, "branch": r.info.branch}));
    }
    Ok(Report::new(
        json!({"base": base, "dry_run": dry_run, "removed": removed, "kept": kept}),
        text,
    ))
}
