//! The one "safe to remove" check (user decision 2026-11-12): every tool that deletes a folder under the work disks' dev-targets
//! roots or a worktree path goes through [`remove`], and it deletes only on positive proof, worked out again right before the
//! removal and never taken from an index or an earlier look:
//!
//! - the folder has an owner record ([`crate::disk::record`]);
//! - it lies strictly below an allowed root, reached without a symlink (the folder itself is no symlink and its real path is
//!   below the real root);
//! - no process has its current folder at or below it;
//! - every git repository or worktree at any depth below it (a `.git` file or folder) is clean (no modified or untracked
//!   files), has no stash, has a remote, and has no commit that is on no remote; a linked worktree is also not locked and not
//!   claimed by an agent.
//!
//! A fact that cannot be established is a refusal, never a pass: an unreadable process list (every process list on Windows),
//! a git error, a repository without any remote, a walk that hit its limit. `tests/it/safe_remove_lint.rs` fails when other
//! code in `tools/` deletes folders without going through here (apart from the listed callers that adopt it later).

use crate::disk::record;
use crate::worktree::git::{Git, Repo, key, worktrees};
use crate::worktree::owners::OwnerStore;
use crate::worktree::procs;
use paddock_env::winpath;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The longest the walk for nested repositories runs for one folder.
pub const WALK_LIMIT: Duration = Duration::from_secs(300);
/// The most entries that walk looks at before it refuses.
pub const WALK_ENTRIES: u64 = 20_000_000;

/// What was proved about a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proof {
    /// The folder is a linked git worktree (its `.git` is a file): it is removed through `git worktree remove`.
    pub linked_worktree: bool,
    /// The folders holding a `.git`, the folder itself included when it is one.
    pub repos: Vec<PathBuf>,
}

/// Why a folder was kept.
pub type Refusal = String;

/// The current folders of running processes as [`crate::worktree::procs::process_folders`] answers (an error on Windows).
pub type ProcessFolders = Result<Vec<PathBuf>, String>;

/// Checks `path` against `roots`, reading the process list now.
pub fn check(path: &Path, roots: &[PathBuf]) -> Result<Proof, Refusal> {
    check_with(path, roots, procs::process_folders())
}

/// [`check`] with the process list given (tests; [`remove`] reads it fresh).
pub fn check_with(path: &Path, roots: &[PathBuf], processes: ProcessFolders) -> Result<Proof, Refusal> {
    check_limited(path, roots, processes, WALK_LIMIT, WALK_ENTRIES)
}

/// [`check_with`] with the walk's limits given.
pub fn check_limited(path: &Path, roots: &[PathBuf], processes: ProcessFolders, limit: Duration, entries: u64) -> Result<Proof, Refusal> {
    let real = under_root(path, roots)?;
    match record::read(path) {
        Ok(Some(_)) => {}
        Ok(None) => return Err("it has no owner record".into()),
        Err(e) => return Err(format!("its owner record is unreadable ({e})")),
    }
    let folders = processes.map_err(|e| format!("the process list cannot be read ({e}), so whether something uses it is not known"))?;
    if procs::any_inside(&folders, path) || procs::any_inside(&folders, &real) {
        return Err("a process has its current folder inside".into());
    }
    let repos = find_repos(&real, limit, entries)?;
    for r in &repos {
        repo_proof(r, &real)?;
    }
    let linked_worktree = std::fs::symlink_metadata(real.join(".git")).is_ok_and(|m| m.is_file());
    Ok(Proof { linked_worktree, repos })
}

/// The real path of `path`, when it is a real folder strictly below a root and no part of it is a symlink.
fn under_root(path: &Path, roots: &[PathBuf]) -> Result<PathBuf, Refusal> {
    let meta = path.symlink_metadata().map_err(|e| format!("it cannot be read ({e})"))?;
    if meta.file_type().is_symlink() {
        return Err("it is a symlink".into());
    }
    if !meta.is_dir() {
        return Err("it is not a folder".into());
    }
    // Both sides of every comparison below are `real_verbatim`: the same spelling, whatever the prefix.
    let real = winpath::real_verbatim(path).map_err(|e| format!("its real path cannot be resolved ({e})"))?;
    let parent_real = path
        .parent()
        .and_then(|p| winpath::real_verbatim(p).ok())
        .ok_or_else(|| "its parent cannot be resolved".to_string())?;
    if real.parent() != Some(parent_real.as_path()) || real.file_name() != path.file_name() {
        return Err("its path goes through a symlink".into());
    }
    let allowed = roots
        .iter()
        .filter_map(|r| winpath::real_verbatim(r).ok())
        .any(|r| real.starts_with(&r) && real != r);
    if !allowed {
        return Err("it is not strictly below an allowed root".into());
    }
    Ok(real)
}

/// Every folder at or below `dir` that holds a `.git` entry. Symlinks are not followed and `.git` folders are not entered; a
/// walk that runs out of time or entries is a refusal.
fn find_repos(dir: &Path, limit: Duration, max_entries: u64) -> Result<Vec<PathBuf>, Refusal> {
    let deadline = Instant::now() + limit;
    let mut seen = 0u64;
    let mut repos = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = std::fs::read_dir(&d).map_err(|e| format!("{} cannot be read ({e})", d.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("{} cannot be listed ({e})", d.display()))?;
            seen += 1;
            if seen > max_entries || (seen.is_multiple_of(512) && Instant::now() >= deadline) {
                return Err("looking for git repositories inside took too long, so none can be ruled out".into());
            }
            let p = entry.path();
            if entry.file_name() == ".git" {
                repos.push(d.clone());
                continue;
            }
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(p);
            }
        }
    }
    repos.sort();
    Ok(repos)
}

fn git_fail(repo: &Path, what: &str, e: impl std::fmt::Display) -> Refusal {
    format!("git could not say whether {} has {what} ({e})", repo.display())
}

/// The proof for one repository or worktree: clean, no stash, a remote, nothing unpushed; a linked worktree is also unlocked and
/// unclaimed.
fn repo_proof(repo_dir: &Path, top: &Path) -> Result<(), Refusal> {
    let git = Git::at(repo_dir);
    let named = |m: &str| {
        format!(
            "{}{m}",
            if repo_dir == top {
                String::new()
            } else {
                format!("{}: ", repo_dir.display())
            }
        )
    };
    let status = git
        .text(&[
            "--no-optional-locks",
            "status",
            "--porcelain",
            "--untracked-files=normal",
            "--",
            ".",
            &format!(":(exclude){}", record::FILE),
        ])
        .map_err(|e| named(&git_fail(repo_dir, "uncommitted work", e)))?;
    if !status.trim().is_empty() {
        return Err(named("it holds uncommitted or untracked files"));
    }
    let stash = git
        .run(&["rev-parse", "--verify", "--quiet", "refs/stash"])
        .map_err(|e| named(&git_fail(repo_dir, "a stash", e)))?;
    if stash.ok {
        return Err(named("it holds a stash"));
    }
    let remotes = git
        .text(&["for-each-ref", "--format=%(refname)", "refs/remotes"])
        .map_err(|e| named(&git_fail(repo_dir, "a remote", e)))?;
    if remotes.trim().is_empty() {
        return Err(named("it has no remote branch, so nothing in it is known to be pushed"));
    }
    let linked = std::fs::symlink_metadata(repo_dir.join(".git")).is_ok_and(|m| m.is_file());
    // A linked worktree shares its branches with the primary: only its HEAD is its own. A full repository answers for all its branches.
    let scope = if linked { "HEAD" } else { "--branches" };
    let ahead = git
        .text(&["rev-list", "--count", scope, "--not", "--remotes"])
        .map_err(|e| named(&git_fail(repo_dir, "unpushed commits", e)))?;
    match ahead.trim().parse::<u64>() {
        Ok(0) => {}
        Ok(n) => return Err(named(&format!("it has {n} commit(s) that are on no remote"))),
        Err(_) => return Err(named("git's count of unpushed commits was not a number")),
    }
    if linked {
        let repo = Repo::discover(repo_dir).map_err(|e| named(&git_fail(repo_dir, "its repository", e)))?;
        let me = key(repo_dir);
        let list = worktrees(&repo).map_err(|e| named(&git_fail(repo_dir, "its worktrees", e)))?;
        let entry = list
            .iter()
            .find(|w| key(Path::new(&w.path)) == me)
            .ok_or_else(|| named("git does not list it as a worktree"))?;
        if entry.locked.is_some() {
            return Err(named("it is a locked worktree"));
        }
        let claimed = OwnerStore::open(&repo.common_dir)
            .read()
            .map_err(|e| named(&format!("the owner map cannot be read ({e})")))?
            .get(&me)
            .is_some();
        if claimed {
            return Err(named("an agent has claimed it"));
        }
    }
    Ok(())
}

/// What [`remove`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    /// The folder.
    pub path: PathBuf,
    /// What was proved.
    pub proof: Proof,
}

/// Removes `path` if, and only if, [`check`] passes now. A linked worktree goes through `git worktree remove` (no force) and
/// leaves the owner map; anything else through `remove_dir_all`.
pub fn remove(path: &Path, roots: &[PathBuf]) -> Result<Removed, Refusal> {
    remove_with(path, roots, procs::process_folders())
}

/// [`remove`] with the process list given.
pub fn remove_with(path: &Path, roots: &[PathBuf], processes: ProcessFolders) -> Result<Removed, Refusal> {
    let proof = check_with(path, roots, processes)?;
    if proof.linked_worktree {
        let repo = Repo::discover(path).map_err(|e| format!("its repository cannot be found ({e})"))?;
        let k = key(path);
        repo.git()
            .text(&["worktree", "remove", &k])
            .map_err(|e| format!("git would not remove it: {}", e.stderr.trim()))?;
        if let Some(warning) = forget_owner(&repo.common_dir, &k) {
            eprintln!("{warning}");
        }
    } else {
        std::fs::remove_dir_all(path).map_err(|e| format!("it could not be removed: {e}"))?; // the one deleting call
    }
    Ok(Removed {
        path: path.to_path_buf(),
        proof,
    })
}

/// Drops the removed worktree's entry from the owner map. A map that cannot be updated leaves a stale entry behind, which
/// is returned as the `::warning::` line to print (the removal itself already happened).
fn forget_owner(common_dir: &Path, key: &str) -> Option<String> {
    OwnerStore::open(common_dir).update(|m| m.remove(key)).err().map(|e| {
        format!("::warning::the worktree {key} was removed but its owner-map entry could not be dropped ({e}); `./dev worktree list` shows it until the map is repaired")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::record::{Kind, Record};

    fn recorded(root: &Path, name: &str) -> PathBuf {
        let p = root.join(name);
        std::fs::create_dir_all(&p).unwrap();
        record::write(&p, &Record::new(Kind::Build, "implementer:t")).unwrap();
        p
    }

    fn roots(d: &tempfile::TempDir) -> Vec<PathBuf> {
        vec![d.path().join("root")]
    }

    fn setup() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        (d, root)
    }

    #[test]
    fn a_recorded_folder_with_no_process_and_no_repository_is_removed() {
        let (d, root) = setup();
        let p = recorded(&root, "x");
        std::fs::write(p.join("blob"), b"x").unwrap();
        let r = remove_with(&p, &roots(&d), Ok(vec![])).unwrap();
        assert!(!r.proof.linked_worktree && r.proof.repos.is_empty());
        assert!(!p.exists());
    }

    #[test]
    fn no_record_means_no_removal() {
        let (d, root) = setup();
        std::fs::create_dir_all(root.join("bare")).unwrap();
        assert!(
            remove_with(&root.join("bare"), &roots(&d), Ok(vec![]))
                .unwrap_err()
                .contains("no owner record")
        );
        assert!(root.join("bare").is_dir());
    }

    #[test]
    fn a_folder_outside_every_root_or_a_root_itself_is_never_removed() {
        let (d, root) = setup();
        let outside = recorded(d.path(), "elsewhere");
        assert!(check_with(&outside, &roots(&d), Ok(vec![])).unwrap_err().contains("allowed root"));
        record::write(&root, &Record::new(Kind::Build, "x")).unwrap();
        assert!(check_with(&root, &roots(&d), Ok(vec![])).unwrap_err().contains("allowed root"));
        assert!(outside.is_dir() && root.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_never_followed_neither_as_the_folder_nor_in_its_path() {
        let (d, root) = setup();
        let real = recorded(d.path(), "real-outside");
        std::os::unix::fs::symlink(&real, root.join("link")).unwrap();
        assert!(
            check_with(&root.join("link"), &roots(&d), Ok(vec![]))
                .unwrap_err()
                .contains("symlink")
        );
        // A folder reached through a symlinked parent that leads out of the root.
        std::fs::create_dir_all(d.path().join("other/inner")).unwrap();
        record::write(&d.path().join("other/inner"), &Record::new(Kind::Build, "x")).unwrap();
        std::os::unix::fs::symlink(d.path().join("other"), root.join("hop")).unwrap();
        let e = check_with(&root.join("hop/inner"), &roots(&d), Ok(vec![])).unwrap_err();
        assert!(e.contains("symlink") || e.contains("allowed root"), "{e}");
        assert!(real.is_dir() && d.path().join("other/inner").is_dir());
    }

    #[test]
    fn a_process_inside_or_an_unreadable_process_list_refuses() {
        let (d, root) = setup();
        let p = recorded(&root, "busy");
        let inside = vec![p.join("sub")];
        assert!(check_with(&p, &roots(&d), Ok(inside)).unwrap_err().contains("current folder"));
        let e = check_with(&p, &roots(&d), Err("no /proc".into())).unwrap_err();
        assert!(e.contains("process list cannot be read") && e.contains("no /proc"), "{e}");
        assert!(p.is_dir());
    }

    #[test]
    fn a_walk_that_hits_its_limit_refuses_instead_of_guessing() {
        let (d, root) = setup();
        let p = recorded(&root, "big");
        for n in 0..20 {
            std::fs::write(p.join(format!("f{n}")), b"x").unwrap();
        }
        let e = check_limited(&p, &roots(&d), Ok(vec![]), WALK_LIMIT, 5).unwrap_err();
        assert!(e.contains("took too long"), "{e}");
    }

    #[test]
    fn a_git_folder_at_any_depth_is_found() {
        let (_d, root) = setup();
        std::fs::create_dir_all(root.join("a/b/c/.git")).unwrap();
        std::fs::create_dir_all(root.join("a/x")).unwrap();
        std::fs::write(root.join("a/x/.git"), "gitdir: /nowhere").unwrap();
        let found = find_repos(&root, WALK_LIMIT, WALK_ENTRIES).unwrap();
        assert_eq!(found, vec![root.join("a/b/c"), root.join("a/x")]);
    }

    #[test]
    fn an_owner_map_that_cannot_be_updated_is_a_warning_not_a_silent_leftover() {
        let d = tempfile::tempdir().unwrap();
        let not_a_folder = d.path().join("file");
        std::fs::write(&not_a_folder, b"x").unwrap();
        let w = forget_owner(&not_a_folder, "/some/worktree").expect("a warning");
        assert!(w.starts_with("::warning::"), "{w}");
        assert!(w.contains("/some/worktree"), "{w}");
        assert!(forget_owner(d.path(), "/some/worktree").is_none());
    }
}
