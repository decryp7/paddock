//! The git side of `./dev worktree`: running git, finding the repository and reading `git worktree list --porcelain`.

use crate::git::{Git, GitError};
use std::path::{Path, PathBuf};

/// The longest one git call may run, in seconds.
pub const GIT_TIMEOUT_SECS: u64 = crate::git::TIMEOUT_SECS;

/// The reader of a worktree command: the shared [`Git`] with optional locks off (a reader must not collide with the work it reads).
pub fn git_at(dir: &Path) -> Git<'static> {
    Git::owned(dir).without_optional_locks()
}

/// The repository the command works on.
#[derive(Debug, Clone)]
pub struct Repo {
    /// The common git directory (`.git` of the primary checkout), absolute.
    pub common_dir: PathBuf,
    /// The primary checkout: the folder that holds `common_dir`.
    pub primary: PathBuf,
}

impl Repo {
    /// Finds the repository from `from` with `git rev-parse --git-common-dir`, whichever worktree `from` is in.
    pub fn discover(from: &Path) -> Result<Repo, GitError> {
        let out = git_at(from).text(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
        let common = tool_dir(&canonical(Path::new(out.trim())), paddock_env::Os::current())?;
        let primary = common.parent().map(Path::to_path_buf).ok_or_else(|| GitError {
            args: "rev-parse --git-common-dir".into(),
            stderr: format!("{} has no parent folder", common.display()),
        })?;
        Ok(Repo {
            common_dir: common,
            primary,
        })
    }

    /// Git in the primary checkout.
    pub fn git(&self) -> Git<'static> {
        git_at(&self.primary)
    }
}

/// One line group of `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    /// The absolute path git prints.
    pub path: String,
    /// The commit checked out.
    pub head: String,
    /// The branch (without `refs/heads/`), or `None` when detached or bare.
    pub branch: Option<String>,
    /// HEAD is detached.
    pub detached: bool,
    /// Locked (with the reason, possibly empty).
    pub locked: Option<String>,
    /// The folder is gone (`git worktree prune` would remove the entry).
    pub prunable: bool,
}

/// Parses `git worktree list --porcelain`; the first entry is the primary checkout.
pub fn parse_worktrees(text: &str) -> Vec<WorktreeInfo> {
    let mut out: Vec<WorktreeInfo> = Vec::new();
    for line in text.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            out.push(WorktreeInfo {
                path: p.to_string(),
                head: String::new(),
                branch: None,
                detached: false,
                locked: None,
                prunable: false,
            });
            continue;
        }
        let Some(w) = out.last_mut() else { continue };
        if let Some(h) = line.strip_prefix("HEAD ") {
            w.head = h.to_string();
        } else if let Some(b) = line.strip_prefix("branch ") {
            w.branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
        } else if line == "detached" {
            w.detached = true;
        } else if line == "locked" {
            w.locked = Some(String::new());
        } else if let Some(r) = line.strip_prefix("locked ") {
            w.locked = Some(r.to_string());
        } else if line == "prunable" || line.starts_with("prunable ") {
            w.prunable = true;
        }
    }
    out
}

/// The worktrees of the repository, primary first.
pub fn worktrees(repo: &Repo) -> Result<Vec<WorktreeInfo>, GitError> {
    Ok(parse_worktrees(&repo.git().text(&["worktree", "list", "--porcelain"])?))
}

/// The real path of `p` (on Windows in the verbatim form `canonicalize` gives: compare it through [`key`], hand it to git through
/// [`tool_dir`]), or of its deepest existing folder with the rest appended (a worktree that is not made yet).
pub fn canonical(p: &Path) -> PathBuf {
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = p.to_path_buf();
    loop {
        if let Ok(mut out) = paddock_env::winpath::real_verbatim(&cur) {
            for part in rest.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (cur.file_name().map(std::ffi::OsString::from), cur.parent().map(Path::to_path_buf)) {
            (Some(name), Some(parent)) if !parent.as_os_str().is_empty() => {
                rest.push(name);
                cur = parent;
            }
            _ => return p.to_path_buf(),
        }
    }
}

/// `child` is `parent` or below it (both in [`key`] form), ignoring case where the file system does.
pub fn is_under(child: &str, parent: &str) -> bool {
    let fold = |s: &str| {
        if cfg!(any(target_os = "macos", windows)) {
            s.to_ascii_lowercase()
        } else {
            s.to_string()
        }
    };
    let (c, p) = (fold(child.trim_end_matches('/')), fold(parent.trim_end_matches('/')));
    c == p || c.starts_with(&format!("{p}/"))
}

/// What a worktree holds that no remote has: the work `drop` and `clean` must never remove.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct WorkState {
    /// Files with a change in the index.
    pub staged: u64,
    /// Tracked files modified (or deleted) in the folder.
    pub modified: u64,
    /// Files git does not track and does not ignore.
    pub untracked: u64,
    /// Commits reachable from HEAD that no remote-tracking branch has.
    pub unpushed: u64,
}

impl WorkState {
    /// Reads the worktree at `dir` with two git calls; an error is an unknown state, which callers treat as not removable.
    pub fn of(dir: &Path) -> Result<WorkState, GitError> {
        let git = git_at(dir);
        let mut s = WorkState::default();
        for line in git.text(&["status", "--porcelain"])?.lines() {
            let b = line.as_bytes();
            if b.len() < 2 {
                continue;
            }
            if b[0] == b'?' {
                s.untracked += 1;
                continue;
            }
            s.staged += u64::from(b[0] != b' ');
            s.modified += u64::from(b[1] != b' ');
        }
        s.unpushed = git
            .text(&["rev-list", "--count", "HEAD", "--not", "--remotes"])?
            .trim()
            .parse()
            .unwrap_or(1);
        Ok(s)
    }

    /// Nothing that only this folder has.
    pub fn is_clean(&self) -> bool {
        *self == WorkState::default()
    }

    /// The uncommitted part, for a message: `1 staged, 2 modified, 3 untracked`; empty when there is none.
    pub fn uncommitted(&self) -> String {
        let parts: Vec<String> = [(self.staged, "staged"), (self.modified, "modified"), (self.untracked, "untracked")]
            .iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, w)| format!("{n} {w}"))
            .collect();
        parts.join(", ")
    }
}

/// The path as the owner map and `git worktree list` spell it: absolute, real, with `/` separators and no `\\?\` prefix.
pub fn key(p: &Path) -> String {
    key_text(&canonical(p), paddock_env::Os::current())
}

/// [`key`] of an already resolved path, with the platform's rules given (lexical: the prefix goes whatever the length).
fn key_text(resolved: &Path, os: paddock_env::Os) -> String {
    paddock_env::winpath::lexical_text(resolved, os)
}

/// A resolved folder as a program may be given it (git runs in it): plain, or an error that names the cause and the next step.
fn tool_dir(resolved: &Path, os: paddock_env::Os) -> Result<PathBuf, GitError> {
    paddock_env::winpath::for_tools_native(resolved, os)
        .map(PathBuf::from)
        .map_err(|e| GitError {
            args: "rev-parse --git-common-dir".into(),
            stderr: e.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_worktree_reader_runs_with_hooks_off_and_unquoted_paths_like_every_other_reader() {
        let d = tempfile::tempdir().unwrap();
        let init = crate::git_env::git().arg("-C").arg(d.path()).args(["init", "-q"]).output().unwrap();
        assert!(init.status.success());
        let git = git_at(d.path());
        assert_eq!(git.text(&["config", "--get", "core.hooksPath"]).unwrap(), "/dev/null\n");
        assert_eq!(git.text(&["config", "--get", "core.quotePath"]).unwrap(), "false\n");
    }

    #[test]
    fn porcelain_is_parsed_with_locks_branches_and_detached_heads() {
        let text = "worktree /r\nHEAD aaa\nbranch refs/heads/main\n\nworktree /w/a\nHEAD bbb\nbranch refs/heads/feat/x\nlocked owner=x:y\n\n\
                    worktree /w/b\nHEAD ccc\ndetached\nlocked\n\nworktree /w/c\nHEAD ddd\ndetached\nprunable gitdir file points to non-existent location\n";
        let w = parse_worktrees(text);
        assert_eq!(w.len(), 4);
        assert_eq!(w[0].branch.as_deref(), Some("main"));
        assert_eq!(w[1].branch.as_deref(), Some("feat/x"));
        assert_eq!(w[1].locked.as_deref(), Some("owner=x:y"));
        assert!(w[2].detached && w[2].locked.as_deref() == Some("") && w[2].branch.is_none());
        assert!(w[3].prunable && w[3].locked.is_none());
    }

    #[test]
    fn under_means_the_same_folder_or_a_component_boundary() {
        assert!(is_under("/a/b", "/a/b") && is_under("/a/b/c", "/a/b") && is_under("/a/b/c/", "/a/b/"));
        assert!(!is_under("/a/bc", "/a/b") && !is_under("/a", "/a/b"));
    }

    #[test]
    fn a_path_that_does_not_exist_yet_resolves_through_its_existing_parent() {
        let d = tempfile::tempdir().unwrap();
        let real = paddock_env::winpath::real_verbatim(d.path()).unwrap();
        assert_eq!(canonical(&d.path().join("a").join("b")), real.join("a").join("b"));
    }

    /// 300 UTF-16 units: `\\?\C:\` and folders of 50 `a`.
    fn long_verbatim() -> String {
        let mut s = String::from(r"\\?\C:\");
        for _ in 0..6 {
            s.push_str(&"a".repeat(50));
            s.push('\\');
        }
        s.pop();
        s
    }

    #[test]
    fn the_key_of_a_300_unit_verbatim_path_has_no_prefix_and_matches_gits_spelling() {
        let long = long_verbatim();
        assert!(long.encode_utf16().count() >= 300);
        let k = key_text(Path::new(&long), paddock_env::Os::Windows);
        assert!(k.starts_with("C:/aaaa") && !k.contains('?') && !k.contains('\\'), "{k}");
        assert!(is_under(&k, &format!("C:/{}", "a".repeat(50))));
        assert_eq!(key_text(Path::new(r"\\?\C:\w\x"), paddock_env::Os::Windows), "C:/w/x");
    }

    #[test]
    fn a_resolved_folder_over_the_limit_is_not_handed_to_git_and_a_short_one_loses_its_prefix() {
        let e = tool_dir(Path::new(&long_verbatim()), paddock_env::Os::Windows).unwrap_err();
        assert!(e.stderr.contains("longer than Windows") && e.stderr.contains("shorter path"), "{e}");
        assert_eq!(
            tool_dir(Path::new(r"\\?\C:\w\.git"), paddock_env::Os::Windows).unwrap(),
            Path::new(r"C:\w\.git")
        );
    }
}
