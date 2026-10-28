use crate::RepoFilesError;
use ignore::WalkBuilder;
use std::path::Path;

/// Bounds of a walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkLimits {
    /// Most files returned before the walk fails.
    pub max_files: usize,
    /// Deepest folder level followed.
    pub max_depth: usize,
}

impl Default for WalkLimits {
    fn default() -> WalkLimits {
        WalkLimits {
            max_files: 500_000,
            max_depth: 64,
        }
    }
}

/// A file found by [`walk`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RepoFile {
    /// Relative to the root, `/`-separated.
    pub path: String,
    /// Size in bytes.
    pub size: u64,
}

fn io_error(path: &Path, e: &ignore::Error) -> RepoFilesError {
    RepoFilesError::Io {
        path: path.to_path_buf(),
        source: std::io::Error::other(e.to_string()),
    }
}

/// Lists the files below `root`, sorted by path. `.gitignore` files apply (also without a git repository), `.git` is
/// skipped, hidden files are kept and symbolic links are not followed (a link is listed as a file only when it points to
/// one; a link to a folder is skipped).
pub fn walk(root: &Path, limits: WalkLimits) -> Result<Vec<RepoFile>, RepoFilesError> {
    if !root.is_dir() {
        return Err(RepoFilesError::NotAFolder(root.to_path_buf()));
    }
    let mut out = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .parents(false)
        .git_global(false)
        .git_exclude(false)
        .require_git(false)
        .follow_links(false)
        .max_depth(Some(limits.max_depth))
        .filter_entry(|e| e.file_name() != ".git")
        .build();
    for entry in walker {
        let entry = entry.map_err(|e| io_error(root, &e))?;
        let Some(ft) = entry.file_type() else {
            continue;
        };
        let Ok(rel) = entry.path().strip_prefix(root) else {
            continue;
        };
        if !ft.is_file() {
            continue;
        }
        if out.len() >= limits.max_files {
            return Err(RepoFilesError::TooManyFiles(limits.max_files));
        }
        let path = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        out.push(RepoFile {
            path,
            size: entry.metadata().map_or(0, |m| m.len()),
        });
    }
    out.sort();
    Ok(out)
}
