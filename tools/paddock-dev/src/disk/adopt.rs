//! `disk adopt`: writes owner records for the folders that already exist and can be identified: the repository's git
//! worktrees (from `git worktree list`) and the build folders whose name matches a worktree. Everything else stays an orphan.

use super::config::{CommonArgs, Settings};
use super::record::{self, Kind, Record};
use super::scan::discover;
use crate::worktree::git::{Repo, key, worktrees};
use crate::worktree::note::{Note, now_secs};
use crate::worktree::owners::OwnerStore;
use clap::Args;
use paddock_cli::{CliError, Report};
use serde_json::json;
use std::path::{Path, PathBuf};

/// `disk adopt`'s arguments.
#[derive(Args, Debug, Clone)]
pub struct AdoptArgs {
    #[command(flatten)]
    /// The shared settings flags.
    pub common: CommonArgs,
    /// The repository whose worktrees are adopted (default: the current folder's).
    #[arg(long)]
    pub repo: Option<PathBuf>,
    /// Say what would be written and write nothing.
    #[arg(long)]
    pub dry_run: bool,
}

/// One worktree as adoption sees it.
#[derive(Debug, Clone)]
pub struct Wt {
    /// The folder.
    pub path: PathBuf,
    /// Its owner text (`role:task`, an agent id, or `unknown`).
    pub owner: String,
    /// The branch (empty when detached).
    pub branch: String,
    /// HEAD.
    pub head: String,
}

fn mtime(p: &Path) -> u64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or_else(now_secs, |d| d.as_secs())
}

/// The worktree a build folder named `name` serves: the one whose folder name equals it or is followed by `-` in it; the longest
/// such name wins (`ci-in-rust-crates-target` belongs to `ci-in-rust-crates`, not `ci-in-rust`).
pub fn match_worktree<'a>(name: &str, wts: &'a [Wt]) -> Option<&'a Wt> {
    wts.iter()
        .filter(|w| {
            let n = w.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            !n.is_empty() && (name == n || name.starts_with(&format!("{n}-")))
        })
        .max_by_key(|w| w.path.file_name().map_or(0, |n| n.len()))
}

/// Runs `adopt`.
pub fn run(a: &AdoptArgs, env: &dyn Fn(&str) -> Option<String>) -> Result<Report, CliError> {
    let s = Settings::resolve(&a.common, env);
    let from = match &a.repo {
        Some(r) => r.clone(),
        None => std::env::current_dir().map_err(|e| CliError::failure(format!("Cannot read the current folder: {e}.")))?,
    };
    let repo = Repo::discover(&from)
        .map_err(|e| CliError::failure(e.to_string()).with_hint("Run it inside the repository, or pass --repo <folder>."))?;
    let list = worktrees(&repo).map_err(|e| CliError::failure(e.to_string()))?;
    let owners = OwnerStore::open(&repo.common_dir).read().ok();
    let mut wts = Vec::new();
    let mut written = Vec::new();
    let mut text = String::new();
    let common = repo.common_dir.display().to_string();
    for (i, w) in list.iter().enumerate() {
        let path = PathBuf::from(&w.path);
        if i == 0 || !path.is_dir() {
            continue;
        }
        let note = w.locked.as_deref().and_then(Note::parse);
        let agent = owners.as_ref().and_then(|m| m.get(&key(&path))).map(|e| e.agent_id.clone());
        let owner = match (&note, agent) {
            (Some(n), _) => format!("{}:{}", n.role, n.task),
            (None, Some(agent)) => agent,
            (None, None) => "unknown".to_string(),
        };
        let wt = Wt {
            path: path.clone(),
            owner: owner.clone(),
            branch: w.branch.clone().unwrap_or_default(),
            head: w.head.clone(),
        };
        if matches!(record::read(&path), Ok(Some(_))) {
            wts.push(wt);
            continue;
        }
        let mut r = Record::new(Kind::Worktree, &owner);
        r.branch = wt.branch.clone();
        r.head = wt.head.clone();
        r.repo = common.clone();
        r.created = note.as_ref().and_then(|n| n.created).unwrap_or_else(|| mtime(&path));
        r.last_used = mtime(&path).max(r.created);
        write_one(&path, &r, a.dry_run, &mut text, &mut written, &repo)?;
        wts.push(wt);
    }
    let worktree_paths: Vec<String> = wts.iter().map(|w| key(&w.path)).collect();
    for f in discover(&s) {
        if f.record.is_some() || worktree_paths.contains(&key(&f.path)) {
            continue;
        }
        let name = f.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let Some(w) = match_worktree(name, &wts) else { continue };
        let kind = if name.contains("tmp") || name.contains("scratch") {
            Kind::Scratch
        } else {
            Kind::Build
        };
        let mut r = Record::new(kind, &w.owner);
        r.branch = w.branch.clone();
        r.head = w.head.clone();
        r.repo = common.clone();
        r.link = w.path.display().to_string();
        r.created = mtime(&f.path);
        r.last_used = r.created;
        write_one(&f.path, &r, a.dry_run, &mut text, &mut written, &repo)?;
    }
    text.push_str(&format!(
        "{} {} owner record(s); every other folder stays an orphan in `./dev disk report`.\n",
        if a.dry_run { "Would write" } else { "Wrote" },
        written.len()
    ));
    Ok(Report::new(json!({"dry_run": a.dry_run, "written": written}), text))
}

fn write_one(
    dir: &Path,
    r: &Record,
    dry: bool,
    text: &mut String,
    written: &mut Vec<serde_json::Value>,
    repo: &Repo,
) -> Result<(), CliError> {
    let line = format!(
        "{} {} (owner {}{})",
        if dry { "would record" } else { "recorded" },
        dir.display(),
        r.owner,
        if r.link.is_empty() {
            String::new()
        } else {
            format!(", serves {}", r.link)
        }
    );
    if !dry {
        if r.kind == Kind::Worktree {
            record::exclude_in_repo(&repo.common_dir).map_err(CliError::failure)?;
        }
        record::write(dir, r).map_err(CliError::failure)?;
    }
    text.push_str(&line);
    text.push('\n');
    written.push(json!({"path": dir.display().to_string(), "kind": r.kind.word(), "owner": r.owner, "link": r.link}));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wt(name: &str) -> Wt {
        Wt {
            path: PathBuf::from(format!("/wt/{name}")),
            owner: format!("implementer:{name}"),
            branch: String::new(),
            head: String::new(),
        }
    }

    #[test]
    fn a_build_folder_belongs_to_the_worktree_with_the_longest_matching_name() {
        let wts = vec![wt("ci-in-rust"), wt("ci-in-rust-crates"), wt("fixes-1")];
        assert_eq!(
            match_worktree("ci-in-rust-crates-target", &wts).unwrap().owner,
            "implementer:ci-in-rust-crates"
        );
        assert_eq!(
            match_worktree("ci-in-rust-target-xwin", &wts).unwrap().owner,
            "implementer:ci-in-rust"
        );
        assert_eq!(match_worktree("fixes-1", &wts).unwrap().owner, "implementer:fixes-1");
        assert!(match_worktree("sccache", &wts).is_none());
        assert!(match_worktree("ci-in-rustic", &wts).is_none(), "a longer word is not a match");
    }
}
