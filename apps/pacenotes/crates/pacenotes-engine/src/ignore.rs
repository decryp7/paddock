//! `.gitignore` rules for the walk, from the shared file walker.

use pacenotes_core::RepoPath;
use pacenotes_index::Ignore;
use paddock_repo_files::{RepoFilesError, WalkLimits, walk};
use std::collections::HashSet;
use std::path::Path;

/// The walk of the ignore rules failed.
#[derive(Debug, thiserror::Error)]
#[error("could not read the ignore rules: {0}")]
pub struct IgnoreError(#[from] RepoFilesError);

/// Which files and folders `.gitignore` leaves visible, learned from one [`paddock_repo_files::walk`] of the workspace.
///
/// The shared walker applies the rules (nested `.gitignore` files, negations, no git repository needed) and skips `.git`.
/// A folder is visible when a visible file lies below it, so an ignored folder is never entered by the index's own walk,
/// and a folder with nothing visible in it is left out. The cost is a second walk of the tree; it is measured in
/// `docs/performance.md`.
pub struct RepoFilesIgnore {
    files: HashSet<String>,
    dirs: HashSet<String>,
}

impl RepoFilesIgnore {
    /// Walks `root` with the shared walker.
    pub fn scan(root: &Path, limits: WalkLimits) -> Result<RepoFilesIgnore, IgnoreError> {
        Ok(Self::from_paths(walk(root, limits)?.into_iter().map(|f| f.path)))
    }

    /// The visible set from relative `/`-separated file paths.
    pub fn from_paths(paths: impl IntoIterator<Item = String>) -> RepoFilesIgnore {
        let mut files = HashSet::new();
        let mut dirs = HashSet::new();
        for path in paths {
            let mut end = 0;
            while let Some(i) = path[end..].find('/') {
                end += i;
                dirs.insert(path[..end].to_owned());
                end += 1;
            }
            files.insert(path);
        }
        RepoFilesIgnore { files, dirs }
    }
}

impl Ignore for RepoFilesIgnore {
    fn visible_file(&self, path: &RepoPath) -> bool {
        self.files.contains(path.as_str())
    }

    fn visible_dir(&self, path: &RepoPath) -> bool {
        self.dirs.contains(path.as_str())
    }
}
