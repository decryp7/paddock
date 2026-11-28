//! What a run of a check is about ([`Subject`]): the commit and its tree, or the working tree as it is now (repo layer: the push gate, the
//! generator checks, the conformance host and `ci landing-held` all name their subject through it).

use crate::git::git_text;
use paddock_cli::Subject;
use paddock_cli::outcome::{LiveRepo, Sha, TreeId};
use std::path::Path;

/// The repository as it is now, for `Evidence::adopt`.
pub struct GitRepo<'a>(pub &'a Path);

impl LiveRepo for GitRepo<'_> {
    fn tree_of(&self, sha: &Sha) -> Option<TreeId> {
        git_text(self.0, &["rev-parse", "--verify", "--quiet", &format!("{}^{{tree}}", sha.as_str())]).map(|t| TreeId::new(t.trim()))
    }
}

/// What a run of the gates at `root` is about: the commit and its tree when `sha` names one, else the working tree.
pub fn subject_of(root: &Path, sha: Option<&str>) -> Subject {
    match sha.and_then(|s| GitRepo(root).tree_of(&Sha::new(s)).map(|t| (s, t))) {
        Some((s, tree)) => Subject::Commit { sha: Sha::new(s), tree },
        None => working_tree(root),
    }
}

/// The head and a SHA-256 of the working tree's state (`git diff HEAD` and the untracked files' names): what a generator check
/// that reads files is a check of. A git that cannot answer gives `unknown`, which no consumer matches to a commit.
pub fn working_tree(root: &Path) -> Subject {
    use sha2::{Digest, Sha256};
    let git = crate::git::Git::new(root);
    let head = git
        .text(&["rev-parse", "HEAD"])
        .map(|t| t.trim().to_string())
        .unwrap_or_else(|_| "unknown".into());
    let mut h = Sha256::new();
    for args in [&["diff", "HEAD"][..], &["ls-files", "--others", "--exclude-standard"][..]] {
        h.update(git.text(args).unwrap_or_default());
    }
    let hex = crate::input_hash::hex(&h.finalize());
    Subject::Files {
        head: paddock_cli::outcome::Sha::new(head),
        digest: paddock_cli::outcome::Sha256::new(hex),
    }
}
