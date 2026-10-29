//! Opening a repository that may be hostile (ADR 0004).

use std::path::{Path, PathBuf};

use crate::history::GixHistory;

/// How many alternate object folders are followed.
const MAX_ALTERNATES: usize = 8;
/// How many entries of a git folder are checked for symbolic links.
const MAX_GIT_DIR_ENTRIES: usize = 1_000_000;
/// The top-level folders of a git folder that gix reads; the walk descends only into these.
const READ_BY_GIX: [&str; 3] = ["objects", "refs", "info"];
/// How deep the walk of a git folder goes (a git folder is `objects/ab/cdef...`, a few levels).
const MAX_GIT_DIR_DEPTH: usize = 12;

/// Why a workspace's repository was not opened.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OpenError {
    /// The workspace has no readable git repository (`.git`).
    #[error("{0} is not a git repository, or cannot be read")]
    NotARepository(PathBuf),
    /// Something in the repository leads outside the workspace, so the repository is not read.
    #[error(
        "the repository's {what} leads outside the workspace ({path}); it is not read. Move the repository inside the workspace or remove the link."
    )]
    OutsideWorkspace {
        /// What leads out: `gitdir`, `commondir`, `objects` or `alternates`.
        what: &'static str,
        /// Where it points.
        path: PathBuf,
    },
    /// More alternate object folders than are followed.
    #[error("the repository borrows objects from more than {MAX_ALTERNATES} folders ({0}); it is not read")]
    AlternatesTooDeep(PathBuf),
    /// An alternate object folder outside the git folder. It is inside the workspace but nothing checks it for symbolic links,
    /// so it is not read; objects borrowed from another folder must live under `.git`.
    #[error("the repository borrows objects from {0}, outside its git folder; it is not read. Keep alternates under .git.")]
    AlternateOutsideGitDir(PathBuf),
    /// A symbolic link inside the git folder. Git never creates one there, and one can lead anywhere.
    #[error("the git folder holds a symbolic link ({0}); git never creates one there, so the repository is not read")]
    SymlinkInGitDir(PathBuf),
    /// The git folder has more entries than are checked.
    #[error("the git folder holds more than {MAX_GIT_DIR_ENTRIES} entries ({0}); it is not read")]
    GitDirTooLarge(PathBuf),
    /// gix could not open the repository.
    #[error("the repository could not be opened: {0}")]
    Backend(String),
}

fn io_err(path: &Path, e: std::io::Error) -> OpenError {
    OpenError::Backend(format!("{}: {e}", path.display()))
}

fn inside(root: &Path, path: &Path) -> bool {
    path.starts_with(root)
}

/// `path` with `.` and `..` applied without touching the disk.
fn lexical(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolves `path` as far as it exists (following symlinks), then appends the rest, so a target that does not exist is still
/// judged by where it would be.
fn resolve(path: &Path) -> PathBuf {
    if let Ok(real) = path.canonicalize() {
        return real;
    }
    let flat = lexical(path);
    let mut existing = flat.clone();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    while !existing.exists() {
        match (
            existing.file_name().map(ToOwned::to_owned),
            existing.parent().map(Path::to_path_buf),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent;
            }
            _ => break,
        }
    }
    let mut out = existing.canonicalize().unwrap_or(existing);
    out.extend(rest.iter().rev());
    out
}

fn read_first_line(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.lines().next().unwrap_or("").trim().to_owned())
}

/// Checks the alternates of the object folder `objects` with gix's own resolver, the code that will read them, so the check
/// and the read agree on quoting, escapes, relative paths and nesting. Every folder it returns must lie inside `root`
/// (`OutsideWorkspace`), and under one of `git_dirs` (`AlternateOutsideGitDir`): those are the folders walked for symbolic
/// links, and an alternate elsewhere could hold a link (to its `pack` folder) that leads out.
fn check_alternates(root: &Path, objects: &Path, git_dirs: &[&Path]) -> Result<(), OpenError> {
    let dirs = gix::odb::alternate::resolve(objects.to_path_buf(), root).map_err(|e| OpenError::Backend(e.to_string()))?;
    if dirs.len() > MAX_ALTERNATES {
        return Err(OpenError::AlternatesTooDeep(objects.to_path_buf()));
    }
    let targets: Vec<PathBuf> = dirs.iter().map(|d| resolve(d)).collect();
    if let Some(out) = targets.iter().find(|t| !inside(root, t)) {
        return Err(OpenError::OutsideWorkspace {
            what: "alternates",
            path: out.clone(),
        });
    }
    match targets.iter().find(|t| !git_dirs.iter().any(|g| inside(g, t))) {
        Some(elsewhere) => Err(OpenError::AlternateOutsideGitDir(elsewhere.clone())),
        None => Ok(()),
    }
}

/// The entries of `folder` with what each is (from `symlink_metadata`, never following a link): `(path, is_symlink, is_dir)`.
fn entries_of(folder: &Path) -> Result<Vec<(PathBuf, bool, bool)>, OpenError> {
    let read = || -> std::io::Result<Vec<(PathBuf, bool, bool)>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(folder)? {
            let path = entry?.path();
            let meta = std::fs::symlink_metadata(&path)?;
            out.push((path, meta.file_type().is_symlink(), meta.is_dir()));
        }
        Ok(out)
    };
    read().map_err(|e| io_err(folder, e))
}

/// Refuses any symbolic link under `dir`, walking without following links. Git creates none inside a git folder; a link
/// there (to `objects/pack`, `packed-refs`, `refs/heads/x`) would make the read leave the workspace. Bounded in entries
/// and depth.
fn check_no_symlinks(dir: &Path, max_entries: usize, max_depth: usize) -> Result<(), OpenError> {
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    let mut seen = 0usize;
    while let Some((folder, depth)) = stack.pop() {
        for (path, is_symlink, is_dir) in entries_of(&folder)? {
            seen += 1;
            if seen > max_entries || (is_dir && depth + 1 > max_depth) {
                return Err(OpenError::GitDirTooLarge(dir.to_path_buf()));
            }
            if is_symlink {
                return Err(OpenError::SymlinkInGitDir(path));
            }
            // At the top of a git folder only what gix reads is descended into: hooks, logs, worktrees and modules are never
            // opened (the entries themselves are still checked above, so a link named `hooks` is refused).
            if is_dir && (depth > 0 || path.file_name().is_some_and(|n| READ_BY_GIX.iter().any(|r| n == *r))) {
                stack.push((path, depth + 1));
            }
        }
    }
    Ok(())
}

/// Opens the repository of `workspace` for reading history. See the module and ADR 0004 for what is refused and what gix is
/// never asked to do.
pub fn open(workspace: &Path) -> Result<GixHistory, OpenError> {
    let root = workspace
        .canonicalize()
        .map_err(|_| OpenError::NotARepository(workspace.to_path_buf()))?;
    let dot_git = root.join(".git");
    if std::fs::symlink_metadata(&dot_git).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(OpenError::SymlinkInGitDir(dot_git));
    }
    let gitdir = if dot_git.is_dir() {
        resolve(&dot_git)
    } else if dot_git.is_file() {
        let text = read_first_line(&dot_git).ok_or_else(|| OpenError::NotARepository(root.clone()))?;
        let target = text
            .strip_prefix("gitdir:")
            .map(str::trim)
            .ok_or_else(|| OpenError::NotARepository(root.clone()))?;
        resolve(&root.join(target))
    } else {
        return Err(OpenError::NotARepository(root));
    };
    if !inside(&root, &gitdir) {
        return Err(OpenError::OutsideWorkspace {
            what: "gitdir",
            path: gitdir,
        });
    }
    let mut common = gitdir.clone();
    if let Some(line) = read_first_line(&gitdir.join("commondir")) {
        common = resolve(&gitdir.join(line));
        if !inside(&root, &common) {
            return Err(OpenError::OutsideWorkspace {
                what: "commondir",
                path: common,
            });
        }
    }
    let objects = resolve(&common.join("objects"));
    if !inside(&root, &objects) {
        return Err(OpenError::OutsideWorkspace {
            what: "objects",
            path: objects,
        });
    }
    // links first (so the alternates resolver below never follows one), then the alternates
    check_no_symlinks(&gitdir, MAX_GIT_DIR_ENTRIES, MAX_GIT_DIR_DEPTH)?;
    if common != gitdir {
        check_no_symlinks(&common, MAX_GIT_DIR_ENTRIES, MAX_GIT_DIR_DEPTH)?;
    }
    check_alternates(&root, &objects, &[&gitdir, &common])?;

    let options = gix::open::Options::isolated()
        .open_path_as_is(true)
        .with(gix::sec::Trust::Reduced)
        .strict_config(false)
        .lossy_config(true)
        .config_overrides(["core.fsmonitor=false"]);
    let repo = gix::ThreadSafeRepository::open_opts(&gitdir, options).map_err(|e| OpenError::Backend(e.to_string()))?;
    Ok(GixHistory::new(repo, root))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_symlink_walk_is_bounded_in_entries_and_depth() {
        let scratch = paddock_testkit::Scratch::new();
        let root = scratch.path("g");
        std::fs::create_dir_all(root.join("objects/b/c")).unwrap();
        for i in 0..5 {
            std::fs::write(root.join(format!("f{i}")), "x").unwrap();
        }
        assert!(check_no_symlinks(&root, 100, 10).is_ok());
        let too_many = check_no_symlinks(&root, 4, 10);
        assert!(matches!(too_many, Err(OpenError::GitDirTooLarge(_))));
        let too_deep = check_no_symlinks(&root, 100, 2);
        assert!(matches!(too_deep, Err(OpenError::GitDirTooLarge(_))));
        assert!(matches!(
            check_no_symlinks(&scratch.path("missing"), 100, 10),
            Err(OpenError::Backend(_))
        ));
    }

    #[test]
    fn lexical_applies_dot_and_dotdot_and_resolve_survives_an_empty_path() {
        assert_eq!(lexical(Path::new("./a/../b")), PathBuf::from("b"));
        assert_eq!(lexical(Path::new("/a/b/../../c/.")), PathBuf::from("/c"));
        let _ = resolve(Path::new(""));
    }

    #[test]
    fn resolve_judges_a_missing_target_by_where_it_would_be() {
        let tmp = std::env::temp_dir();
        let missing = tmp.join("pn-no-such-folder").join("a").join("..").join("b");
        let r = resolve(&missing);
        assert!(r.ends_with("pn-no-such-folder/b"), "{r:?}");
        assert!(inside(&tmp.canonicalize().unwrap(), &r));
        let up = resolve(&tmp.join("pn-no-such-folder/../../elsewhere"));
        assert!(!inside(&tmp.canonicalize().unwrap(), &up), "{up:?}");
    }
}
