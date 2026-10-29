//! Opening a repository that may be hostile (ADR 0004).

use std::path::{Path, PathBuf};

use crate::history::GixHistory;

/// How many levels of alternates are followed (git's own limit).
const MAX_ALTERNATE_DEPTH: usize = 5;

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
    /// The alternates nest deeper than git follows; a chain that long is not read.
    #[error("the repository's alternates nest more than {MAX_ALTERNATE_DEPTH} levels deep ({0}); it is not read")]
    AlternatesTooDeep(PathBuf),
    /// gix could not open the repository.
    #[error("the repository could not be opened: {0}")]
    Backend(String),
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

/// Checks the alternates of the object folder `objects`, recursively; they must all lie inside `root`.
fn check_alternates(root: &Path, objects: &Path, depth: usize) -> Result<(), OpenError> {
    let file = objects.join("info/alternates");
    let Ok(text) = std::fs::read_to_string(&file) else {
        return Ok(());
    };
    if depth >= MAX_ALTERNATE_DEPTH {
        return Err(OpenError::AlternatesTooDeep(objects.to_path_buf()));
    }
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let target = resolve(&objects.join(line));
        if !inside(root, &target) {
            return Err(OpenError::OutsideWorkspace {
                what: "alternates",
                path: target,
            });
        }
        check_alternates(root, &target, depth + 1)?;
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
    check_alternates(&root, &objects, 0)?;

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
