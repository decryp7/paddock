//! Which files a project says to leave out (`.gitignore`), as a port: the shared walk of `paddock-repo-files` implements it in the
//! engine (ER-22), so this crate holds no ignore-rule parser.

use pacenotes_core::RepoPath;

/// Says whether the project's ignore rules leave a file or a folder out. A file the rules show but the denylist withholds stays
/// withheld: the denylist always wins.
pub trait Ignore: Sync {
    /// The file is not ignored.
    fn visible_file(&self, path: &RepoPath) -> bool;

    /// Something below the folder is not ignored, so the walk must go in. `false` lets the walk skip the whole folder.
    fn visible_dir(&self, path: &RepoPath) -> bool;
}

/// Nothing is ignored.
pub struct NoIgnore;

impl Ignore for NoIgnore {
    fn visible_file(&self, _: &RepoPath) -> bool {
        true
    }

    fn visible_dir(&self, _: &RepoPath) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_ignored_by_default() {
        let p = RepoPath::parse("a/b.rs").unwrap();
        assert!(NoIgnore.visible_file(&p) && NoIgnore.visible_dir(&p));
    }
}
