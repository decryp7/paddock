//! The git side of `./dev worktree`: running git, finding the repository and reading `git worktree list --porcelain`.

use paddock_proc::{Limits, command, run_bounded};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The longest one git call may run.
pub const GIT_TIMEOUT_SECS: u64 = 120;
const TIMEOUT: Duration = Duration::from_secs(GIT_TIMEOUT_SECS);

/// A git command that failed, with the words that explain it.
#[derive(Debug)]
pub struct GitError {
    /// The git arguments (never secrets: only paths, refs and flags).
    pub args: String,
    /// What git printed on standard error (or why it could not run).
    pub stderr: String,
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "`git {}` failed: {}", self.args, self.stderr.trim())
    }
}

/// What a git run produced.
pub struct Out {
    /// Exit status 0.
    pub ok: bool,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
}

/// Git in one folder (`git -C <dir>`).
#[derive(Debug, Clone)]
pub struct Git {
    dir: PathBuf,
}

impl Git {
    /// Git run in `dir`.
    pub fn at(dir: &Path) -> Git {
        Git { dir: dir.to_path_buf() }
    }

    /// Runs git and returns what it printed whatever its status.
    pub fn run(&self, args: &[&str]) -> Result<Out, GitError> {
        let mut cmd = command("git");
        cmd.arg("-C").arg(&self.dir).args(args);
        let out = run_bounded(cmd, Limits::new(TIMEOUT).max_output(1 << 26)).map_err(|e| GitError {
            args: args.join(" "),
            stderr: e.to_string(),
        })?;
        if !matches!(out.outcome, paddock_proc::Outcome::Exited(_)) {
            return Err(GitError {
                args: args.join(" "),
                stderr: format!("did not finish ({:?})", out.outcome),
            });
        }
        Ok(Out {
            ok: out.success(),
            stdout: out.stdout_text(),
            stderr: out.stderr_text(),
        })
    }

    /// Runs git; a non-zero status is an error carrying git's message.
    pub fn text(&self, args: &[&str]) -> Result<String, GitError> {
        let out = self.run(args)?;
        if out.ok {
            Ok(out.stdout)
        } else {
            Err(GitError {
                args: args.join(" "),
                stderr: out.stderr,
            })
        }
    }

    /// Runs git for a yes/no question (`merge-base --is-ancestor`): exit 0 is true, anything else false.
    pub fn succeeds(&self, args: &[&str]) -> Result<bool, GitError> {
        Ok(self.run(args)?.ok)
    }
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
        let out = Git::at(from).text(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
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
    pub fn git(&self) -> Git {
        Git::at(&self.primary)
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
        let real = paddock_env::winpath::real_or_given(d.path());
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
